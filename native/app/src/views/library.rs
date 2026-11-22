//! Library tabs, playlists and stats (`LibraryView.js`, `LikedList.js`, `HistoryList.js`,
//! `Stats.js`, `PlaylistsTab.js`, `PlaylistDetail.js`, `Collage.js`).

use iced::widget::{column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::format::{ago, hm};
use mosaic_core::model::Track;
use mosaic_core::stats::summarize;

use super::artwork_of;
use super::common::{
    empty_state, ghost_button, icon_button, page, primary_button, tonal_button, virtual_rows,
    visible_range,
};
use super::reorder::{preview_order, Kind, State as Reorder};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::rail::View;

const LIST_TOP: f32 = 330.0;
const MAX_REORDERABLE: usize = 250;
const WEEKDAY: [&str; 7] = ["S", "M", "T", "W", "T", "F", "S"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Liked,
    Playlists,
    History,
    Stats,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::Liked, "Liked"),
    (Tab::Playlists, "Playlists"),
    (Tab::History, "History"),
    (Tab::Stats, "Stats"),
];

#[derive(Clone, Debug)]
pub enum Msg {
    Tab(Tab),
    Open(String),
    Back,
    NewPlaylist,
    Rename,
    RenameInput(String),
    StartRename,
    Delete,
    RemoveFrom(String, usize),
    ClearHistory,
    PlayAll(ListRef),
    ShuffleAll(ListRef),
    AddToQueue(String),
}

#[derive(Default)]
pub struct State {
    pub tab: Tab,
    pub open: Option<String>,
    pub editing: Option<String>,
}

pub fn plural(n: u64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl App {
    pub(crate) fn on_library(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Tab(t) => {
                self.ui.library.tab = t;
                self.ui.library.open = None;
                self.ui.scroll = (0.0, self.ui.scroll.1);
                if t == Tab::Stats {
                    self.anim.reset("stats-grow", 0.0, 140.0, 20.0);
                    self.anim.spring("stats-grow", 1.0, 140.0, 20.0);
                }
                Task::none()
            }
            Msg::Open(id) => {
                self.ui.library.open = Some(id);
                Task::none()
            }
            Msg::Back => {
                self.ui.library.open = None;
                self.ui.library.editing = None;
                Task::none()
            }
            Msg::NewPlaylist => {
                let id = self.store.create_playlist("New playlist", &[]);
                self.touch();
                self.ui.library.open = Some(id);
                Task::none()
            }
            Msg::StartRename => {
                let name = self
                    .open_playlist()
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                self.ui.library.editing = Some(name);
                iced::widget::operation::focus("playlist-name")
            }
            Msg::RenameInput(s) => {
                self.ui.library.editing = Some(s);
                Task::none()
            }
            Msg::Rename => {
                if let (Some(id), Some(name)) =
                    (self.ui.library.open.clone(), self.ui.library.editing.take())
                {
                    self.store.rename_playlist(&id, &name);
                    self.touch();
                }
                Task::none()
            }
            Msg::Delete => {
                if let Some(id) = self.ui.library.open.take() {
                    self.store.delete_playlist(&id);
                    self.touch();
                    self.toast("Playlist deleted");
                }
                Task::none()
            }
            Msg::RemoveFrom(id, index) => {
                let track_id = self
                    .store
                    .playlists
                    .iter()
                    .find(|p| p.id == id)
                    .and_then(|p| p.tracks.get(index))
                    .map(|t| t.id.clone());
                if let Some(t) = track_id {
                    self.store.remove_from_playlist(&id, &t);
                    self.touch();
                }
                Task::none()
            }
            Msg::ClearHistory => {
                self.store.clear_history();
                self.touch();
                self.refresh_recent();
                Task::none()
            }
            Msg::PlayAll(list) => {
                let tracks = self.resolve_list(&list);
                match tracks.first().cloned() {
                    Some(first) => self.player_input(PlayerInput::Play(first, Some(tracks))),
                    None => Task::none(),
                }
            }
            Msg::ShuffleAll(list) => {
                let tracks = self.resolve_list(&list);
                if tracks.is_empty() {
                    return Task::none();
                }
                let mut tasks = Vec::new();
                if !self.settings().shuffle {
                    tasks.push(self.player_input(PlayerInput::ToggleShuffle));
                }
                let pick = tracks[rand::random_range(0..tracks.len())].clone();
                tasks.push(self.player_input(PlayerInput::Play(pick, Some(tracks))));
                Task::batch(tasks)
            }
            Msg::AddToQueue(id) => {
                let tracks = self
                    .store
                    .playlists
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.tracks.clone())
                    .unwrap_or_default();
                self.player_input(PlayerInput::EnqueueMany(tracks))
            }
        }
    }

    fn open_playlist(&self) -> Option<&mosaic_core::model::Playlist> {
        let id = self.ui.library.open.as_ref()?;
        self.store.playlists.iter().find(|p| &p.id == id)
    }

    fn play_all<'a>(&self, list: ListRef, p: Palette) -> Vec<Element<'a, Message>> {
        vec![
            primary_button(
                "Play",
                Icon::PlayArrow,
                p,
                Message::Library(Msg::PlayAll(list.clone())),
            ),
            tonal_button(
                "Shuffle",
                Icon::Shuffle,
                p,
                Message::Library(Msg::ShuffleAll(list)),
            ),
        ]
    }

    pub(crate) fn collage<'a>(
        &'a self,
        tracks: &[Track],
        size: f32,
        p: Palette,
    ) -> Element<'a, Message> {
        let covers: Vec<&str> = tracks
            .iter()
            .map(artwork_of)
            .filter(|s| !s.is_empty())
            .take(4)
            .collect();
        let half = size / 2.0;
        let cell =
            |src: &str, w: f32, h: f32| -> Element<'a, Message> { self.art_sized(src, w, h, 0.0) };
        let content: Element<'a, Message> = match covers.len() {
            0 => container(icon(Icon::PlaylistPlay, 40.0).color(p.on_surface_variant))
                .center(size)
                .into(),
            1 => cell(covers[0], size, size),
            2 => row![cell(covers[0], half, size), cell(covers[1], half, size)].into(),
            3 => row![
                cell(covers[0], half, size),
                column![cell(covers[1], half, half), cell(covers[2], half, half)]
            ]
            .into(),
            _ => column![
                row![cell(covers[0], half, half), cell(covers[1], half, half)],
                row![cell(covers[2], half, half), cell(covers[3], half, half)],
            ]
            .into(),
        };
        container(content)
            .width(size)
            .height(size)
            .clip(true)
            .style(styles::fill(p.surface_container_highest, 20.0))
            .into()
    }

    pub(crate) fn library_view(&self, p: Palette) -> Element<'_, Message> {
        let playlists_only = self.view_kind() == View::Playlists;
        let tab = if playlists_only {
            Tab::Playlists
        } else {
            self.ui.library.tab
        };
        let count = self.store.playlists.len() as u64;
        let subtitle = if playlists_only {
            format!("{count} playlists")
        } else {
            format!("{} liked · {count} playlists", self.store.likes.len())
        };
        let mut actions = Vec::new();
        if playlists_only && self.ui.library.open.is_none() {
            actions.push(tonal_button(
                "Import from Spotify",
                Icon::CloudDownload,
                p,
                Message::OpenImport,
            ));
            actions.push(primary_button(
                "New playlist",
                Icon::Add,
                p,
                Message::Library(Msg::NewPlaylist),
            ));
        }
        let mut body = column![self.page_header(
            if playlists_only {
                "Playlists".into()
            } else {
                "Your library".into()
            },
            Some(subtitle),
            actions,
            p
        )];
        if !playlists_only {
            let labels: Vec<(&'static str, bool)> =
                TABS.iter().map(|(t, l)| (*l, *t == tab)).collect();
            body = body.push(self.tabs(&labels, |i| Message::Library(Msg::Tab(TABS[i].0)), p));
        }
        body = body.push(match tab {
            Tab::Liked => self.liked_list(p),
            Tab::Playlists => self.playlists_tab(p),
            Tab::History => self.history_list(p),
            Tab::Stats => self.stats_view(p),
        });
        page(body, p)
    }

    fn track_list<'a>(
        &'a self,
        list: &'a [Track],
        which: ListRef,
        p: Palette,
    ) -> Element<'a, Message> {
        let range = visible_range(list.len(), self.ui.scroll.0, self.ui.scroll.1, LIST_TOP);
        virtual_rows(list.len(), range, move |i| {
            self.track_row(
                &list[i],
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

    fn liked_list(&self, p: Palette) -> Element<'_, Message> {
        let likes = &self.store.likes;
        if likes.is_empty() {
            return empty_state(
                Icon::Favorite,
                "No liked songs yet",
                "Tap the heart on any track and it will land here.",
                None,
                p,
            );
        }
        column![
            row(self.play_all(ListRef::Likes, p))
                .spacing(8)
                .padding(iced::Padding {
                    bottom: 12.0,
                    ..iced::Padding::ZERO
                }),
            self.track_list(likes, ListRef::Likes, p),
        ]
        .into()
    }

    fn history_list(&self, p: Palette) -> Element<'_, Message> {
        let history = &self.store.history;
        if history.is_empty() {
            return empty_state(
                Icon::History,
                "Nothing played yet",
                "Tracks you listen to for a few seconds show up here.",
                None,
                p,
            );
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut rows = column![].spacing(6);
        for (i, h) in history.iter().enumerate() {
            let line = self.track_row(
                &h.track,
                i,
                TrackRef {
                    list: ListRef::History,
                    index: i,
                },
                false,
                p,
            );
            rows = rows.push(iced::widget::stack![
                line,
                container(text(ago(h.at, now_ms)).size(12).color(p.on_surface_variant))
                    .width(Length::Fill)
                    .height(48)
                    .center_y(48)
                    .align_x(iced::alignment::Horizontal::Right)
                    .padding(iced::Padding {
                        right: 150.0,
                        ..iced::Padding::ZERO
                    }),
            ]);
        }
        column![
            row![tonal_button(
                "Clear history",
                Icon::Delete,
                p,
                Message::Library(Msg::ClearHistory)
            )]
            .padding(iced::Padding {
                bottom: 12.0,
                ..iced::Padding::ZERO
            }),
            rows,
        ]
        .into()
    }

    fn playlists_tab(&self, p: Palette) -> Element<'_, Message> {
        if let Some(pl) = self.open_playlist() {
            return self.playlist_detail(pl, p);
        }
        if self.store.playlists.is_empty() {
            return empty_state(
                Icon::PlaylistAdd,
                "No playlists yet",
                "Create one, add tracks from any track’s menu, or bring a playlist over from Spotify.",
                Some(
                    row![
                        tonal_button("Import from Spotify", Icon::CloudDownload, p, Message::OpenImport),
                        primary_button("New playlist", Icon::Add, p, Message::Library(Msg::NewPlaylist)),
                    ]
                    .spacing(8)
                    .into(),
                ),
                p,
            );
        }
        iced::widget::Row::with_children(self.store.playlists.iter().map(|pl| {
            iced::widget::button(
                column![
                    self.collage(&pl.tracks, 160.0, p),
                    text(pl.name.clone()).size(14),
                    text(format!("{} tracks", pl.tracks.len()))
                        .size(14)
                        .color(p.on_surface_variant),
                ]
                .spacing(4)
                .width(160),
            )
            .padding(0)
            .style(styles::bare(p))
            .on_press(Message::Library(Msg::Open(pl.id.clone())))
            .into()
        }))
        .spacing(24)
        .wrap()
        .vertical_spacing(28)
        .into()
    }

    fn playlist_detail<'a>(
        &'a self,
        pl: &'a mosaic_core::model::Playlist,
        p: Palette,
    ) -> Element<'a, Message> {
        let total: f64 = pl.tracks.iter().map(|t| t.duration as f64).sum::<f64>() / 1000.0;
        let name: Element<'a, Message> = match &self.ui.library.editing {
            Some(n) => text_input("Playlist name", n)
                .id("playlist-name")
                .on_input(|s| Message::Library(Msg::RenameInput(s)))
                .on_submit(Message::Library(Msg::Rename))
                .size(32)
                .padding([0, 0])
                .style(move |_, _| iced::widget::text_input::Style {
                    background: iced::Color::TRANSPARENT.into(),
                    border: iced::Border {
                        color: p.primary,
                        width: 2.0,
                        radius: 0.0.into(),
                    },
                    icon: p.on_surface,
                    placeholder: p.on_surface_variant,
                    value: p.on_surface,
                    selection: crate::theme::alpha(p.primary, 0.35),
                })
                .into(),
            None => text(pl.name.clone()).size(32).into(),
        };
        let mut actions = self.play_all(ListRef::Playlist(pl.id.clone()), p);
        if !pl.tracks.is_empty() {
            actions.push(tonal_button(
                "Add to queue",
                Icon::QueueMusic,
                p,
                Message::Library(Msg::AddToQueue(pl.id.clone())),
            ));
        }
        actions.push(icon_button(
            Icon::Edit,
            "Rename",
            p,
            Message::Library(Msg::StartRename),
        ));
        actions.push(icon_button(
            Icon::Delete,
            "Delete playlist",
            p,
            Message::Library(Msg::Delete),
        ));
        let info = column![
            name,
            text(format!(
                "{} tracks{}",
                pl.tracks.len(),
                if total > 0.0 {
                    format!(" · {}", hm(total))
                } else {
                    String::new()
                }
            ))
            .size(14)
            .color(p.on_surface_variant),
            iced::widget::Row::with_children(actions)
                .spacing(8)
                .align_y(Alignment::Center)
                .padding(iced::Padding {
                    top: 16.0,
                    ..iced::Padding::ZERO
                }),
        ]
        .spacing(4)
        .width(Length::Fill);
        let head = row![self.collage(&pl.tracks, 168.0, p), info]
            .spacing(28)
            .align_y(Alignment::End)
            .padding([8, 0]);

        let list: Element<'a, Message> = if pl.tracks.is_empty() {
            empty_state(
                Icon::PlaylistPlay,
                "This playlist is empty",
                "Right-click any track and choose “Add to playlist”, or import one from Spotify.",
                None,
                p,
            )
        } else if pl.tracks.len() > MAX_REORDERABLE {
            self.track_list(&pl.tracks, ListRef::Playlist(pl.id.clone()), p)
        } else {
            let kind = Kind::Playlist(pl.id.clone());
            let order = match &self.ui.reorder {
                Some(Reorder {
                    kind: k,
                    from,
                    to,
                    len,
                    ..
                }) if *k == kind => preview_order(*len, *from, *to),
                _ => (0..pl.tracks.len()).collect(),
            };
            let mut rows = column![].spacing(6);
            for (shown, src) in order.into_iter().enumerate() {
                let t = &pl.tracks[src];
                let remove = self.remove_button(
                    Message::Library(Msg::RemoveFrom(pl.id.clone(), src)),
                    p,
                    "Remove from playlist",
                );
                rows = rows.push(self.reorder_row(
                    t,
                    shown,
                    src,
                    TrackRef {
                        list: ListRef::Playlist(pl.id.clone()),
                        index: src,
                    },
                    &kind,
                    pl.tracks.len(),
                    remove,
                    p,
                ));
            }
            rows.into()
        };
        column![
            ghost_button("Playlists", Icon::ArrowBack, p, Message::Library(Msg::Back)),
            head,
            Space::new().height(20),
            list
        ]
        .into()
    }

    fn stats_view(&self, p: Palette) -> Element<'_, Message> {
        let today = chrono::Local::now().date_naive();
        let s = summarize(&self.store.plays, &self.store.days, today);
        if s.plays == 0 {
            return empty_state(
                Icon::History,
                "No listening data yet",
                "Play a few tracks and your stats will show up here.",
                None,
                p,
            );
        }
        let lead = &s.tracks[0];
        let grow = self.anim.value("stats-grow", 1.0).clamp(0.0, 1.2);

        let hero = container(
            row![
                self.art(artwork_of(&lead.track), 112.0, 20.0),
                column![
                    text("MOST PLAYED")
                        .size(12)
                        .color(crate::theme::alpha(p.on_primary_container, 0.8)),
                    text(lead.track.title.clone())
                        .size(28)
                        .wrapping(text::Wrapping::None)
                        .color(p.on_primary_container),
                    text(lead.track.artist.clone())
                        .size(14)
                        .color(crate::theme::alpha(p.on_primary_container, 0.8)),
                    text(format!(
                        "{}{}",
                        plural(lead.count, "play"),
                        if lead.seconds >= 60.0 {
                            format!(" · {} listened", hm(lead.seconds))
                        } else {
                            String::new()
                        }
                    ))
                    .size(13)
                    .color(p.on_primary_container),
                ]
                .spacing(2),
            ]
            .spacing(20)
            .align_y(Alignment::Center),
        )
        .padding(20)
        .width(Length::Fill)
        .style(styles::fill(p.primary_container, 28.0));

        let card = |value: String, label: &str, flame: bool| {
            let mut v = row![text(value)
                .size(32)
                .wrapping(text::Wrapping::None)
                .color(p.primary)]
            .spacing(6)
            .align_y(Alignment::Center);
            if flame {
                v = v.push(icon(Icon::Fire, 22.0).color(p.primary));
            }
            container(
                column![
                    v,
                    text(label.to_string()).size(14).color(p.on_surface_variant)
                ]
                .spacing(2),
            )
            .padding(20)
            .width(Length::Fill)
            .style(styles::card(p, 20.0))
        };
        let cards = row![
            card(hm(s.seconds), "listened", false),
            card(s.plays.to_string(), "plays", false),
            card(s.artist_count.to_string(), "artists", false),
            card(s.streak.to_string(), "day streak", s.streak > 1),
        ]
        .spacing(12);

        let peak = s.recent.iter().map(|d| d.seconds).fold(1.0_f64, f64::max);
        let mut bars = row![].spacing(8).height(120);
        for (i, d) in s.recent.iter().enumerate() {
            let today_col = i == s.recent.len() - 1;
            let frac = if d.seconds > 0.0 {
                (d.seconds / peak).max(0.08)
            } else {
                0.03
            } as f32;
            let color = if d.seconds == 0.0 {
                crate::theme::alpha(p.outline_variant, 0.6)
            } else if today_col {
                p.primary
            } else {
                crate::theme::alpha(p.primary, 0.55)
            };
            use chrono::Datelike;
            let letter = WEEKDAY[d.date.weekday().num_days_from_sunday() as usize];
            bars = bars.push(
                column![
                    container(
                        container(Space::new().width(Length::Fill).height(frac * 92.0 * grow))
                            .style(styles::fill(color, 8.0))
                    )
                    .height(Length::Fill)
                    .align_y(iced::alignment::Vertical::Bottom),
                    text(letter).size(11).color(if today_col {
                        p.primary
                    } else {
                        p.on_surface_variant
                    }),
                ]
                .spacing(6)
                .align_x(Alignment::Center)
                .width(Length::Fill),
            );
        }
        let activity = column![
            row![
                text("Last 2 weeks").size(16).color(p.on_surface_variant),
                Space::new().width(Length::Fill),
                text(if s.week > 0.0 {
                    format!("{} this week", hm(s.week))
                } else {
                    "Nothing this week yet".into()
                })
                .size(14)
                .color(p.on_surface_variant),
            ],
            bars,
        ]
        .spacing(12);

        let mut top_tracks =
            column![text("Top tracks").size(16).color(p.on_surface_variant)].spacing(12);
        for (i, t) in s.tracks.iter().enumerate() {
            top_tracks = top_tracks.push(rank_row(
                p,
                i,
                self.art(artwork_of(&t.track), 40.0, 10.0),
                t.track.title.clone(),
                t.track.artist.clone(),
                t.count,
            ));
        }
        let mut top_artists =
            column![text("Top artists").size(16).color(p.on_surface_variant)].spacing(12);
        for (i, a) in s.artists.iter().enumerate() {
            let sub = if a.seconds >= 60.0 {
                format!("{} listened", hm(a.seconds))
            } else {
                plural(a.count, "play")
            };
            top_artists = top_artists.push(rank_row(
                p,
                i,
                self.art(artwork_of(&a.top.track), 40.0, 20.0),
                a.name.clone(),
                sub,
                a.count,
            ));
        }
        column![
            hero,
            cards,
            activity,
            row![
                top_tracks.width(Length::Fill),
                top_artists.width(Length::Fill)
            ]
            .spacing(32),
        ]
        .spacing(28)
        .into()
    }
}

fn rank_row<'a>(
    p: Palette,
    i: usize,
    art: Element<'a, Message>,
    title: String,
    sub: String,
    n: u64,
) -> Element<'a, Message> {
    row![
        container(
            text((i + 1).to_string())
                .size(14)
                .color(p.on_surface_variant)
        )
        .width(20)
        .align_x(iced::alignment::Horizontal::Center),
        art,
        column![
            text(title).size(14).wrapping(text::Wrapping::None),
            text(sub)
                .size(12)
                .color(p.on_surface_variant)
                .wrapping(text::Wrapping::None)
        ]
        .width(Length::Fill),
        text(format!("{n}×")).size(13).color(p.on_surface_variant),
    ]
    .spacing(12)
    .align_y(Alignment::Center)
    .into()
}
