//! The Spotify section: home, Liked Songs, playlist, artist and album pages, plus the calls that
//! keep Spotify's library in step (likes, saves, follows, adding to playlists).

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::{Source, Store, Track};
use mosaic_services::spotify::auth::Status;
use mosaic_services::spotify::library::{Home, PlaylistCard, PlaylistPage, Saveable};
use mosaic_services::spotify::web_api::{
    AlbumCard, AlbumPage, ArtistCard, ArtistPage, Everything, SearchResults,
};
use rand::seq::SliceRandom;

use super::common::{ghost_button, page, tonal_button, virtual_rows, visible_range};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, outlined, Icon};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Home,
    Liked,
    Playlist(String),
    Artist(String),
    Album(String),
}

impl Route {
    fn key(&self) -> String {
        match self {
            Route::Home => "home".into(),
            Route::Liked => "liked".into(),
            Route::Playlist(id) => format!("playlist:{id}"),
            Route::Artist(id) => format!("artist:{id}"),
            Route::Album(id) => format!("album:{id}"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Page {
    Home(Box<Home>),
    Liked(Vec<Track>),
    Playlist(PlaylistPage),
    /// The page and whether the user follows the artist.
    Artist(ArtistPage, bool),
    /// The page and whether the album is in the library.
    Album(AlbumPage, bool),
}

/// Home shelves of tracks, for resolving clicks on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shelf {
    Recent,
    Top,
    Liked,
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
    ImportAll,
    ImportedAll(Box<Everything>),
    ClientId(String),
    PlayAll,
    ShuffleAll,
    EnqueueAll,
    /// Save / follow (or undo) whatever the open page shows.
    TogglePage,
    /// Result of a library change; on failure the optimistic state is rolled back.
    Changed(Saveable, bool, Result<(), String>),
    Editable(Vec<PlaylistCard>),
    AddedTo(String, Result<(), String>),
    Created(Result<PlaylistCard, String>),
    /// Refresh Soloist's up-next list.
    PollQueue,
    Queue(Vec<String>, Vec<Track>),
    /// Soloist moved on from `from` to `to` by itself.
    Moved {
        from: String,
        to: String,
    },
    /// The track Soloist moved on to, looked up (`None` if that failed).
    Adopt(String, Option<Box<Track>>),
}

pub struct State {
    pub status: Status,
    pub stack: Vec<Route>,
    pub page: Option<(String, Result<Page, String>)>,
    pub saved: Vec<Track>,
    pub search: SearchResults,
    pub connecting: bool,
    pub importing: bool,
    pub error: String,
    pub client_id: String,
    /// Spotify Soloist settings being edited (path to the binary and the user's API key).
    pub soloist_path: String,
    pub soloist_key: String,
    /// The user's own and collaborative playlists, for "Add to playlist".
    pub editable: Vec<PlaylistCard>,
    /// Soloist's up-next URIs and the tracks they resolved to.
    pub queue: (Vec<String>, Vec<Track>),
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
            importing: false,
            error: String::new(),
            client_id: String::new(),
            soloist_path: String::new(),
            soloist_key: String::new(),
            editable: Vec::new(),
            queue: (Vec::new(), Vec::new()),
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

/// Total running time the way Spotify writes it: "1 hr 4 min", "38 min 12 sec".
pub fn total_length(tracks: &[Track]) -> String {
    let secs = tracks.iter().map(|t| t.duration).sum::<u64>() / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h} hr {m} min")
    } else {
        format!("{m} min {s} sec")
    }
}

fn songs(n: u64) -> String {
    if n == 1 {
        "1 song".into()
    } else {
        format!("{n} songs")
    }
}

fn err(e: anyhow::Error) -> String {
    e.to_string()
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

    /// Whether Mosaic may change the Spotify library (connected with the edit scopes).
    pub(crate) fn spotify_can_edit(&self) -> bool {
        self.spotify_enabled() && self.ui.spotify.status.can_edit
    }

    pub(crate) fn spotify_boot(&mut self) -> Task<Message> {
        if self.services.config().beta != Some(true) {
            return Task::none();
        }
        self.ui.spotify.client_id = self.services.config().spotify_client_id.unwrap_or_default();
        #[cfg(feature = "spotify")]
        {
            let soloist = self.services.soloist_settings();
            self.ui.spotify.soloist_path = soloist.path;
            self.ui.spotify.soloist_key = soloist.api_key;
        }
        let services = self.services.clone();
        Task::perform(async move { services.spotify_status().await }, |s| {
            Message::Spotify(Msg::Status(s))
        })
    }

    /// Loads the open Spotify page if it isn't on screen yet (after arriving from the rail).
    pub(crate) fn spotify_ensure_loaded(&mut self) -> Task<Message> {
        let key = self.spotify_route().key();
        match &self.ui.spotify.page {
            Some((k, Ok(_))) if *k == key => Task::none(),
            _ if !self.spotify_enabled() => Task::none(),
            _ => self.load_route(),
        }
    }

    /// Whether the playing track goes through Spotify (and so Soloist's queue means something).
    pub(crate) fn playing_spotify(&self) -> bool {
        self.player
            .track
            .as_ref()
            .is_some_and(|t| t.source == Source::Spotify)
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
                    Route::Home => Ok(Page::Home(Box::new(services.spotify_home().await))),
                    Route::Liked => services.spotify_liked().await.map(Page::Liked).map_err(err),
                    Route::Playlist(id) => services
                        .spotify_playlist(&id)
                        .await
                        .map(Page::Playlist)
                        .map_err(err),
                    Route::Artist(id) => {
                        let follows = Saveable::Artist(id.clone());
                        let (page, saved) = tokio::join!(
                            services.spotify_artist(&id),
                            services.spotify_contains(&follows)
                        );
                        page.map(|a| Page::Artist(a, saved.unwrap_or(false)))
                            .map_err(err)
                    }
                    Route::Album(id) => {
                        let saved = Saveable::Album(id.clone());
                        let (page, saved) = tokio::join!(
                            services.spotify_album(&id),
                            services.spotify_contains(&saved)
                        );
                        page.map(|a| Page::Album(a, saved.unwrap_or(false)))
                            .map_err(err)
                    }
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
        self.page_transition();
        if route == Route::Home {
            self.ui.spotify.stack = vec![Route::Home];
        } else if self.spotify_route() != route {
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
            async move { services.spotify_search(&q, 20).await.map_err(err) },
            move |r| Message::Spotify(Msg::Search(seq, Box::new(r))),
        )
    }

    fn load_editable(&self) -> Task<Message> {
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .spotify_editable_playlists()
                    .await
                    .unwrap_or_default()
            },
            |l| Message::Spotify(Msg::Editable(l)),
        )
    }

    /// Mirrors a like in Mosaic onto the Spotify library for Spotify tracks.
    pub(crate) fn spotify_sync_like(&self, track: &Track, liked: bool) -> Task<Message> {
        if track.source != Source::Spotify || !self.spotify_can_edit() {
            return Task::none();
        }
        self.spotify_change(Saveable::Track(track.id.key().to_string()), liked)
    }

    fn spotify_change(&self, what: Saveable, on: bool) -> Task<Message> {
        let services = self.services.clone();
        Task::perform(
            async move {
                let r = services.spotify_set_saved(&what, on).await.map_err(err);
                (what, on, r)
            },
            |(what, on, r)| Message::Spotify(Msg::Changed(what, on, r)),
        )
    }

    /// Adds `track` to one of the user's Spotify playlists.
    pub(crate) fn spotify_add_to(&mut self, playlist: String, track: &Track) -> Task<Message> {
        let Some(uri) = track.uri.clone() else {
            return Task::none();
        };
        let services = self.services.clone();
        Task::perform(
            async move {
                let r = services
                    .spotify_add_to_playlist(&playlist, &[uri])
                    .await
                    .map_err(err);
                (playlist, r)
            },
            |(id, r)| Message::Spotify(Msg::AddedTo(id, r)),
        )
    }

    /// Creates a Spotify playlist holding `track`.
    pub(crate) fn spotify_new_playlist(&mut self, track: &Track) -> Task<Message> {
        let uris: Vec<String> = track.uri.clone().into_iter().collect();
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .spotify_create_playlist("New playlist", &uris)
                    .await
                    .map_err(err)
            },
            |r| Message::Spotify(Msg::Created(r)),
        )
    }

    /// The page's save / follow flag, set optimistically.
    fn set_page_saved(&mut self, what: &Saveable, on: bool) {
        match (&mut self.ui.spotify.page, what) {
            (Some((_, Ok(Page::Playlist(pl)))), Saveable::Playlist(id))
                if &pl.playlist.id == id =>
            {
                pl.saved = on;
            }
            (Some((_, Ok(Page::Artist(a, saved)))), Saveable::Artist(id)) if &a.artist.id == id => {
                *saved = on;
            }
            (Some((_, Ok(Page::Album(a, saved)))), Saveable::Album(id)) if &a.album.id == id => {
                *saved = on;
            }
            _ => {}
        }
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
                    tasks.push(self.load_editable());
                }
                if self.view_kind() == crate::widgets::rail::View::Spotify {
                    if self.spotify_enabled() {
                        tasks.push(self.spotify_ensure_loaded());
                    } else {
                        self.ui.view = Some(crate::widgets::rail::View::Discover);
                        self.retarget_rail();
                    }
                }
                Task::batch(tasks)
            }
            Msg::Saved(list) => {
                self.ui.spotify.saved = list;
                Task::none()
            }
            Msg::Editable(list) => {
                self.ui.spotify.editable = list;
                Task::none()
            }
            Msg::Go(route) => self.spotify_go(route),
            Msg::Back => {
                if self.ui.spotify.stack.len() > 1 {
                    self.ui.spotify.stack.pop();
                }
                self.page_transition();
                self.load_route()
            }
            Msg::Loaded(key, result) => {
                if key == self.spotify_route().key() {
                    if let Ok(Page::Home(h)) = result.as_ref() {
                        if !h.liked.is_empty() {
                            self.ui.spotify.saved = h.liked.clone();
                        }
                    }
                    self.ui.spotify.page = Some((key, *result));
                }
                Task::none()
            }
            Msg::Search(seq, result) => {
                if seq == self.ui.discover.seq {
                    self.ui.discover.sp_busy = false;
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
                            .map_err(err)
                    },
                    |r| Message::Spotify(Msg::Connected(r)),
                )
            }
            Msg::Connected(result) => {
                self.ui.spotify.connecting = false;
                match result {
                    Ok(status) => {
                        let reconnect = self.ui.spotify.status.connected;
                        self.ui.spotify.status.connected = false;
                        let task = self.on_spotify(Msg::Status(status));
                        if reconnect {
                            self.toast("Spotify reconnected");
                            return Task::batch([task, self.load_route()]);
                        }
                        Task::batch([task, self.on_spotify(Msg::ImportAll)])
                    }
                    Err(e) => {
                        self.toast(e.clone());
                        self.ui.spotify.error = e;
                        Task::none()
                    }
                }
            }
            Msg::ImportAll => {
                if self.ui.spotify.importing || !self.ui.spotify.status.connected {
                    return Task::none();
                }
                self.ui.spotify.importing = true;
                self.toast("Importing your Spotify library…");
                let services = self.services.clone();
                Task::perform(async move { services.spotify_everything().await }, |e| {
                    Message::Spotify(Msg::ImportedAll(Box::new(e)))
                })
            }
            Msg::ImportedAll(all) => {
                self.ui.spotify.importing = false;
                let (liked, lists) = self.apply_spotify_import(*all);
                self.touch();
                self.toast(format!(
                    "Imported {liked} liked songs and {lists} playlists from Spotify"
                ));
                Task::none()
            }
            Msg::Disconnect => {
                self.services.spotify_disconnect();
                self.ui.spotify.saved.clear();
                self.ui.spotify.editable.clear();
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
            Msg::ShuffleAll => {
                let mut list = self.spotify_page_tracks();
                list.shuffle(&mut rand::rng());
                match list.first().cloned() {
                    Some(t) => self.player_input(crate::player::PlayerInput::Play(t, Some(list))),
                    None => Task::none(),
                }
            }
            Msg::EnqueueAll => {
                let list = self.spotify_page_tracks();
                self.toast(format!("Added {} to the queue", songs(list.len() as u64)));
                self.player_input(crate::player::PlayerInput::EnqueueMany(list))
            }
            Msg::TogglePage => {
                let (what, on) = match &self.ui.spotify.page {
                    Some((_, Ok(Page::Playlist(pl)))) => {
                        (Saveable::Playlist(pl.playlist.id.clone()), !pl.saved)
                    }
                    Some((_, Ok(Page::Artist(a, saved)))) => {
                        (Saveable::Artist(a.artist.id.clone()), !saved)
                    }
                    Some((_, Ok(Page::Album(a, saved)))) => {
                        (Saveable::Album(a.album.id.clone()), !saved)
                    }
                    _ => return Task::none(),
                };
                if !self.spotify_can_edit() {
                    self.toast("Reconnect Spotify to change your library from Mosaic");
                    return Task::none();
                }
                self.set_page_saved(&what, on);
                self.spotify_change(what, on)
            }
            Msg::Changed(what, on, result) => {
                match result {
                    Ok(()) => {
                        let label = match (&what, on) {
                            (Saveable::Track(_), _) => None,
                            (Saveable::Artist(_), true) => Some("Following on Spotify"),
                            (Saveable::Artist(_), false) => Some("Unfollowed on Spotify"),
                            (_, true) => Some("Saved to Your Library on Spotify"),
                            (_, false) => Some("Removed from Your Library on Spotify"),
                        };
                        if let Some(l) = label {
                            self.toast(l);
                        }
                        if matches!(what, Saveable::Playlist(_)) {
                            return self.load_editable();
                        }
                    }
                    Err(e) => {
                        self.set_page_saved(&what, !on);
                        self.toast(e);
                    }
                }
                Task::none()
            }
            Msg::AddedTo(id, result) => {
                let name = self
                    .ui
                    .spotify
                    .editable
                    .iter()
                    .find(|p| p.id == id)
                    .map_or_else(|| "playlist".to_string(), |p| p.name.clone());
                match result {
                    Ok(()) => {
                        self.toast(format!("Added to {name} on Spotify"));
                        if self.spotify_route() == Route::Playlist(id) {
                            return self.load_route();
                        }
                    }
                    Err(e) => self.toast(e),
                }
                Task::none()
            }
            Msg::PollQueue => {
                if !self.playing_spotify() {
                    return Task::none();
                }
                let services = self.services.clone();
                let known = self.ui.spotify.queue.0.clone();
                Task::future(async move {
                    // Only an existing backend: never start Soloist just to read a queue.
                    let pb = services.spotify_playback_current().await?;
                    let uris = pb.queue();
                    if uris == known {
                        return None;
                    }
                    let tracks = services.spotify_tracks(&uris[..uris.len().min(20)]).await;
                    Some(Message::Spotify(Msg::Queue(uris, tracks)))
                })
                .and_then(Task::done)
            }
            Msg::Queue(uris, tracks) => {
                self.ui.spotify.queue = (uris, tracks);
                Task::none()
            }
            Msg::Moved { from, to } => {
                let services = self.services.clone();
                Task::perform(
                    async move {
                        let t = services.spotify_tracks(std::slice::from_ref(&to)).await;
                        (from, t.into_iter().next().map(Box::new))
                    },
                    |(from, t)| Message::Spotify(Msg::Adopt(from, t)),
                )
            }
            Msg::Adopt(from, track) => match track {
                Some(t) => {
                    self.player_input(crate::player::PlayerInput::AdoptSpotify { from, track: *t })
                }
                None => Task::none(),
            },
            Msg::Created(result) => match result {
                Ok(card) => {
                    self.toast(format!("Created “{}” on Spotify", card.name));
                    self.ui.spotify.editable.insert(0, card);
                    Task::none()
                }
                Err(e) => {
                    self.toast(e);
                    Task::none()
                }
            },
        }
    }

    /// Merges an account import into the store; safe to run repeatedly.
    fn apply_spotify_import(&mut self, all: Everything) -> (usize, usize) {
        let mut liked = 0;
        for t in all.liked.iter().rev() {
            if !self.store.is_liked(&t.id) {
                self.store.likes.insert(0, t.strip());
                liked += 1;
            }
        }
        let mut lists = 0;
        let mut put = |store: &mut Store, name: String, tracks: &[Track]| {
            if tracks.is_empty() {
                return;
            }
            match store.playlists.iter().position(|p| p.name == name) {
                Some(i) => {
                    let have: std::collections::HashSet<_> = store.playlists[i]
                        .tracks
                        .iter()
                        .map(|t| t.id.clone())
                        .collect();
                    let new = tracks
                        .iter()
                        .filter(|t| !have.contains(&t.id))
                        .map(Track::strip);
                    store.playlists[i].tracks.extend(new);
                }
                None => {
                    store.create_playlist(&name, tracks);
                    lists += 1;
                }
            }
        };
        for (name, tracks) in &all.playlists {
            put(&mut self.store, format!("{name} (Spotify)"), tracks);
        }
        put(
            &mut self.store,
            "Your top tracks (Spotify)".into(),
            &all.top,
        );
        put(&mut self.store, "Mix for you (Spotify)".into(), &all.mix);
        let known: std::collections::HashSet<_> = self
            .store
            .history
            .iter()
            .map(|h| (h.track.id.clone(), h.at))
            .collect();
        for (t, at) in all.recent {
            if !known.contains(&(t.id.clone(), at)) {
                self.store.history.push(mosaic_core::model::HistoryEntry {
                    track: t.strip(),
                    at,
                });
            }
        }
        self.store.history.sort_by_key(|h| std::cmp::Reverse(h.at));
        self.store.history.truncate(500);
        (liked, lists)
    }

    /// Tracks of the page on screen.
    pub(crate) fn spotify_page_tracks(&self) -> Vec<Track> {
        match &self.ui.spotify.page {
            Some((_, Ok(Page::Home(h)))) => h.liked.clone(),
            Some((_, Ok(Page::Liked(l)))) => l.clone(),
            Some((_, Ok(Page::Playlist(pl)))) => pl.tracks.clone(),
            Some((_, Ok(Page::Artist(a, _)))) => a.top.clone(),
            Some((_, Ok(Page::Album(a, _)))) => a.tracks.clone(),
            _ => Vec::new(),
        }
    }

    /// A track shelf on the home page.
    pub(crate) fn spotify_shelf(&self, shelf: Shelf) -> Vec<Track> {
        let Some((_, Ok(Page::Home(h)))) = &self.ui.spotify.page else {
            return Vec::new();
        };
        match shelf {
            Shelf::Recent => h.recent.clone(),
            Shelf::Top => h.top_tracks.clone(),
            Shelf::Liked => h.liked.clone(),
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
                .on_press(msg.clone()),
            button(
                container(text(title).size(14))
                    .height(40)
                    .padding(iced::Padding {
                        top: 12.0,
                        ..iced::Padding::ZERO
                    })
                    .clip(true)
            )
            .padding(0)
            .style(styles::bare(p))
            .on_press(msg),
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

    fn shelf_row<'a>(
        cards: impl Iterator<Item = Element<'a, Message>>,
        p: Palette,
    ) -> Element<'a, Message> {
        crate::widgets::smooth::carousel(
            scrollable(row(cards).spacing(20).padding(iced::Padding {
                bottom: 14.0,
                ..iced::Padding::ZERO
            }))
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .style(styles::scrollbar(p)),
        )
    }

    pub(crate) fn album_cards<'a>(
        &'a self,
        albums: &'a [AlbumCard],
        p: Palette,
    ) -> Element<'a, Message> {
        Self::shelf_row(
            albums.iter().map(|a| {
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
            }),
            p,
        )
    }

    pub(crate) fn artist_cards<'a>(
        &'a self,
        artists: &'a [ArtistCard],
        p: Palette,
    ) -> Element<'a, Message> {
        Self::shelf_row(
            artists.iter().map(|a| {
                self.card(
                    &a.image,
                    a.name.clone(),
                    "Artist".into(),
                    true,
                    Message::Spotify(Msg::Go(Route::Artist(a.id.clone()))),
                    p,
                )
            }),
            p,
        )
    }

    pub(crate) fn playlist_cards<'a>(
        &'a self,
        lists: &'a [PlaylistCard],
        liked_total: Option<u64>,
        p: Palette,
    ) -> Element<'a, Message> {
        let liked = liked_total.map(|n| self.liked_card(n, p));
        Self::shelf_row(
            liked.into_iter().chain(lists.iter().map(|l| {
                self.card(
                    &l.image,
                    l.name.clone(),
                    format!("Playlist · {}", l.owner),
                    false,
                    Message::Spotify(Msg::Go(Route::Playlist(l.id.clone()))),
                    p,
                )
            })),
            p,
        )
    }

    fn liked_card<'a>(&'a self, total: u64, p: Palette) -> Element<'a, Message> {
        let go = Message::Spotify(Msg::Go(Route::Liked));
        column![
            button(
                container(icon(Icon::Favorite, 56.0).color(p.on_primary_container))
                    .center(148)
                    .style(styles::fill(p.primary_container, 16.0))
            )
            .padding(0)
            .style(styles::bare(p))
            .on_press(go.clone()),
            button(
                container(text("Liked Songs").size(14))
                    .height(40)
                    .padding(iced::Padding {
                        top: 12.0,
                        ..iced::Padding::ZERO
                    })
            )
            .padding(0)
            .style(styles::bare(p))
            .on_press(go),
            text(format!("Playlist · {}", songs(total)))
                .size(12)
                .color(p.on_surface_variant),
        ]
        .spacing(2)
        .width(148)
        .into()
    }

    pub(crate) fn tile_shelf<'a>(
        &'a self,
        list: &'a [Track],
        which: ListRef,
        p: Palette,
    ) -> Element<'a, Message> {
        Self::shelf_row(
            list.iter().enumerate().map(|(i, t)| {
                self.tile(
                    t,
                    TrackRef {
                        list: which.clone(),
                        index: i,
                    },
                    148.0,
                    p,
                )
            }),
            p,
        )
    }

    /// Page header: cover, kind, title, details, then Play / Shuffle / Queue / Save.
    #[allow(clippy::too_many_arguments)]
    fn sp_head<'a>(
        &'a self,
        cover: Element<'a, Message>,
        eyebrow: String,
        title: String,
        description: String,
        meta: String,
        save: Option<(bool, &'static str, &'static str)>,
        p: Palette,
    ) -> Element<'a, Message> {
        let mut actions = row![
            super::common::primary_button(
                "Play",
                Icon::PlayArrow,
                p,
                Message::Spotify(Msg::PlayAll)
            ),
            tonal_button(
                "Shuffle",
                Icon::Shuffle,
                p,
                Message::Spotify(Msg::ShuffleAll)
            ),
            tonal_button(
                "Add to queue",
                Icon::QueueMusic,
                p,
                Message::Spotify(Msg::EnqueueAll)
            ),
        ]
        .spacing(8);
        if let Some((saved, on, off)) = save {
            let glyph = if saved {
                icon(Icon::Favorite, 20.0).color(p.primary)
            } else {
                outlined(Icon::Favorite, 20.0)
            };
            actions = actions.push(
                button(
                    container(
                        row![glyph, text(if saved { on } else { off }).size(14)]
                            .spacing(8)
                            .align_y(Alignment::Center),
                    )
                    .center_y(Length::Fill),
                )
                .height(40)
                .padding([0, 16])
                .style(styles::ghost(p))
                .on_press(Message::Spotify(Msg::TogglePage)),
            );
        }
        let mut info = column![
            text(eyebrow).size(14).color(p.on_surface_variant),
            text(title).size(32),
        ]
        .spacing(4)
        .width(Length::Fill);
        if !description.is_empty() {
            info = info.push(
                container(text(description).size(14).color(p.on_surface_variant))
                    .max_height(40)
                    .clip(true),
            );
        }
        info = info
            .push(text(meta).size(14).color(p.on_surface_variant))
            .push(container(actions.wrap()).padding(iced::Padding {
                top: 16.0,
                ..iced::Padding::ZERO
            }));
        row![cover, info]
            .spacing(28)
            .align_y(Alignment::End)
            .padding([8, 0])
            .into()
    }

    fn reconnect_banner<'a>(&self, p: Palette) -> Element<'a, Message> {
        container(
            row![
                text("Reconnect Spotify so Mosaic can like songs, save albums, follow artists and edit playlists.")
                    .size(14)
                    .width(Length::Fill),
                tonal_button(
                    if self.ui.spotify.connecting {
                        "Waiting for Spotify…"
                    } else {
                        "Reconnect"
                    },
                    Icon::Link,
                    p,
                    Message::Spotify(Msg::Connect)
                ),
            ]
            .spacing(16)
            .align_y(Alignment::Center),
        )
        .padding([12, 16])
        .width(Length::Fill)
        .style(styles::fill(p.surface_container_high, 16.0))
        .into()
    }

    fn rows<'a>(&'a self, tracks: &'a [Track], top: f32, p: Palette) -> Element<'a, Message> {
        let range = visible_range(tracks.len(), self.ui.scroll.0, self.ui.scroll.1, top);
        virtual_rows(tracks.len(), range, |i| {
            self.track_row(
                &tracks[i],
                i,
                TrackRef {
                    list: ListRef::SpotifyPage,
                    index: i,
                },
                true,
                p,
            )
        })
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
            body =
                body.push(self.page_header("Spotify".into(), s.status.user.clone(), Vec::new(), p));
        }
        if s.status.connected && !s.status.can_edit {
            body = body
                .push(self.reconnect_banner(p))
                .push(Space::new().height(16));
        }
        let square = |src: &str, round: bool| self.art(src, 168.0, if round { 84.0 } else { 20.0 });
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
            Some((_, Ok(Page::Home(h)))) => {
                body = body.push(h3("Your playlists")).push(self.playlist_cards(
                    &h.playlists,
                    Some(h.liked_total),
                    p,
                ));
                let shelves: [(&str, &[Track], Shelf); 3] = [
                    ("Recently played", &h.recent, Shelf::Recent),
                    ("Your top tracks this month", &h.top_tracks, Shelf::Top),
                    ("Liked Songs", &h.liked, Shelf::Liked),
                ];
                for (title, list, shelf) in shelves {
                    if !list.is_empty() {
                        body = body.push(h3(title)).push(self.tile_shelf(
                            list,
                            ListRef::SpotifyShelf(shelf),
                            p,
                        ));
                    }
                }
                let artist_shelves = [
                    ("Your top artists", &h.top_artists),
                    ("Artists you follow", &h.artists),
                ];
                for (title, list) in artist_shelves {
                    if !list.is_empty() {
                        body = body.push(h3(title)).push(self.artist_cards(list, p));
                    }
                }
                let album_shelves = [
                    ("Your albums", &h.albums),
                    ("New releases", &h.new_releases),
                ];
                for (title, list) in album_shelves {
                    if !list.is_empty() {
                        body = body.push(h3(title)).push(self.album_cards(list, p));
                    }
                }
            }
            Some((_, Ok(Page::Liked(list)))) => {
                let cover = container(icon(Icon::Favorite, 72.0).color(p.on_primary_container))
                    .center(168)
                    .style(styles::fill(p.primary_container, 20.0))
                    .into();
                body = body.push(self.sp_head(
                    cover,
                    "Playlist".into(),
                    "Liked Songs".into(),
                    String::new(),
                    format!(
                        "{} · {} · {}",
                        s.status.user.clone().unwrap_or_default(),
                        songs(list.len() as u64),
                        total_length(list)
                    ),
                    None,
                    p,
                ));
                body = body
                    .push(Space::new().height(16))
                    .push(self.rows(list, 330.0, p));
            }
            Some((_, Ok(Page::Playlist(pl)))) => {
                let c = &pl.playlist;
                let own = c.owner_id == self.services.spotify_user_id();
                body = body.push(self.sp_head(
                    square(&c.image, false),
                    if c.collaborative {
                        "Collaborative playlist".into()
                    } else {
                        "Playlist".into()
                    },
                    c.name.clone(),
                    c.description.clone(),
                    format!(
                        "{} · {} · {}",
                        c.owner,
                        songs(c.total.max(pl.tracks.len() as u64)),
                        total_length(&pl.tracks)
                    ),
                    (!own).then_some((pl.saved, "Saved", "Save")),
                    p,
                ));
                if pl.tracks.is_empty() {
                    body = body.push(h3("This playlist is empty"));
                }
                body = body
                    .push(Space::new().height(16))
                    .push(self.rows(&pl.tracks, 380.0, p));
            }
            Some((_, Ok(Page::Artist(a, follows)))) => {
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
                    square(&a.artist.image, true),
                    "Artist".into(),
                    a.artist.name.clone(),
                    String::new(),
                    meta,
                    Some((*follows, "Following", "Follow")),
                    p,
                ));
                if !a.top.is_empty() {
                    body = body.push(h3("Popular")).push(self.rows(
                        &a.top[..a.top.len().min(10)],
                        420.0,
                        p,
                    ));
                }
                if !a.albums.is_empty() {
                    body = body
                        .push(h3("Discography"))
                        .push(self.album_cards(&a.albums, p));
                }
            }
            Some((_, Ok(Page::Album(a, saved)))) => {
                let meta = format!(
                    "{} · {} · {} · {}",
                    a.album.artist,
                    a.album.year,
                    songs(a.album.total.unwrap_or(a.tracks.len() as u64)),
                    total_length(&a.tracks)
                );
                let artist_link: Option<Element<'_, Message>> =
                    a.album.artist_id.clone().map(|id| {
                        ghost_button(
                            &format!("More by {}", a.album.artist),
                            Icon::Person,
                            p,
                            Message::Spotify(Msg::Go(Route::Artist(id))),
                        )
                    });
                body = body.push(self.sp_head(
                    square(&a.album.image, false),
                    if a.album.kind == "single" {
                        "Single".into()
                    } else {
                        "Album".into()
                    },
                    a.album.name.clone(),
                    String::new(),
                    meta,
                    Some((*saved, "Saved", "Save")),
                    p,
                ));
                body = body
                    .push(Space::new().height(16))
                    .push(self.rows(&a.tracks, 360.0, p));
                if let Some(link) = artist_link {
                    body = body.push(Space::new().height(16)).push(link);
                }
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
        assert_ne!(
            Route::Playlist("a".into()).key(),
            Route::Album("a".into()).key()
        );
        assert_eq!(Route::Home.key(), "home");
    }

    #[test]
    fn lengths_read_like_spotify() {
        let t = |ms: u64| {
            let mut t: Track = serde_json::from_value(serde_json::json!({
                "id": "sp:x", "source": "spotify", "title": "t", "artist": "a"
            }))
            .unwrap();
            t.duration = ms;
            t
        };
        assert_eq!(total_length(&[t(125_000), t(60_000)]), "3 min 5 sec");
        assert_eq!(total_length(&[t(3_900_000)]), "1 hr 5 min");
        assert_eq!(songs(1), "1 song");
    }
}
