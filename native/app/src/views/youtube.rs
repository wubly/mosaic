//! The YouTube section: liked videos and playlists of the connected Google account. Searching
//! lives in Discover, and playing a video needs no account (see `services::youtube::innertube`).

use iced::widget::{column, container, row, text, Space};
use iced::{Element, Length, Task};
use mosaic_core::model::Track;
use mosaic_services::youtube::api::{Library, PlaylistCard};
use mosaic_services::youtube::auth::Status;
use rand::seq::SliceRandom;

use super::common::{
    ghost_button, page, primary_button, tonal_button, virtual_rows, visible_range,
};
use super::spotify::{songs, total_length};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::widgets::icon::Icon;

#[derive(Clone, Debug)]
pub enum Msg {
    Status(Status),
    Connect,
    Connected(Result<Status, String>),
    Disconnect,
    ClientId(String),
    ClientSecret(String),
    Library(Box<Library>),
    Open(PlaylistCard),
    Opened(String, Result<Vec<Track>, String>),
    Back,
    PlayAll,
    ShuffleAll,
    EnqueueAll,
    /// Find and open the video of a track (menu, player bar).
    Watch(Box<Track>),
    Watched(Box<Track>, Result<Option<String>, String>),
}

/// A playlist on screen and its tracks once loaded (or why they didn't).
type OpenPlaylist = (PlaylistCard, Option<Result<Vec<Track>, String>>);

#[derive(Default)]
pub struct State {
    pub status: Status,
    pub connecting: bool,
    pub error: String,
    pub client_id: String,
    pub client_secret: String,
    pub library: Library,
    pub library_loaded: bool,
    pub open: Option<OpenPlaylist>,
}

impl App {
    /// Whether YouTube tracks play: the beta is on. Playback itself needs no Google account.
    pub(crate) fn youtube_playable(&self) -> bool {
        self.services.config().beta == Some(true)
    }

    /// Whether the account section is shown: the beta is on and Google is connected.
    pub(crate) fn youtube_enabled(&self) -> bool {
        self.youtube_playable() && self.ui.youtube.status.connected
    }

    pub(crate) fn youtube_boot(&mut self) -> Task<Message> {
        let on = self.youtube_playable();
        let availability = self.player_input(PlayerInput::YoutubeAvailable(on));
        if !on {
            return availability;
        }
        let config = self.services.config();
        self.ui.youtube.client_id = config.youtube_client_id.unwrap_or_default();
        self.ui.youtube.client_secret.clear();
        let services = self.services.clone();
        Task::batch([
            availability,
            Task::perform(async move { services.yt_status().await }, |s| {
                Message::Youtube(Msg::Status(s))
            }),
        ])
    }

    pub(crate) fn youtube_boot_or_clear(&mut self) -> Task<Message> {
        if self.youtube_playable() {
            return self.youtube_boot();
        }
        self.ui.youtube = State::default();
        let leave = if self.view_kind() == crate::widgets::rail::View::YouTube {
            self.update(Message::Nav(crate::widgets::rail::View::Discover))
        } else {
            Task::none()
        };
        Task::batch([
            self.player_input(PlayerInput::YoutubeAvailable(false)),
            leave,
        ])
    }

    pub(crate) fn youtube_ensure_loaded(&mut self) -> Task<Message> {
        if !self.youtube_enabled() || self.ui.youtube.library_loaded {
            return Task::none();
        }
        self.youtube_load_library()
    }

    fn youtube_load_library(&mut self) -> Task<Message> {
        self.ui.youtube.library_loaded = true;
        let services = self.services.clone();
        Task::perform(async move { services.yt_library().await }, |l| {
            Message::Youtube(Msg::Library(Box::new(l)))
        })
    }

    /// Tracks of the playlist on screen.
    pub(crate) fn youtube_open_tracks(&self) -> Vec<Track> {
        match &self.ui.youtube.open {
            Some((_, Some(Ok(tracks)))) => tracks.clone(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn on_youtube(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Status(status) => {
                let was = self.ui.youtube.status.connected;
                self.ui.youtube.status = status;
                if self.view_kind() == crate::widgets::rail::View::YouTube {
                    if self.youtube_enabled() {
                        return self.youtube_ensure_loaded();
                    }
                    self.ui.view = Some(crate::widgets::rail::View::Discover);
                    self.retarget_rail();
                } else if self.youtube_enabled() && !was {
                    self.retarget_rail();
                }
                Task::none()
            }
            Msg::ClientId(id) => {
                self.services
                    .yt_set_credentials(&id, &self.ui.youtube.client_secret);
                self.ui.youtube.client_id = id;
                Task::none()
            }
            Msg::ClientSecret(secret) => {
                self.services
                    .yt_set_credentials(&self.ui.youtube.client_id, &secret);
                self.ui.youtube.client_secret = secret;
                Task::none()
            }
            Msg::Connect => {
                self.ui.youtube.connecting = true;
                self.ui.youtube.error.clear();
                let services = self.services.clone();
                Task::perform(
                    async move {
                        services
                            .yt_connect(|url| {
                                mosaic_platform::open::open_https(url);
                            })
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |r| Message::Youtube(Msg::Connected(r)),
                )
            }
            Msg::Connected(result) => {
                self.ui.youtube.connecting = false;
                match result {
                    Ok(status) => {
                        self.ui.youtube.library_loaded = false;
                        self.toast("YouTube connected");
                        self.on_youtube(Msg::Status(status))
                    }
                    Err(e) => {
                        self.toast(e.clone());
                        self.ui.youtube.error = e;
                        Task::none()
                    }
                }
            }
            Msg::Disconnect => {
                self.services.yt_disconnect();
                let client_id = self.ui.youtube.client_id.clone();
                self.ui.youtube = State {
                    client_id,
                    ..State::default()
                };
                self.on_youtube(Msg::Status(Status::default()))
            }
            Msg::Library(library) => {
                self.ui.youtube.library = *library;
                Task::none()
            }
            Msg::Open(card) => {
                let id = card.id.clone();
                self.ui.youtube.open = Some((card, None));
                self.ui.scroll.0 = 0.0;
                let services = self.services.clone();
                Task::perform(
                    async move {
                        let tracks = services.yt_playlist(&id).await.map_err(|e| e.to_string());
                        (id, tracks)
                    },
                    |(id, tracks)| Message::Youtube(Msg::Opened(id, tracks)),
                )
            }
            Msg::Opened(id, result) => {
                if let Some((card, slot)) = &mut self.ui.youtube.open {
                    if card.id == id {
                        *slot = Some(result);
                    }
                }
                Task::none()
            }
            Msg::Back => {
                self.ui.youtube.open = None;
                Task::none()
            }
            Msg::PlayAll | Msg::ShuffleAll => {
                let shuffle = matches!(msg, Msg::ShuffleAll);
                let mut list = self.visible_youtube_tracks();
                if shuffle {
                    list.shuffle(&mut rand::rng());
                }
                match list.first().cloned() {
                    Some(t) => self.player_input(PlayerInput::Play(t, Some(list))),
                    None => Task::none(),
                }
            }
            Msg::Watch(track) => {
                self.toast("Looking for the video…");
                let services = self.services.clone();
                Task::perform(
                    async move {
                        let found = services
                            .yt_video_for(&track)
                            .await
                            .map_err(|e| e.to_string());
                        (track, found)
                    },
                    |(track, found)| Message::Youtube(Msg::Watched(track, found)),
                )
            }
            Msg::Watched(track, result) => match result {
                Ok(Some(id)) => self.open_video(&track, &id),
                Ok(None) => {
                    self.toast("This song has no video on YouTube");
                    Task::none()
                }
                Err(e) => {
                    self.toast(e);
                    Task::none()
                }
            },
            Msg::EnqueueAll => {
                let list = self.visible_youtube_tracks();
                self.toast(format!("Added {} to the queue", songs(list.len() as u64)));
                self.player_input(PlayerInput::EnqueueMany(list))
            }
        }
    }

    /// Opens a video in the browser. When it is the video of what's playing it starts where
    /// the music is, and Mosaic pauses so the two don't play over each other.
    fn open_video(&mut self, track: &Track, id: &str) -> Task<Message> {
        let mut url = format!("https://www.youtube.com/watch?v={id}");
        let playing_it = self.player.track.as_ref().is_some_and(|t| t.id == track.id);
        if playing_it && track.video_id.as_deref() == Some(id) {
            url.push_str(&format!("&t={}s", self.player.time.current.max(0.0) as u64));
        }
        if !mosaic_platform::open::open_https(&url) {
            self.toast("Could not open the browser");
            return Task::none();
        }
        if playing_it && self.player.playing {
            return self.player_input(PlayerInput::Toggle);
        }
        Task::none()
    }

    fn visible_youtube_tracks(&self) -> Vec<Track> {
        if self.ui.youtube.open.is_some() {
            self.youtube_open_tracks()
        } else {
            self.ui.youtube.library.liked.clone()
        }
    }

    fn youtube_actions(&self, p: Palette) -> Element<'_, Message> {
        row![
            primary_button("Play", Icon::PlayArrow, p, Message::Youtube(Msg::PlayAll)),
            tonal_button(
                "Shuffle",
                Icon::Shuffle,
                p,
                Message::Youtube(Msg::ShuffleAll)
            ),
            tonal_button(
                "Add to queue",
                Icon::QueueMusic,
                p,
                Message::Youtube(Msg::EnqueueAll)
            ),
        ]
        .spacing(8)
        .into()
    }

    fn youtube_rows<'a>(
        &'a self,
        tracks: &'a [Track],
        which: ListRef,
        top: f32,
        p: Palette,
    ) -> Element<'a, Message> {
        let range = visible_range(tracks.len(), self.ui.scroll.0, self.ui.scroll.1, top);
        virtual_rows(tracks.len(), range, |i| {
            self.track_row(
                &tracks[i],
                i,
                TrackRef {
                    list: which.clone(),
                    index: i,
                },
                true,
                p,
            )
        })
    }

    pub(crate) fn youtube_view(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.youtube;
        let h3 = |t: &str| {
            container(text(t.to_string()).size(16).color(p.on_surface_variant)).padding(
                iced::Padding {
                    top: 32.0,
                    bottom: 16.0,
                    ..iced::Padding::ZERO
                },
            )
        };
        let error = |e: &str| {
            container(text(e.to_string()).size(14))
                .padding([12, 16])
                .width(Length::Fill)
                .style(styles::error_box(p))
        };
        let mut body = column![];

        if let Some((card, slot)) = &s.open {
            body = body
                .push(ghost_button(
                    "Back",
                    Icon::ArrowBack,
                    p,
                    Message::Youtube(Msg::Back),
                ))
                .push(self.page_header(card.name.clone(), None, Vec::new(), p));
            match slot {
                None => body = body.push(h3("Loading…")),
                Some(Err(e)) => body = body.push(error(e)),
                Some(Ok(tracks)) => {
                    body = body
                        .push(
                            text(format!(
                                "{} · {}",
                                songs(tracks.len() as u64),
                                total_length(tracks)
                            ))
                            .size(14)
                            .color(p.on_surface_variant),
                        )
                        .push(Space::new().height(12))
                        .push(self.youtube_actions(p));
                    if tracks.is_empty() {
                        body = body.push(h3("Nothing playable in this playlist"));
                    }
                    body = body.push(Space::new().height(16)).push(self.youtube_rows(
                        tracks,
                        ListRef::YoutubePlaylist,
                        340.0,
                        p,
                    ));
                }
            }
            return page(body, p);
        }

        body = body.push(self.page_header("YouTube".into(), s.status.user.clone(), Vec::new(), p));
        let lib = &s.library;
        if !s.library_loaded {
            body = body.push(h3("Loading…"));
        }
        if !lib.playlists.is_empty() {
            body = body.push(h3("Your playlists")).push(Self::shelf_row(
                lib.playlists.iter().map(|l| {
                    self.card(
                        &l.image,
                        l.name.clone(),
                        format!("Playlist · {}", songs(l.count)),
                        false,
                        Message::Youtube(Msg::Open(l.clone())),
                        p,
                    )
                }),
                p,
            ));
        }
        if !lib.liked.is_empty() {
            body = body
                .push(h3("Liked videos"))
                .push(self.youtube_actions(p))
                .push(Space::new().height(16))
                .push(self.youtube_rows(&lib.liked, ListRef::YoutubeLiked, 420.0 + 120.0, p));
        }
        if s.library_loaded && lib.playlists.is_empty() && lib.liked.is_empty() {
            body = body.push(h3(
                "Nothing here yet. Your liked videos and playlists show up here.",
            ));
        }
        page(body, p)
    }
}
