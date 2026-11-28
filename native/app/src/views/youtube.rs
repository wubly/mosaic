//! The YouTube section: liked videos and playlists of the connected Google account. Searching
//! lives in Discover, and playing a video needs no account (see `services::youtube::innertube`).

use iced::widget::{column, container, image, opaque, row, text, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::Track;
use mosaic_services::youtube::api::{Library, PlaylistCard};
use mosaic_services::youtube::auth::{Account, Status};
use mosaic_services::youtube::home::YtHome;
use mosaic_services::youtube::innertube::VideoStream;
use mosaic_services::youtube::library::YtLibrary;
use rand::seq::SliceRandom;

use super::common::{
    ghost_button, page, primary_button, tonal_button, virtual_rows, visible_range,
};
use super::spotify::{songs, total_length};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::video::Decoder;
use crate::widgets::icon::Icon;
use crate::widgets::spinner::spinner;

#[derive(Clone, Debug)]
pub enum Msg {
    /// Open the Google sign-in window.
    GoogleSignIn,
    GoogleSignedIn(Result<Option<Account>, String>),
    GoogleSignOut,
    /// Copy the signed-in account's likes and playlists into Mosaic's library.
    ImportLibrary,
    Imported(Result<Box<YtLibrary>, String>),
    Status(Status),
    Connect,
    Connected(Result<Status, String>),
    Disconnect,
    ClientId(String),
    ClientSecret(String),
    Library(Box<Library>),
    /// The signed-in account's recently played, home shelves, likes, playlists and artists.
    Home(Result<Box<YtHome>, String>),
    Open(PlaylistCard),
    Opened(String, Result<Vec<Track>, String>),
    Back,
    PlayAll,
    ShuffleAll,
    EnqueueAll,
    /// Find and open the video of a track (menu, player bar).
    Watch(Box<Track>),
    Watched(Box<Track>, Result<Option<String>, String>),
    /// The picture-only stream of a video was resolved (or why not).
    Stream(Box<Track>, String, Result<VideoStream, String>),
    /// Close the in-app video.
    CloseVideo,
    /// Open the video being watched in the browser instead.
    VideoInBrowser,
}

/// The video on screen. The sound is the engine's; this only draws the picture that goes with
/// it (see `crate::video`).
pub struct Video {
    pub track: Track,
    pub id: String,
    pub stream: VideoStream,
    pub decoder: Option<Decoder>,
    pub frame: Option<iced::widget::image::Handle>,
    /// The audio clock the picture was last matched to.
    pub clock: f64,
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
    pub video: Option<Video>,
    /// The Google account signed in through the sign-in window.
    pub account: Option<Account>,
    pub signing_in: bool,
    pub importing: bool,
    /// What the signed-in account has on YouTube Music (`None` until asked for).
    pub home: Option<Result<YtHome, String>>,
    pub home_loading: bool,
    /// The browser the Google sign-in opens (looked up once at start).
    pub signin_browser: Option<&'static str>,
}

impl App {
    /// Whether YouTube tracks play: the beta is on. Playback itself needs no Google account.
    pub(crate) fn youtube_playable(&self) -> bool {
        true
    }

    /// Whether there is an account to show: signed in with Google (or, for whoever set it up
    /// before, connected through their own developer client).
    pub(crate) fn youtube_enabled(&self) -> bool {
        self.youtube_playable()
            && (self.ui.youtube.account.is_some() || self.ui.youtube.status.connected)
    }

    /// Asks YouTube Music for the signed-in account's home and library, unless that is already
    /// on its way.
    fn youtube_load_home(&mut self) -> Task<Message> {
        if self.ui.youtube.account.is_none() || self.ui.youtube.home_loading {
            return Task::none();
        }
        self.ui.youtube.home_loading = true;
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .yt_home()
                    .await
                    .map(Box::new)
                    .map_err(|e| e.to_string())
            },
            |r| Message::Youtube(Msg::Home(r)),
        )
    }

    pub(crate) fn youtube_boot(&mut self) -> Task<Message> {
        self.ui.youtube.account = self.services.yt_account();
        self.ui.youtube.signin_browser = mosaic_services::youtube::browser::find()
            .first()
            .map(|b| b.name);
        let on = self.youtube_playable();
        let availability = self.player_input(PlayerInput::YoutubeAvailable(on));
        if !on {
            return availability;
        }
        let config = self.services.config();
        self.ui.youtube.client_id = config.youtube_client_id.unwrap_or_default();
        self.ui.youtube.client_secret.clear();
        let services = self.services.clone();
        // Loaded up front, so the tab is already filled when it's opened.
        let home = self.youtube_load_home();
        Task::batch([
            availability,
            home,
            Task::perform(async move { services.yt_status().await }, |s| {
                Message::Youtube(Msg::Status(s))
            }),
        ])
    }

    pub(crate) fn youtube_ensure_loaded(&mut self) -> Task<Message> {
        if self.ui.youtube.account.is_some() {
            return match &self.ui.youtube.home {
                Some(Ok(_)) => Task::none(),
                _ => self.youtube_load_home(),
            };
        }
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
            Msg::GoogleSignIn => {
                self.ui.youtube.signing_in = true;
                let services = self.services.clone();
                Task::perform(
                    async move { services.yt_sign_in().await.map_err(|e| e.to_string()) },
                    |r| Message::Youtube(Msg::GoogleSignedIn(r)),
                )
            }
            Msg::GoogleSignedIn(result) => {
                self.ui.youtube.signing_in = false;
                match result {
                    Ok(Some(account)) => {
                        self.toast(format!("Signed in as {}", account.name));
                        self.ui.youtube.account = Some(account);
                        self.ui.youtube.home = None;
                        let home = self.youtube_load_home();
                        // Straight to the library, which fills in as it arrives.
                        let view = crate::widgets::rail::View::YouTube;
                        if self.view_kind() == view {
                            return Task::batch([
                                home,
                                self.on_platform_tab(view, super::platform::Tab::Home),
                            ]);
                        }
                        self.ui.platform_tabs.remove(&view);
                        return home;
                    }
                    Ok(None) => {}
                    Err(e) => self.toast(e),
                }
                Task::none()
            }
            Msg::GoogleSignOut => {
                self.services.yt_sign_out();
                self.ui.youtube.account = None;
                self.ui.youtube.home = None;
                Task::none()
            }
            Msg::Home(result) => {
                self.ui.youtube.home_loading = false;
                // Signed out while it was on its way: drop it.
                if self.ui.youtube.account.is_some() {
                    self.ui.youtube.home = Some(result.map(|h| *h));
                }
                Task::none()
            }
            Msg::ImportLibrary => {
                if self.ui.youtube.importing {
                    return Task::none();
                }
                self.ui.youtube.importing = true;
                self.toast("Importing your YouTube Music library…");
                let services = self.services.clone();
                Task::perform(
                    async move {
                        services
                            .yt_import_everything()
                            .await
                            .map(Box::new)
                            .map_err(|e| e.to_string())
                    },
                    |r| Message::Youtube(Msg::Imported(r)),
                )
            }
            Msg::Imported(result) => {
                self.ui.youtube.importing = false;
                match result {
                    Ok(lib) => {
                        let (liked, lists) = self.apply_youtube_import(*lib);
                        self.toast(format!(
                            "Imported {liked} liked songs and {lists} playlists from YouTube Music"
                        ));
                        self.touch();
                    }
                    Err(e) => self.toast(e),
                }
                Task::none()
            }
            Msg::Status(status) => {
                let was = self.ui.youtube.status.connected;
                self.ui.youtube.status = status;
                let _ = was;
                if self.view_kind() == crate::widgets::rail::View::YouTube && self.youtube_enabled()
                {
                    return self.youtube_ensure_loaded();
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
                Ok(Some(id)) => {
                    let services = self.services.clone();
                    Task::perform(
                        async move {
                            let stream = services
                                .yt_video_stream(&id)
                                .await
                                .map_err(|e| e.to_string());
                            (track, id, stream)
                        },
                        |(track, id, stream)| Message::Youtube(Msg::Stream(track, id, stream)),
                    )
                }
                Ok(None) => {
                    self.toast("This song has no video on YouTube");
                    Task::none()
                }
                Err(e) => {
                    self.toast(e);
                    Task::none()
                }
            },
            Msg::Stream(track, id, result) => match result {
                Ok(stream) => self.show_video(*track, id, stream),
                Err(e) => {
                    self.toast(format!("{e}. Opening it in your browser instead"));
                    self.open_video(&track, &id)
                }
            },
            Msg::CloseVideo => {
                self.ui.youtube.video = None;
                Task::none()
            }
            Msg::VideoInBrowser => {
                let Some(v) = self.ui.youtube.video.take() else {
                    return Task::none();
                };
                self.open_video(&v.track, &v.id)
            }
            Msg::EnqueueAll => {
                let list = self.visible_youtube_tracks();
                self.toast(format!("Added {} to the queue", songs(list.len() as u64)));
                self.player_input(PlayerInput::EnqueueMany(list))
            }
        }
    }

    /// Shows a video inside the window, with the music playing along. A track that isn't the one
    /// playing starts first; for the playing one the picture starts where the music is.
    fn show_video(&mut self, track: Track, id: String, stream: VideoStream) -> Task<Message> {
        let playing_it = self.player.track.as_ref().is_some_and(|t| t.id == track.id);
        let clock = if playing_it {
            self.display_position()
        } else {
            0.0
        };
        let decoder =
            match Decoder::start(&stream.url, stream.width, stream.height, stream.fps, clock) {
                Ok(d) => d,
                Err(e) => {
                    self.toast(format!("{e}. Opening it in your browser instead"));
                    return self.open_video(&track, &id);
                }
            };
        let start = if playing_it {
            Task::none()
        } else {
            self.player_input(PlayerInput::Play(track.clone(), None))
        };
        self.ui.youtube.video = Some(Video {
            track,
            id,
            stream,
            decoder: Some(decoder),
            frame: None,
            clock,
        });
        start
    }

    /// Whether the video view needs frames drawn: while the music moves, until the first
    /// picture arrives, and after a seek while paused.
    pub(crate) fn video_active(&self) -> bool {
        self.ui.youtube.video.as_ref().is_some_and(|v| {
            v.frame.is_none()
                || self.player.playing
                || self.player.loading
                || (self.display_position() - v.clock).abs() > 0.05
        })
    }

    /// Matches the picture to the audio clock. Run on every frame while the video is open.
    pub(crate) fn video_tick(&mut self) {
        let Some(v) = &self.ui.youtube.video else {
            return;
        };
        if self.player.track.as_ref().map(|t| &t.id) != Some(&v.track.id) {
            // The queue moved on; the picture belonged to the song that finished.
            self.ui.youtube.video = None;
            return;
        }
        let clock = self.display_position();
        let Some(v) = self.ui.youtube.video.as_mut() else {
            return;
        };
        v.clock = clock;
        let Some(decoder) = v.decoder.as_mut() else {
            return;
        };
        let poll = decoder.poll(clock);
        if let Some(frame) = poll.frame {
            v.frame = Some(frame);
        }
        if poll.restart {
            v.decoder = Decoder::start(
                &v.stream.url,
                v.stream.width,
                v.stream.height,
                v.stream.fps,
                clock,
            )
            .ok();
            if v.decoder.is_none() {
                self.ui.youtube.video = None;
                self.toast("The video stopped");
            }
        }
        if let Some(why) = poll.failed {
            self.ui.youtube.video = None;
            self.toast(why);
        }
    }

    /// The mini video player: a small card above the player bar, at the right. Only the card
    /// takes clicks; everything around it stays usable.
    pub(crate) fn video_layer(&self, p: Palette) -> Option<Element<'_, Message>> {
        let v = self.ui.youtube.video.as_ref()?;
        let width = (self.size.width - 32.0).clamp(200.0, 400.0);
        let height = width * v.stream.height as f32 / v.stream.width.max(1) as f32;
        let picture: Element<'_, Message> = match &v.frame {
            Some(frame) => image(frame.clone())
                .content_fit(iced::ContentFit::Contain)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            None => spinner(
                32.0,
                self.now
                    .saturating_duration_since(self.epoch)
                    .as_secs_f32()
                    .fract(),
                crate::theme::alpha(iced::Color::WHITE, 0.7),
            ),
        };
        let screen = container(picture)
            .center(Length::Fill)
            .width(Length::Fill)
            .height(height)
            .clip(true)
            .style(styles::fill(iced::Color::BLACK, 12.0));
        let footer = row![
            column![
                text(v.track.title.clone())
                    .size(13)
                    .color(p.on_surface)
                    .wrapping(text::Wrapping::None),
                text(v.track.artist.clone())
                    .size(12)
                    .color(p.on_surface_variant)
                    .wrapping(text::Wrapping::None),
            ]
            .width(Length::Fill)
            .clip(true),
            super::common::icon_button(
                Icon::OpenInNew,
                "Open on YouTube",
                p,
                Message::Youtube(Msg::VideoInBrowser)
            ),
            super::common::icon_button(
                Icon::Close,
                "Close video",
                p,
                Message::Youtube(Msg::CloseVideo)
            ),
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let card = container(column![screen, footer].spacing(4))
            .padding(8)
            .width(width + 16.0)
            .style(styles::popup(p, 16.0));
        // Clear the player bar and its margins.
        let bar = self.settings().bar_height as f32;
        Some(
            container(opaque(card))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::End)
                .padding(iced::Padding {
                    top: 0.0,
                    right: 28.0,
                    bottom: bar + 44.0,
                    left: 0.0,
                })
                .into(),
        )
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
            self.youtube_liked()
        }
    }

    /// Liked songs: the signed-in account's, else the developer-client library's.
    pub(crate) fn youtube_liked(&self) -> Vec<Track> {
        match &self.ui.youtube.home {
            Some(Ok(h)) => h.liked.clone(),
            _ => self.ui.youtube.library.liked.clone(),
        }
    }

    /// Recently played, from YouTube Music's history.
    pub(crate) fn youtube_recent(&self) -> Vec<Track> {
        match &self.ui.youtube.home {
            Some(Ok(h)) => h.recent.clone(),
            _ => Vec::new(),
        }
    }

    /// The songs of home shelf `i`.
    pub(crate) fn youtube_shelf(&self, i: usize) -> Vec<Track> {
        match &self.ui.youtube.home {
            Some(Ok(h)) => h
                .shelves
                .get(i)
                .map(|s| s.tracks.clone())
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The YouTube tab for a Google sign-in.
    fn youtube_home_view<'a>(&'a self, p: Palette) -> Element<'a, Message> {
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
        let name = s.account.as_ref().map(|a| a.name.clone());
        let mut body = column![self.page_header("YouTube Music".into(), name, Vec::new(), p)];
        match &s.home {
            None => {
                for title in ["Recently played", "Your playlists"] {
                    body = body.push(h3(title)).push(
                        row((0..6).map(|_| crate::widgets::skeleton::block(148, 148, 16.0, p)))
                            .spacing(20),
                    );
                }
            }
            Some(Err(e)) => {
                body = body.push(Space::new().height(16)).push(
                    container(text(e.clone()).size(14))
                        .padding([12, 16])
                        .width(Length::Fill)
                        .style(styles::error_box(p)),
                );
            }
            Some(Ok(h)) => {
                if !h.recent.is_empty() {
                    body = body.push(h3("Recently played")).push(self.tile_shelf(
                        &h.recent,
                        ListRef::YoutubeRecent,
                        p,
                    ));
                }
                if !h.playlists.is_empty() {
                    body = body
                        .push(h3("Your playlists"))
                        .push(self.catalog_cards(&h.playlists, p));
                }
                if !h.liked.is_empty() {
                    body = body
                        .push(h3("Liked songs"))
                        .push(self.youtube_actions(p))
                        .push(Space::new().height(16))
                        .push(self.tile_shelf(&h.liked, ListRef::YoutubeLiked, p));
                }
                if !h.artists.is_empty() {
                    body = body
                        .push(h3("Artists you follow"))
                        .push(self.catalog_cards(&h.artists, p));
                }
                for (i, shelf) in h.shelves.iter().enumerate() {
                    body = body.push(h3(&shelf.title));
                    if !shelf.tracks.is_empty() {
                        body =
                            body.push(self.tile_shelf(&shelf.tracks, ListRef::YoutubeShelf(i), p));
                    }
                    if !shelf.cards.is_empty() {
                        body = body.push(self.catalog_cards(&shelf.cards, p));
                    }
                }
                if h.recent.is_empty()
                    && h.playlists.is_empty()
                    && h.liked.is_empty()
                    && h.shelves.is_empty()
                {
                    body = body.push(h3(
                        "Nothing here yet. What you play and like on YouTube Music shows up here.",
                    ));
                }
            }
        }
        page(body, p)
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

    /// Adds what's new: liked songs to Likes, each playlist as "<name> (YouTube Music)".
    fn apply_youtube_import(&mut self, lib: YtLibrary) -> (usize, usize) {
        let mut liked = 0;
        for t in lib.liked.iter().rev() {
            if !self.store.is_liked(&t.id) {
                self.store.likes.insert(0, t.strip());
                liked += 1;
            }
        }
        let mut lists = 0;
        for (name, tracks) in &lib.playlists {
            if tracks.is_empty() {
                continue;
            }
            let name = format!("{name} (YouTube Music)");
            match self.store.playlists.iter().position(|p| p.name == name) {
                Some(i) => {
                    let have: std::collections::HashSet<_> = self.store.playlists[i]
                        .tracks
                        .iter()
                        .map(|t| t.id.clone())
                        .collect();
                    let new: Vec<Track> = tracks
                        .iter()
                        .filter(|t| !have.contains(&t.id))
                        .map(Track::strip)
                        .collect();
                    self.store.playlists[i].tracks.extend(new);
                }
                None => {
                    self.store.create_playlist(&name, tracks);
                    lists += 1;
                }
            }
        }
        (liked, lists)
    }

    /// The Google sign-in part of the YouTube Music account tab.
    pub(crate) fn youtube_account(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.youtube;
        let (title, hint, actions): (String, String, Vec<Element<'_, Message>>) = match &s.account {
            Some(a) => (
                format!("Signed in as {}", a.name),
                if a.email_or_handle.is_empty() {
                    "Bring your liked songs and playlists over to Mosaic.".to_string()
                } else {
                    format!(
                        "{} · bring your liked songs and playlists over to Mosaic.",
                        a.email_or_handle
                    )
                },
                vec![
                    primary_button(
                        if s.importing {
                            "Importing…"
                        } else {
                            "Import library"
                        },
                        Icon::CloudDownload,
                        p,
                        Message::Youtube(Msg::ImportLibrary),
                    ),
                    tonal_button(
                        "Sign out",
                        Icon::Close,
                        p,
                        Message::Youtube(Msg::GoogleSignOut),
                    ),
                ],
            ),
            None => (
                "Sign in with Google".to_string(),
                match s.signin_browser {
                    Some(name) if s.signing_in => format!(
                        "Sign in with Google in the {name} window. Once YouTube Music shows up, \
                         close that window and Mosaic finishes signing in."
                    ),
                    Some(name) => format!(
                        "Opens {name} on a separate profile to sign in, then shows your YouTube \
                         Music library here. Playing needs no account."
                    ),
                    None if cfg!(target_os = "linux") => "Signing in needs Firefox or a \
                        Chromium-based browser (Chrome, Chromium, Brave, Edge, Zen…). Playing \
                        needs no account."
                        .to_string(),
                    None => {
                        "See your YouTube Music library here. Playing needs no account.".to_string()
                    }
                },
                vec![primary_button(
                    if s.signing_in {
                        "Waiting for the browser…"
                    } else {
                        "Sign in"
                    },
                    Icon::Person,
                    p,
                    Message::Youtube(Msg::GoogleSignIn),
                )],
            ),
        };
        container(
            row![
                column![
                    text(title).size(16),
                    text(hint).size(13).color(p.on_surface_variant)
                ]
                .spacing(2)
                .width(Length::Fill),
                row(actions).spacing(8),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        )
        .padding([14, 0])
        .into()
    }

    pub(crate) fn youtube_view(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.youtube;
        if s.account.is_some() && s.open.is_none() {
            return self.youtube_home_view(p);
        }
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
                0,
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
