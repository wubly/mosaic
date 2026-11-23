//! Port of `Discover.js` (YouTube and, until M11, Spotify sections left out).

use std::time::Duration;

use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Space,
};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::Track;
use mosaic_core::relevance::filter_along;

use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::logo;
use crate::widgets::smooth::carousel;

pub const GENRES: [(&str, &str); 7] = [
    ("all-music", "Trending"),
    ("hip hop", "Hip hop"),
    ("electronic", "Electronic"),
    ("pop", "Pop"),
    ("rnb", "R&B"),
    ("rock", "Rock"),
    ("lofi", "Lo-fi"),
];

pub const SEARCH_ID: &str = "discover-search";
const DEBOUNCE: Duration = Duration::from_millis(400);

/// Where Discover's search looks. One service at a time, picked from the dropdown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchService {
    #[default]
    SoundCloud,
    Spotify,
    YouTube,
    Device,
}

impl SearchService {
    fn label(self) -> &'static str {
        match self {
            SearchService::SoundCloud => "SoundCloud",
            SearchService::Spotify => "Spotify",
            SearchService::YouTube => "YouTube",
            SearchService::Device => "This device",
        }
    }
}

impl std::fmt::Display for SearchService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Service(SearchService),
    Query(String),
    Debounced(u64),
    Submit,
    Clear,
    Results(u64, Result<Vec<Track>, String>),
    YoutubeResults(u64, Result<Vec<Track>, String>),
    Genre(&'static str),
    Charts(&'static str, Result<Vec<Track>, String>),
    More(String, Result<Vec<Track>, String>),
}

pub struct State {
    pub query: String,
    pub service: SearchService,
    pub results: Vec<Track>,
    pub youtube: Vec<Track>,
    pub busy: bool,
    /// The Spotify search is still out.
    pub sp_busy: bool,
    pub error: String,
    pub seq: u64,
    pub genre: &'static str,
    /// Genre the chart list belongs to, and the list.
    pub charts: (&'static str, Vec<Track>),
    pub charts_loaded: bool,
    /// Seed id and the "because you played" list.
    pub more: (Option<mosaic_core::model::TrackId>, Vec<Track>),
    pub more_busy: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            query: String::new(),
            service: SearchService::default(),
            results: Vec::new(),
            youtube: Vec::new(),
            busy: false,
            sp_busy: false,
            error: String::new(),
            seq: 0,
            genre: "all-music",
            charts: ("", Vec::new()),
            charts_loaded: false,
            more: (None, Vec::new()),
            more_busy: false,
        }
    }
}

impl App {
    pub(crate) fn discover_boot(&mut self) -> Task<Message> {
        Task::batch([self.load_charts(self.ui.discover.genre), self.load_more()])
    }

    fn load_charts(&mut self, genre: &'static str) -> Task<Message> {
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .sc_charts(genre, 30)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |r| Message::Discover(Msg::Charts(genre, r)),
        )
    }

    /// "Because you played …" follows the newest SoundCloud track in the history.
    pub(crate) fn load_more(&mut self) -> Task<Message> {
        let seed = self
            .store
            .history
            .iter()
            .map(|h| &h.track)
            .find(|t| t.source == mosaic_core::model::Source::SoundCloud)
            .cloned();
        let Some(seed) = seed else {
            self.ui.discover.more = (None, Vec::new());
            return Task::none();
        };
        if self.ui.discover.more.0.as_ref() == Some(&seed.id) {
            return Task::none();
        }
        self.ui.discover.more = (Some(seed.id.clone()), Vec::new());
        self.ui.discover.more_busy = true;
        let have: std::collections::HashSet<_> =
            self.recent_tracks().into_iter().map(|t| t.id).collect();
        let services = self.services.clone();
        let key = seed.id.key();
        Task::perform(
            async move {
                services
                    .sc_related(&seed.id, 40)
                    .await
                    .map(|list| {
                        filter_along(list, Some(&seed))
                            .into_iter()
                            .filter(|t| !have.contains(&t.id))
                            .take(20)
                            .collect()
                    })
                    .map_err(|e| e.to_string())
            },
            move |r| Message::Discover(Msg::More(key.clone(), r)),
        )
    }

    pub(crate) fn on_discover(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Query(q) => {
                self.ui.discover.query = q;
                let trimmed = self.ui.discover.query.trim().to_string();
                self.ui.discover.seq += 1;
                let seq = self.ui.discover.seq;
                if trimmed.chars().count() > 1 {
                    self.ui.discover.busy = true;
                    self.ui.discover.error.clear();
                } else if trimmed.is_empty() {
                    self.clear_search_results();
                    return Task::none();
                }
                Task::perform(tokio::time::sleep(DEBOUNCE), move |()| {
                    Message::Discover(Msg::Debounced(seq))
                })
            }
            Msg::Debounced(seq) => {
                if seq != self.ui.discover.seq || self.ui.discover.query.trim().chars().count() < 2
                {
                    return Task::none();
                }
                self.run_search()
            }
            Msg::Submit => {
                if self.ui.discover.query.trim().is_empty() {
                    return Task::none();
                }
                self.ui.discover.seq += 1;
                self.run_search()
            }
            Msg::Clear => {
                self.ui.discover.query.clear();
                self.clear_search_results();
                self.ui.discover.seq += 1;
                iced::widget::operation::focus(SEARCH_ID)
            }
            Msg::Service(service) => {
                self.ui.discover.service = service;
                self.ui.discover.seq += 1;
                self.ui.discover.error.clear();
                if self.ui.discover.query.trim().chars().count() > 1 {
                    return self.run_search();
                }
                self.clear_search_results();
                Task::none()
            }
            Msg::YoutubeResults(seq, result) => {
                if seq != self.ui.discover.seq {
                    return Task::none();
                }
                self.ui.discover.busy = false;
                match result {
                    Ok(list) => self.ui.discover.youtube = filter_along(list, None),
                    Err(e) => self.ui.discover.error = e,
                }
                Task::none()
            }
            Msg::Results(seq, result) => {
                if seq != self.ui.discover.seq {
                    return Task::none();
                }
                self.ui.discover.busy = false;
                match result {
                    Ok(list) => self.ui.discover.results = filter_along(list, None),
                    Err(e) => self.ui.discover.error = e,
                }
                Task::none()
            }
            Msg::Genre(g) => {
                self.ui.discover.genre = g;
                self.ui.discover.charts_loaded = false;
                self.load_charts(g)
            }
            Msg::Charts(genre, result) => {
                if genre == self.ui.discover.genre {
                    self.ui.discover.charts =
                        (genre, filter_along(result.unwrap_or_default(), None));
                    self.ui.discover.charts_loaded = true;
                }
                Task::none()
            }
            Msg::More(key, result) => {
                if self.ui.discover.more.0.as_ref().map(|i| i.key()) == Some(key) {
                    self.ui.discover.more =
                        (self.ui.discover.more.0.clone(), result.unwrap_or_default());
                    self.ui.discover.more_busy = false;
                }
                Task::none()
            }
        }
    }

    /// The services a search can use right now. SoundCloud and the device are always there;
    /// YouTube needs the beta but not an account.
    pub(crate) fn search_services(&self) -> Vec<SearchService> {
        let mut list = vec![SearchService::SoundCloud];
        if self.spotify_enabled() {
            list.push(SearchService::Spotify);
        }
        if self.youtube_playable() {
            list.push(SearchService::YouTube);
        }
        list.push(SearchService::Device);
        list
    }

    /// The picked service, or SoundCloud if the pick is gone (disconnected, beta turned off).
    pub(crate) fn search_service(&self) -> SearchService {
        let picked = self.ui.discover.service;
        if self.search_services().contains(&picked) {
            picked
        } else {
            SearchService::SoundCloud
        }
    }

    fn clear_search_results(&mut self) {
        let d = &mut self.ui.discover;
        d.results.clear();
        d.youtube.clear();
        d.busy = false;
        d.sp_busy = false;
        self.ui.spotify.search = Default::default();
    }

    /// Song hits of the picked service.
    pub(crate) fn search_tracks(&self) -> Vec<Track> {
        match self.search_service() {
            SearchService::SoundCloud => self.ui.discover.results.clone(),
            SearchService::Spotify => self.ui.spotify.search.tracks.clone(),
            SearchService::YouTube => self.ui.discover.youtube.clone(),
            SearchService::Device => self.local_hits(60),
        }
    }

    fn run_search(&mut self) -> Task<Message> {
        let q = self.ui.discover.query.trim().to_string();
        let seq = self.ui.discover.seq;
        let service = self.search_service();
        self.ui.discover.error.clear();
        self.ui.discover.busy =
            matches!(service, SearchService::SoundCloud | SearchService::YouTube);
        self.ui.discover.sp_busy = service == SearchService::Spotify;
        let services = self.services.clone();
        match service {
            SearchService::SoundCloud => Task::perform(
                async move { services.sc_search(&q, 30).await.map_err(|e| e.to_string()) },
                move |r| Message::Discover(Msg::Results(seq, r)),
            ),
            SearchService::Spotify => self.search_spotify(q, seq),
            SearchService::YouTube => Task::perform(
                async move { services.yt_search(&q).await.map_err(|e| e.to_string()) },
                move |r| Message::Discover(Msg::YoutubeResults(seq, r)),
            ),
            SearchService::Device => Task::none(),
        }
    }

    /// Opens Discover with a search term (command palette, `/`).
    pub(crate) fn start_search(&mut self, q: String) -> Task<Message> {
        self.ui.view = Some(crate::widgets::rail::View::Discover);
        self.retarget_rail();
        self.ui.now_playing = false;
        let focus = iced::widget::operation::focus(SEARCH_ID);
        if q.is_empty() {
            return focus;
        }
        Task::batch([
            self.on_discover(Msg::Query(q)),
            self.on_discover(Msg::Submit),
            focus,
        ])
    }

    fn skeleton_tile(&self, width: f32, p: Palette) -> Element<'_, Message> {
        let block = |w: Length, h: f32, r: f32| {
            container(Space::new().width(w).height(h))
                .style(styles::fill(p.surface_container_high, r))
        };
        column![
            block(Length::Fixed(width), width, 16.0),
            block(Length::Fixed(width * 0.85), 12.0, 12.0),
            block(Length::Fixed(width * 0.55), 10.0, 12.0),
        ]
        .spacing(10)
        .into()
    }

    fn shelf<'a>(&'a self, list: &'a [Track], which: ListRef, p: Palette) -> Element<'a, Message> {
        carousel(
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
                    top: 0.0,
                    right: 4.0,
                    bottom: 14.0,
                    left: 4.0,
                }),
            )
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(6).scroller_width(6),
            ))
            .style(styles::scrollbar(p)),
        )
    }

    fn grid<'a>(&'a self, list: Vec<Track>, which: ListRef, p: Palette) -> Element<'a, Message> {
        iced::widget::Row::with_children(list.iter().enumerate().map(|(i, t)| {
            self.tile(
                t,
                TrackRef {
                    list: which.clone(),
                    index: i,
                },
                156.0,
                p,
            )
        }))
        .spacing(20)
        .wrap()
        .vertical_spacing(32)
        .into()
    }

    fn search_bar(&self, p: Palette) -> Element<'_, Message> {
        let d = &self.ui.discover;
        let input = text_input("Search songs, artists…", &d.query)
            .id(SEARCH_ID)
            .on_input(|q| Message::Discover(Msg::Query(q)))
            .on_submit(Message::Discover(Msg::Submit))
            .size(16)
            .padding(0)
            .style(move |_, _| iced::widget::text_input::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border::default(),
                icon: p.on_surface_variant,
                placeholder: p.on_surface_variant,
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            });
        let tail: Element<'_, Message> = if d.busy || d.sp_busy {
            crate::widgets::spinner::spinner(
                20.0,
                self.now
                    .saturating_duration_since(self.epoch)
                    .as_secs_f32()
                    .fract(),
                p.primary,
            )
        } else if !d.query.is_empty() {
            button(container(icon(Icon::Close, 18.0)).center(Length::Fill))
                .width(32)
                .height(32)
                .padding(0)
                .style(styles::icon_button(p, false))
                .on_press(Message::Discover(Msg::Clear))
                .into()
        } else {
            container(text("/").size(12))
                .padding([1, 7])
                .style(styles::kbd(p))
                .into()
        };
        container(
            row![
                icon(Icon::Search, 22.0).color(p.on_surface_variant),
                input,
                tail
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([0, 20])
        .height(56)
        .max_width(520)
        .center_y(56)
        .style(styles::fill(p.surface_container_high, 28.0))
        .into()
    }

    /// The search bar with the service picker beside it, or under it when the page is narrow.
    fn search_row(&self, p: Palette) -> Element<'_, Message> {
        if self.main_width() < 680.0 {
            column![self.search_bar(p), self.service_picker(p)]
                .spacing(12)
                .into()
        } else {
            row![self.search_bar(p), self.service_picker(p)]
                .spacing(12)
                .align_y(Alignment::Center)
                .into()
        }
    }

    fn service_picker(&self, p: Palette) -> Element<'_, Message> {
        let options = self.search_services();
        pick_list(options, Some(self.search_service()), |s| {
            Message::Discover(Msg::Service(s))
        })
        .text_size(14)
        .padding([10, 16])
        .style(move |_, status| {
            let open = matches!(status, pick_list::Status::Opened { .. });
            pick_list::Style {
                text_color: p.on_surface,
                placeholder_color: p.on_surface_variant,
                handle_color: p.on_surface_variant,
                background: if open {
                    p.surface_container_highest
                } else {
                    p.surface_container_high
                }
                .into(),
                border: iced::border::rounded(28),
            }
        })
        .menu_style(move |_| iced::overlay::menu::Style {
            background: p.surface_container_highest.into(),
            border: iced::border::rounded(16),
            text_color: p.on_surface,
            selected_text_color: p.on_primary_container,
            selected_background: p.primary_container.into(),
            shadow: iced::Shadow::default(),
        })
        .into()
    }

    pub(crate) fn discover_view(&self, p: Palette) -> Element<'_, Message> {
        let d = &self.ui.discover;
        let name = self.services.config().name.unwrap_or_default();
        let title = if name.is_empty() {
            "What's next?".to_string()
        } else {
            format!("Hey {name}, what's next?")
        };
        let h3 = |s: String| {
            container(text(s).size(16).color(p.on_surface_variant)).padding(iced::Padding {
                top: 32.0,
                bottom: 16.0,
                ..iced::Padding::ZERO
            })
        };
        let mut body = column![
            row![logo::mark(18.0), text("Mosaic").size(14).color(p.primary)]
                .spacing(8)
                .align_y(Alignment::Center),
            text(title).size(28),
            Space::new().height(24),
            self.search_row(p),
        ]
        .spacing(4);

        if !d.query.trim().is_empty() {
            let service = self.search_service();
            let sp = &self.ui.spotify.search;
            let on_spotify = service == SearchService::Spotify;
            if on_spotify && !sp.artists.is_empty() {
                body = body
                    .push(h3("Spotify artists (beta)".into()))
                    .push(self.artist_cards(&sp.artists, p));
            }
            if on_spotify && !sp.albums.is_empty() {
                body = body
                    .push(h3("Spotify albums".into()))
                    .push(self.album_cards(&sp.albums, p));
            }
            if on_spotify && !sp.playlists.is_empty() {
                body = body
                    .push(h3("Spotify playlists".into()))
                    .push(self.playlist_cards(&sp.playlists, None, p));
            }
            body = body.push(
                row![
                    h3(format!("Songs for “{}”", d.query.trim())),
                    Space::new().width(Length::Fill),
                    text(if d.busy || d.sp_busy {
                        format!("Searching {service}…")
                    } else {
                        format!("From {service}")
                    })
                    .size(12)
                    .color(p.on_surface_variant),
                ]
                .align_y(Alignment::Center),
            );
            if !d.error.is_empty() {
                body = body.push(
                    container(text(d.error.clone()).size(14))
                        .padding([12, 16])
                        .width(Length::Fill)
                        .style(styles::error_box(p)),
                );
            }
            let songs = self.search_tracks();
            let any_busy = d.busy || d.sp_busy;
            if any_busy && songs.is_empty() {
                body = body.push(
                    iced::widget::Row::with_children((0..12).map(|_| self.skeleton_tile(156.0, p)))
                        .spacing(20)
                        .wrap()
                        .vertical_spacing(32),
                );
            } else if !any_busy && d.error.is_empty() && songs.is_empty() {
                body = body.push(text("Nothing found.").color(p.on_surface_variant));
            } else {
                body = body.push(self.grid(songs, ListRef::Results, p));
            }
        } else {
            let recent = self.recent.as_slice();
            if !recent.is_empty() {
                body = body.push(h3("Jump back in".into())).push(self.shelf(
                    recent,
                    ListRef::Recent,
                    p,
                ));
            }
            if !self.ui.spotify.saved.is_empty() {
                body = body
                    .push(h3("Your Spotify likes (beta)".into()))
                    .push(self.tile_shelf(&self.ui.spotify.saved, ListRef::SpotifySaved, p));
            }
            let chips = iced::widget::Row::with_children(GENRES.iter().map(|(g, label)| {
                button(container(text(*label).size(14)).center_y(Length::Fill))
                    .height(32)
                    .padding([0, 12])
                    .style(styles::chip(p, *g == d.genre))
                    .on_press(Message::Discover(Msg::Genre(g)))
                    .into()
            }))
            .spacing(4)
            .wrap();
            let genre_title = GENRES
                .iter()
                .find(|(g, _)| *g == d.genre)
                .map_or("", |(_, l)| *l);
            body = body.push(
                row![
                    container(text(genre_title).size(16).color(p.on_surface_variant)),
                    Space::new().width(Length::Fill),
                    chips,
                ]
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    top: 32.0,
                    bottom: 16.0,
                    ..iced::Padding::ZERO
                }),
            );
            if !d.charts_loaded || d.charts.0 != d.genre {
                body = body.push(
                    iced::widget::Row::with_children((0..8).map(|_| self.skeleton_tile(148.0, p)))
                        .spacing(20),
                );
            } else if d.charts.1.is_empty() {
                body = body.push(text("Couldn’t load this right now.").color(p.on_surface_variant));
            } else {
                body = body.push(self.shelf(&d.charts.1, ListRef::Charts, p));
            }
            if let (Some(_), true) = (&d.more.0, d.more_busy || !d.more.1.is_empty()) {
                let seed = self
                    .store
                    .history
                    .iter()
                    .map(|h| &h.track)
                    .find(|t| Some(&t.id) == d.more.0.as_ref());
                let title = seed.map_or(String::new(), |t| {
                    format!("Because you played “{}”", t.title)
                });
                body = body.push(h3(title));
                if d.more_busy && d.more.1.is_empty() {
                    body = body.push(
                        iced::widget::Row::with_children(
                            (0..6).map(|_| self.skeleton_tile(148.0, p)),
                        )
                        .spacing(20),
                    );
                } else {
                    body = body.push(self.shelf(&d.more.1, ListRef::More, p));
                }
            }
        }
        super::common::inset_scroll(
            scrollable(
                container(body)
                    .padding(iced::Padding {
                        top: 32.0,
                        right: 40.0,
                        bottom: 24.0,
                        left: 40.0,
                    })
                    .width(Length::Fill),
            ),
            p,
        )
    }
}
