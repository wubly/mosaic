//! Port of `LocalView.js`.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::Track;

use super::artwork_of;
use super::common::{icon_button, page, primary_button, tonal_button, virtual_rows, visible_range};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};

const LIST_TOP: f32 = 400.0;
const UNKNOWN: &str = "Unknown artist";
const AUDIO_EXT: [&str; 9] = [
    "mp3", "flac", "m4a", "aac", "wav", "ogg", "opus", "oga", "wma",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Sort {
    #[default]
    Artist,
    Folder,
    Title,
    Album,
    Length,
}

impl Sort {
    pub const ALL: [(Sort, &'static str); 5] = [
        (Sort::Artist, "Artist"),
        (Sort::Folder, "Folder"),
        (Sort::Title, "Title"),
        (Sort::Album, "Album"),
        (Sort::Length, "Length"),
    ];
}

#[derive(Clone, Debug)]
pub enum Row {
    Header { name: String, tracks: Vec<usize> },
    Track { index: usize, n: usize },
}

#[derive(Default)]
pub struct State {
    pub filter: String,
    pub sort: Sort,
    pub tracks: Vec<Track>,
    pub rows: Vec<Row>,
    pub scanning: bool,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Filter(String),
    Sort(Sort),
    AddFolder,
    AddFiles,
    Picked(Vec<String>),
    Remove(String),
    Rescan,
    Play,
    Shuffle,
    PlayGroup(usize),
}

fn key(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    s.nfkd()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .collect::<String>()
        .to_lowercase()
}

fn base(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

pub fn sort_tracks(tracks: &mut [Track], sort: Sort) {
    let unknown = |t: &Track| t.artist == UNKNOWN;
    let folder = |t: &Track| key(t.folder.as_deref().unwrap_or(""));
    match sort {
        Sort::Artist => tracks
            .sort_by_cached_key(|t| (unknown(t), key(&t.artist), key(&t.album), key(&t.title))),
        Sort::Folder => {
            tracks.sort_by_cached_key(|t| (folder(t), key(&t.artist), key(&t.album), key(&t.title)))
        }
        Sort::Title => tracks.sort_by_cached_key(|t| key(&t.title)),
        Sort::Album => tracks.sort_by_cached_key(|t| (key(&t.album), key(&t.title))),
        Sort::Length => tracks.sort_by_key(|t| std::cmp::Reverse(t.duration)),
    }
}

/// Filter, sort and (for artist/folder) group headers, like the `useMemo`s in `LocalView.js`.
pub fn build(all: &[Track], filter: &str, sort: Sort) -> (Vec<Track>, Vec<Row>) {
    let q = filter.trim().to_lowercase();
    let mut tracks: Vec<Track> = all
        .iter()
        .filter(|t| {
            q.is_empty()
                || format!(
                    "{} {} {} {}",
                    t.title,
                    t.artist,
                    t.album,
                    t.folder.as_deref().unwrap_or("")
                )
                .to_lowercase()
                .contains(&q)
        })
        .cloned()
        .collect();
    sort_tracks(&mut tracks, sort);
    let group: Option<fn(&Track) -> String> = match sort {
        Sort::Artist => Some(|t| t.artist.clone()),
        Sort::Folder => Some(|t| {
            t.folder
                .clone()
                .filter(|f| !f.is_empty())
                .unwrap_or_else(|| "Other".into())
        }),
        _ => None,
    };
    let mut rows = Vec::with_capacity(tracks.len() + 64);
    match group {
        None => rows.extend((0..tracks.len()).map(|i| Row::Track { index: i, n: i })),
        Some(key_of) => {
            let mut current: Option<(String, usize)> = None;
            for (i, t) in tracks.iter().enumerate() {
                let name = key_of(t);
                if current.as_ref().is_none_or(|(n, _)| *n != name) {
                    rows.push(Row::Header {
                        name: name.clone(),
                        tracks: Vec::new(),
                    });
                    current = Some((name, rows.len() - 1));
                }
                let h = current.as_ref().map_or(0, |(_, h)| *h);
                let mut n = 0;
                if let Row::Header {
                    tracks: members, ..
                } = &mut rows[h]
                {
                    n = members.len();
                    members.push(i);
                }
                rows.push(Row::Track { index: i, n });
            }
        }
    }
    (tracks, rows)
}

impl App {
    pub(crate) fn rebuild_local(&mut self) {
        let (tracks, rows) = build(&self.local, &self.ui.local.filter, self.ui.local.sort);
        self.ui.local.tracks = tracks;
        self.ui.local.rows = rows;
    }

    pub(crate) fn add_sources(&mut self, paths: Vec<String>) -> Task<Message> {
        if paths.is_empty() {
            return Task::none();
        }
        self.store.add_sources(&paths);
        self.touch();
        self.ui.local.scanning = true;
        self.scan()
    }

    pub(crate) fn on_local(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Filter(f) => {
                self.ui.local.filter = f;
                self.rebuild_local();
                Task::none()
            }
            Msg::Sort(s) => {
                self.ui.local.sort = s;
                self.rebuild_local();
                Task::none()
            }
            Msg::AddFolder => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .pick_folders()
                        .await
                        .map(|h| {
                            h.into_iter()
                                .map(|f| f.path().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default()
                },
                |paths| Message::Local(Msg::Picked(paths)),
            ),
            Msg::AddFiles => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("Audio", &AUDIO_EXT)
                        .pick_files()
                        .await
                        .map(|h| {
                            h.into_iter()
                                .map(|f| f.path().to_string_lossy().into_owned())
                                .collect()
                        })
                        .unwrap_or_default()
                },
                |paths| Message::Local(Msg::Picked(paths)),
            ),
            Msg::Picked(paths) => self.add_sources(paths),
            Msg::Remove(path) => {
                self.store.remove_source(&path);
                self.touch();
                self.ui.local.scanning = true;
                self.scan()
            }
            Msg::Rescan => {
                self.ui.local.scanning = true;
                self.scan()
            }
            Msg::Play => match self.ui.local.tracks.first().cloned() {
                Some(first) => {
                    let list = self.ui.local.tracks.clone();
                    self.player_input(PlayerInput::Play(first, Some(list)))
                }
                None => Task::none(),
            },
            Msg::Shuffle => {
                let list = self.ui.local.tracks.clone();
                if list.is_empty() {
                    return Task::none();
                }
                let mut tasks = Vec::new();
                if !self.settings().shuffle {
                    tasks.push(self.player_input(PlayerInput::ToggleShuffle));
                }
                let pick = list[rand::random_range(0..list.len())].clone();
                tasks.push(self.player_input(PlayerInput::Play(pick, Some(list))));
                Task::batch(tasks)
            }
            Msg::PlayGroup(row) => {
                let first = match self.ui.local.rows.get(row) {
                    Some(Row::Header { tracks, .. }) => tracks.first().copied(),
                    _ => None,
                };
                match first.and_then(|i| self.ui.local.tracks.get(i).cloned()) {
                    Some(t) => {
                        let list = self.ui.local.tracks.clone();
                        self.player_input(PlayerInput::Play(t, Some(list)))
                    }
                    None => Task::none(),
                }
            }
        }
    }

    fn group_header<'a>(
        &'a self,
        row_index: usize,
        name: &str,
        members: &[usize],
        p: Palette,
    ) -> Element<'a, Message> {
        let cover = members
            .iter()
            .filter_map(|i| self.ui.local.tracks.get(*i))
            .map(artwork_of)
            .find(|s| !s.is_empty())
            .unwrap_or("");
        let avatar: Element<'a, Message> = if cover.is_empty() {
            container(
                text(
                    name.chars()
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default(),
                )
                .size(16)
                .color(p.on_primary_container),
            )
            .center(36)
            .style(styles::fill(p.primary_container, 18.0))
            .into()
        } else {
            self.art(cover, 36.0, 18.0)
        };
        row![
            avatar,
            column![
                text(name.to_string())
                    .size(14)
                    .wrapping(text::Wrapping::None),
                text(format!(
                    "{} {}",
                    members.len(),
                    if members.len() == 1 {
                        "track"
                    } else {
                        "tracks"
                    }
                ))
                .size(12)
                .color(p.on_surface_variant)
            ]
            .width(Length::Fill),
            icon_button(
                Icon::PlayArrow,
                &format!("Play {name}"),
                p,
                Message::Local(Msg::PlayGroup(row_index))
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .padding([0, 8])
        .into()
    }

    pub(crate) fn local_view(&self, p: Palette) -> Element<'_, Message> {
        let l = &self.ui.local;
        let sources = &self.store.sources;
        let subtitle = if l.scanning {
            "Scanning…".to_string()
        } else {
            format!("{} tracks from {} sources", self.local.len(), sources.len())
        };
        let mut actions = vec![
            primary_button(
                "Add folder",
                Icon::CreateNewFolder,
                p,
                Message::Local(Msg::AddFolder),
            ),
            tonal_button(
                "Add files",
                Icon::AudioFile,
                p,
                Message::Local(Msg::AddFiles),
            ),
        ];
        if !sources.is_empty() {
            actions.push(icon_button(
                Icon::Refresh,
                "Rescan",
                p,
                Message::Local(Msg::Rescan),
            ));
        }
        let mut body =
            column![self.page_header("On your device".into(), Some(subtitle), actions, p)];
        if !sources.is_empty() {
            let chips = iced::widget::Row::with_children(sources.iter().map(|s| {
                container(
                    row![
                        icon(Icon::Folder, 16.0).color(p.on_surface_variant),
                        text(base(s).to_string())
                            .size(14)
                            .color(p.on_surface_variant),
                        button(container(icon(Icon::Close, 14.0)).center(Length::Fill))
                            .width(22)
                            .height(22)
                            .padding(0)
                            .style(styles::icon_button(p, false))
                            .on_press(Message::Local(Msg::Remove(s.clone()))),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding(iced::Padding {
                    top: 0.0,
                    right: 6.0,
                    bottom: 0.0,
                    left: 12.0,
                })
                .center_y(32)
                .max_width(260)
                .style(move |_| container::Style {
                    border: iced::Border {
                        color: p.outline_variant,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    ..container::Style::default()
                })
                .into()
            }))
            .spacing(8)
            .wrap()
            .vertical_spacing(8);
            body = body.push(container(chips).padding(iced::Padding {
                bottom: 20.0,
                ..iced::Padding::ZERO
            }));
        }

        if self.local.is_empty() && !l.scanning {
            body = body.push(
                container(
                    column![
                        icon(Icon::LibraryMusic, 44.0).color(p.primary),
                        text("Bring your own music").size(20),
                        text("Add a folder, pick files, or just drop them anywhere in this window.").size(14).color(p.on_surface_variant),
                        text("MP3, FLAC, M4A, WAV, OGG, OPUS · covers and tags are read automatically · a matching .lrc file gives you your own synced lyrics.")
                            .size(14)
                            .color(p.on_surface_variant),
                    ]
                    .spacing(10)
                    .align_x(Alignment::Center),
                )
                .padding([64, 32])
                .width(Length::Fill)
                .center_x(Length::Fill)
                .style(move |_| container::Style {
                    border: iced::Border { color: p.outline_variant, width: 2.0, radius: 28.0.into() },
                    ..container::Style::default()
                }),
            );
            return page(body, p);
        }

        let filter = text_input("Filter your music", &l.filter)
            .on_input(|s| Message::Local(Msg::Filter(s)))
            .size(14)
            .padding(0)
            .style(move |_, _| iced::widget::text_input::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border::default(),
                icon: p.on_surface_variant,
                placeholder: p.on_surface_variant,
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            });
        body = body.push(
            row![
                container(
                    row![icon(Icon::Search, 20.0).color(p.on_surface_variant), filter]
                        .spacing(10)
                        .align_y(Alignment::Center)
                )
                .padding([0, 12])
                .height(40)
                .width(240)
                .center_y(40)
                .style(styles::fill(p.surface_container, 20.0)),
                Space::new().width(Length::Fill),
                tonal_button("Play", Icon::PlayArrow, p, Message::Local(Msg::Play)),
                tonal_button("Shuffle", Icon::Shuffle, p, Message::Local(Msg::Shuffle)),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .padding(iced::Padding {
                bottom: 12.0,
                ..iced::Padding::ZERO
            }),
        );
        let mut sorts = row![text("Sort by").size(12).color(p.on_surface_variant)]
            .spacing(8)
            .align_y(Alignment::Center);
        for (s, label) in Sort::ALL {
            sorts = sorts.push(
                button(text(label).size(13))
                    .height(32)
                    .padding([0, 14])
                    .style(styles::chip(p, l.sort == s))
                    .on_press(Message::Local(Msg::Sort(s))),
            );
        }
        body = body.push(container(sorts).padding(iced::Padding {
            bottom: 16.0,
            ..iced::Padding::ZERO
        }));
        let range = visible_range(l.rows.len(), self.ui.scroll.0, self.ui.scroll.1, LIST_TOP);
        body = body.push(virtual_rows(l.rows.len(), range, |i| match &l.rows[i] {
            Row::Header { name, tracks } => self.group_header(i, name, tracks, p),
            Row::Track { index, n } => self.track_row(
                &l.tracks[*index],
                *n,
                TrackRef {
                    list: ListRef::Local,
                    index: *index,
                },
                true,
                p,
            ),
        }));
        page(body, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: u64, title: &str, artist: &str, album: &str, folder: &str, dur: u64) -> Track {
        serde_json::from_value(serde_json::json!({
            "id": format!("local:{id}"), "source": "local", "title": title, "artist": artist,
            "album": album, "folder": folder, "duration": dur,
        }))
        .unwrap()
    }

    fn lib() -> Vec<Track> {
        vec![
            t(1, "Zebra", "Beta", "B1", "/m/b", 100),
            t(2, "Alpha", "alpha", "A1", "/m/a", 300),
            t(3, "Apple", "Alpha", "A1", "/m/a", 200),
            t(4, "Mystery", UNKNOWN, "", "/m/x", 50),
            t(5, "Élan", "Alpha", "A0", "/m/a", 10),
        ]
    }

    #[test]
    fn artist_sort_is_accent_and_case_insensitive_with_unknown_last() {
        let (tracks, rows) = build(&lib(), "", Sort::Artist);
        let titles: Vec<_> = tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["Élan", "Alpha", "Apple", "Zebra", "Mystery"]);
        let headers: Vec<_> = rows
            .iter()
            .filter_map(|r| match r {
                Row::Header { name, tracks } => Some((name.as_str(), tracks.len())),
                _ => None,
            })
            .collect();
        assert_eq!(
            headers,
            [
                ("Alpha", 1),
                ("alpha", 1),
                ("Alpha", 1),
                ("Beta", 1),
                (UNKNOWN, 1)
            ]
        );
    }

    #[test]
    fn filter_matches_across_fields() {
        let (tracks, _) = build(&lib(), "/m/x", Sort::Title);
        assert_eq!(tracks.len(), 1);
        let (tracks, rows) = build(&lib(), "alpha", Sort::Title);
        assert_eq!(tracks.len(), 3);
        assert_eq!(rows.len(), 3, "title sort has no headers");
    }

    #[test]
    fn length_sorts_longest_first_and_folder_groups() {
        let (tracks, _) = build(&lib(), "", Sort::Length);
        assert_eq!(tracks[0].title, "Alpha");
        let (_, rows) = build(&lib(), "", Sort::Folder);
        let headers = rows
            .iter()
            .filter(|r| matches!(r, Row::Header { .. }))
            .count();
        assert_eq!(headers, 3);
    }

    #[test]
    fn base_takes_the_last_path_component() {
        assert_eq!(base("/home/u/Music/"), "Music");
        assert_eq!(base("C:\\Users\\u\\Music"), "Music");
    }
}
