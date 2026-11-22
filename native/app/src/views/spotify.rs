//! Port of `SpotifyProvider.js` and `SpotifyView.js`: library home, artist and album pages.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::Track;
use mosaic_services::spotify::auth::Status;
use mosaic_services::spotify::web_api::{
    AlbumCard, AlbumPage, ArtistCard, ArtistPage, Library, SearchResults,
};

use super::common::{ghost_button, page, tonal_button, virtual_rows, visible_range};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::Icon;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Artist(String),
    Album(String),
}

impl Route {
    fn key(&self) -> String {
        match self {
            Route::Home => "home".into(),
            Route::Artist(id) => format!("artist:{id}"),
            Route::Album(id) => format!("album:{id}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Page {
    Home(Library),
    Artist(ArtistPage),
    Album(AlbumPage),
}

#[derive(Clone, Debug)]
pub enum Msg {
    Status(Status),
    Go(Route),
    Back,
    Loaded(String, Box<Result<Page, String>>),
    Saved(Vec<Track>),
    Search(u64, Box<Result<SearchResults, String>>),
    Connect,
    Connected(Result<Status, String>),
    Disconnect,
    ClientId(String),
    PlayAll,
    EnqueueAll,
}

pub struct State {
    pub status: Status,
    pub stack: Vec<Route>,
    pub page: Option<(String, Result<Page, String>)>,
    pub saved: Vec<Track>,
    pub search: SearchResults,
    pub connecting: bool,
    pub error: String,
    pub client_id: String,
}

impl Default for State {
    fn default() -> Self {
        State {
            status: Status::default(),
            stack: vec![Route::Home],
            page: None,
            saved: Vec::new(),
            search: SearchResults::default(),
            connecting: false,
            error: String::new(),
            client_id: String::new(),
        }
    }
}

pub fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1e6)
    } else if n >= 1000 {
        format!("{}K", (n as f64 / 1e3).round())
    } else {
        n.to_string()
    }
}

impl App {
    /// Spotify browsing is available when the beta is on and the account is connected.
    pub(crate) fn spotify_enabled(&self) -> bool {
        self.services.config().beta == Some(true) && self.ui.spotify.status.connected
    }

    /// Whether Spotify tracks can be played (Premium + librespot compiled in).
    pub(crate) fn spotify_playable(&self) -> bool {
        cfg!(feature = "spotify") && self.spotify_enabled() && self.ui.spotify.status.premium
    }

    pub(crate) fn spotify_boot(&mut self) -> Task<Message> {
        if self.services.config().beta != Some(true) {
            return Task::none();
        }
        self.ui.spotify.client_id = self.services.config().spotify_client_id.unwrap_or_default();
        let services = self.services.clone();
        Task::perform(async move { services.spotify_status().await }, |s| {
            Message::Spotify(Msg::Status(s))
        })
    }

    pub(crate) fn spotify_route(&self) -> Route {
        self.ui.spotify.stack.last().cloned().unwrap_or(Route::Home)
    }

    fn load_route(&mut self) -> Task<Message> {
        let route = self.spotify_route();
        let key = route.key();
        self.ui.spotify.page = None;
        let services = self.services.clone();
        Task::perform(
            async move {
                match route {
                    Route::Home => Ok(Page::Home(services.spotify_library().await)),
                    Route::Artist(id) => services
                        .spotify_artist(&id)
                        .await
                        .map(Page::Artist)
                        .map_err(|e| e.to_string()),
                    Route::Album(id) => services
                        .spotify_album(&id)
                        .await
                        .map(Page::Album)
                        .map_err(|e| e.to_string()),
                }
            },
            move |r| Message::Spotify(Msg::Loaded(key.clone(), Box::new(r))),
        )
    }

    pub(crate) fn spotify_go(&mut self, route: Route) -> Task<Message> {
        self.ui.view = Some(crate::widgets::rail::View::Spotify);
        self.ui.now_playing = false;
        self.ui.scroll.0 = 0.0;
        self.retarget_rail();
        if route == Route::Home {
            self.ui.spotify.stack = vec![Route::Home];
        } else {
            self.ui.spotify.stack.push(route);
        }
        self.load_route()
    }

    pub(crate) fn search_spotify(&mut self, q: String, seq: u64) -> Task<Message> {
        if !self.spotify_enabled() {
            return Task::none();
        }
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .spotify_search(&q, 20)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |r| Message::Spotify(Msg::Search(seq, Box::new(r))),
        )
    }

    pub(crate) fn on_spotify(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Status(status) => {
                let was = self.ui.spotify.status.connected;
                self.ui.spotify.status = status;
                let playable = self.spotify_playable();
                let mut tasks =
                    vec![self.player_input(crate::player::PlayerInput::SpotifyAvailable(playable))];
                if self.spotify_enabled() && !was {
                    let services = self.services.clone();
                    tasks.push(Task::perform(
                        async move { services.spotify_library().await.saved },
                        |s| Message::Spotify(Msg::Saved(s)),
                    ));
                }
                if !self.spotify_enabled()
                    && self.view_kind() == crate::widgets::rail::View::Spotify
                {
                    self.ui.view = Some(crate::widgets::rail::View::Discover);
                    self.retarget_rail();
                }
                Task::batch(tasks)
            }
            Msg::Saved(list) => {
                self.ui.spotify.saved = list;
                Task::none()
            }
            Msg::Go(route) => self.spotify_go(route),
            Msg::Back => {
                if self.ui.spotify.stack.len() > 1 {
                    self.ui.spotify.stack.pop();
                }
                self.load_route()
            }
            Msg::Loaded(key, result) => {
                if key == self.spotify_route().key() {
                    self.ui.spotify.page = Some((key, *result));
                }
                Task::none()
            }
            Msg::Search(seq, result) => {
                if seq == self.ui.discover.seq {
                    self.ui.spotify.search =
                        result.map_or_else(|_| SearchResults::default(), |r| r);
                }
                Task::none()
            }
            Msg::ClientId(id) => {
                self.services.spotify_set_client_id(&id);
                self.ui.spotify.client_id = id;
                Task::none()
            }
            Msg::Connect => {
                self.ui.spotify.connecting = true;
                self.ui.spotify.error.clear();
                let services = self.services.clone();
                Task::perform(
                    async move {
                        services
                            .spotify_connect(|url| {
                                mosaic_platform::open::open_https(url);
                            })
                            .await
                            .map_err(|e| e.to_string())
                    },
                    |r| Message::Spotify(Msg::Connected(r)),
                )
            }
            Msg::Connected(result) => {
                self.ui.spotify.connecting = false;
                match result {
                    Ok(status) => self.on_spotify(Msg::Status(status)),
                    Err(e) => {
                        self.ui.spotify.error = e;
                        Task::none()
                    }
                }
            }
            Msg::Disconnect => {
                self.services.spotify_disconnect();
                self.ui.spotify.saved.clear();
                self.on_spotify(Msg::Status(Status {
                    client_id: self.ui.spotify.client_id.clone(),
                    ..Status::default()
                }))
            }
            Msg::PlayAll => {
                let list = self.spotify_page_tracks();
                match list.first().cloned() {
                    Some(t) => self.player_input(crate::player::PlayerInput::Play(t, Some(list))),
                    None => Task::none(),
                }
            }
            Msg::EnqueueAll => {
                let list = self.spotify_page_tracks();
                self.player_input(crate::player::PlayerInput::EnqueueMany(list))
            }
        }
    }

    /// Tracks of the page on screen (liked songs, artist top tracks, album tracks).
    pub(crate) fn spotify_page_tracks(&self) -> Vec<Track> {
        match &self.ui.spotify.page {
            Some((_, Ok(Page::Home(l)))) => l.saved.clone(),
            Some((_, Ok(Page::Artist(a)))) => a.top.clone(),
            Some((_, Ok(Page::Album(a)))) => a.tracks.clone(),
            _ => Vec::new(),
        }
    }

    fn card<'a>(
        &'a self,
        image: &str,
        title: String,
        sub: String,
        round: bool,
        msg: Message,
        p: Palette,
    ) -> Element<'a, Message> {
        let radius = if round { 74.0 } else { 16.0 };
        column![
            button(self.art(image, 148.0, radius))
                .padding(0)
                .style(styles::bare(p))
                .on_press(msg),
            container(text(title).size(14))
                .height(52)
                .padding(iced::Padding {
                    top: 12.0,
                    ..iced::Padding::ZERO
                })
                .clip(true),
            container(
                text(sub)
                    .size(12)
                    .color(p.on_surface_variant)
                    .wrapping(text::Wrapping::None)
            )
            .clip(true),
        ]
        .spacing(2)
        .width(148)
        .into()
    }

    pub(crate) fn album_cards<'a>(
        &'a self,
        albums: &'a [AlbumCard],
        p: Palette,
    ) -> Element<'a, Message> {
        scrollable(
            row(albums.iter().map(|a| {
                let sub = [
                    a.year.clone(),
                    if a.kind == "single" {
                        "Single".into()
                    } else {
                        a.artist.clone()
                    },
                ]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
                self.card(
                    &a.image,
                    a.name.clone(),
                    sub,
                    false,
                    Message::Spotify(Msg::Go(Route::Album(a.id.clone()))),
                    p,
                )
            }))
            .spacing(20)
            .padding(iced::Padding {
                bottom: 14.0,
                ..iced::Padding::ZERO
            }),
        )
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .style(styles::scrollbar(p))
        .into()
    }

    pub(crate) fn artist_cards<'a>(
        &'a self,
        artists: &'a [ArtistCard],
        p: Palette,
    ) -> Element<'a, Message> {
        scrollable(
            row(artists.iter().map(|a| {
                self.card(
                    &a.image,
                    a.name.clone(),
                    "Artist".into(),
                    true,
                    Message::Spotify(Msg::Go(Route::Artist(a.id.clone()))),
                    p,
                )
            }))
            .spacing(20)
            .padding(iced::Padding {
                bottom: 14.0,
                ..iced::Padding::ZERO
            }),
        )
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .style(styles::scrollbar(p))
        .into()
    }

    pub(crate) fn tile_shelf<'a>(
        &'a self,
        list: &'a [Track],
        which: ListRef,
        p: Palette,
    ) -> Element<'a, Message> {
        scrollable(
            row(list.iter().enumerate().map(|(i, t)| {
                self.tile(
                    t,
                    TrackRef {
                        list: which.clone(),
                        index: i,
                    },
                    148.0,
                    p,
                )
            }))
            .spacing(20)
            .padding(iced::Padding {
                bottom: 14.0,
                ..iced::Padding::ZERO
            }),
        )
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(6).scroller_width(6),
        ))
        .style(styles::scrollbar(p))
        .into()
    }

    fn sp_head<'a>(
        &'a self,
        image: &str,
        round: bool,
        eyebrow: String,
        title: String,
        meta: String,
        p: Palette,
    ) -> Element<'a, Message> {
        let actions = row![
            super::common::primary_button(
                "Play",
                Icon::PlayArrow,
                p,
                Message::Spotify(Msg::PlayAll)
            ),
            tonal_button(
                "Add to queue",
                Icon::QueueMusic,
                p,
                Message::Spotify(Msg::EnqueueAll)
            ),
        ]
        .spacing(8)
        .padding(iced::Padding {
            top: 16.0,
            ..iced::Padding::ZERO
        });
        row![
            self.art(image, 168.0, if round { 84.0 } else { 20.0 }),
            column![
                text(eyebrow).size(14).color(p.on_surface_variant),
                text(title).size(32),
                text(meta).size(14).color(p.on_surface_variant),
                actions,
            ]
            .spacing(4)
            .width(Length::Fill),
        ]
        .spacing(28)
        .align_y(Alignment::End)
        .padding([8, 0])
        .into()
    }

    pub(crate) fn spotify_view(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.spotify;
        let h3 = |t: &str| {
            container(text(t.to_string()).size(16).color(p.on_surface_variant)).padding(
                iced::Padding {
                    top: 32.0,
                    bottom: 16.0,
                    ..iced::Padding::ZERO
                },
            )
        };
        let mut body = column![];
        if s.stack.len() > 1 {
            body = body.push(ghost_button(
                "Back",
                Icon::ArrowBack,
                p,
                Message::Spotify(Msg::Back),
            ));
        } else {
            body = body.push(self.page_header("Spotify (beta)".into(), None, Vec::new(), p));
        }
        match &s.page {
            None => {
                body = body.push(
                    row((0..6).map(|_| {
                        container(Space::new().width(148).height(148))
                            .style(styles::fill(p.surface_container_high, 16.0))
                            .into()
                    }))
                    .spacing(20),
                );
            }
            Some((_, Err(e))) => {
                body = body.push(
                    container(text(e.clone()).size(14))
                        .padding([12, 16])
                        .width(Length::Fill)
                        .style(styles::error_box(p)),
                );
            }
            Some((_, Ok(Page::Home(lib)))) => {
                if !lib.saved.is_empty() {
                    body = body
                        .push(
                            row![
                                super::common::primary_button(
                                    "Play",
                                    Icon::PlayArrow,
                                    p,
                                    Message::Spotify(Msg::PlayAll)
                                ),
                                tonal_button(
                                    "Add to queue",
                                    Icon::QueueMusic,
                                    p,
                                    Message::Spotify(Msg::EnqueueAll)
                                ),
                            ]
                            .spacing(8),
                        )
                        .push(h3("Liked songs"))
                        .push(self.tile_shelf(&lib.saved, ListRef::SpotifyPage, p));
                }
                if !lib.albums.is_empty() {
                    body = body
                        .push(h3("Saved albums"))
                        .push(self.album_cards(&lib.albums, p));
                }
                if !lib.artists.is_empty() {
                    body = body
                        .push(h3("Followed artists"))
                        .push(self.artist_cards(&lib.artists, p));
                }
                if lib.saved.is_empty() && lib.albums.is_empty() && lib.artists.is_empty() {
                    body = body.push(
                        text("Nothing saved on Spotify yet. Search for an artist on Discover.")
                            .color(p.on_surface_variant),
                    );
                }
            }
            Some((_, Ok(Page::Artist(a)))) => {
                let meta = [
                    a.artist
                        .followers
                        .map(|n| format!("{} followers", compact(n))),
                    Some(
                        a.artist
                            .genres
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", "),
                    )
                    .filter(|g| !g.is_empty()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
                body = body.push(self.sp_head(
                    &a.artist.image,
                    true,
                    "Artist".into(),
                    a.artist.name.clone(),
                    meta,
                    p,
                ));
                if !a.top.is_empty() {
                    body = body.push(h3("Popular"));
                    let shown = a.top.len().min(10);
                    let range = visible_range(shown, self.ui.scroll.0, self.ui.scroll.1, 420.0);
                    body = body.push(virtual_rows(shown, range, |i| {
                        self.track_row(
                            &a.top[i],
                            i,
                            TrackRef {
                                list: ListRef::SpotifyPage,
                                index: i,
                            },
                            true,
                            p,
                        )
                    }));
                }
                if !a.albums.is_empty() {
                    body = body
                        .push(h3("Albums & singles"))
                        .push(self.album_cards(&a.albums, p));
                }
            }
            Some((_, Ok(Page::Album(a)))) => {
                let meta = format!(
                    "{} · {} · {} tracks",
                    a.album.artist,
                    a.album.year,
                    a.album.total.unwrap_or(a.tracks.len() as u64)
                );
                body = body.push(self.sp_head(
                    &a.album.image,
                    false,
                    if a.album.kind == "single" {
                        "Single".into()
                    } else {
                        "Album".into()
                    },
                    a.album.name.clone(),
                    meta,
                    p,
                ));
                let range =
                    visible_range(a.tracks.len(), self.ui.scroll.0, self.ui.scroll.1, 360.0);
                body = body.push(virtual_rows(a.tracks.len(), range, |i| {
                    self.track_row(
                        &a.tracks[i],
                        i,
                        TrackRef {
                            list: ListRef::SpotifyPage,
                            index: i,
                        },
                        true,
                        p,
                    )
                }));
            }
        }
        page(body, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follower_counts_are_compacted_like_the_js() {
        assert_eq!(compact(0), "0");
        assert_eq!(compact(950), "950");
        assert_eq!(compact(12_400), "12K");
        assert_eq!(compact(1_500_000), "1.5M");
    }

    #[test]
    fn route_keys_are_unique_per_page() {
        assert_ne!(
            Route::Artist("a".into()).key(),
            Route::Album("a".into()).key()
        );
        assert_eq!(Route::Home.key(), "home");
    }
}
