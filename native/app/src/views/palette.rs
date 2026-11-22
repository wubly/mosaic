//! Port of `CommandPalette.js` and `paletteActions.js`.

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space,
};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::Track;

use crate::app::{App, Message};
use crate::player::{PlayerInput, SleepRequest};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::rail::View;

pub const INPUT_ID: &str = "palette-input";
const MAX_ITEMS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Go(View),
    ImportPlaylist,
    NowPlaying,
    Mini,
    Lyrics,
    Shuffle,
    Like,
    Eq,
    Sleep30,
}

#[derive(Clone, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Item {
    Track(Track),
    Action(Action, String, Icon),
    Search(String),
}

#[derive(Clone, Debug)]
pub enum Msg {
    Query(String),
    Move(isize),
    Hover(usize),
    Run(usize),
    Submit,
    Close,
}

#[derive(Default)]
pub struct State {
    pub query: String,
    pub selected: usize,
}

/// `paletteActions.js`, in the same order.
pub fn actions(eq_on: bool) -> Vec<(Action, String, Icon)> {
    vec![
        (
            Action::Go(View::Discover),
            "Go to Discover".into(),
            Icon::Explore,
        ),
        (
            Action::Go(View::Library),
            "Go to Library".into(),
            Icon::LibraryMusic,
        ),
        (
            Action::Go(View::Playlists),
            "Go to Playlists".into(),
            Icon::PlaylistPlay,
        ),
        (
            Action::ImportPlaylist,
            "Import a Spotify playlist".into(),
            Icon::CloudDownload,
        ),
        (
            Action::Go(View::Local),
            "Go to On your device".into(),
            Icon::Folder,
        ),
        (
            Action::Go(View::Queue),
            "Go to Queue".into(),
            Icon::QueueMusic,
        ),
        (
            Action::Go(View::Settings),
            "Open Settings".into(),
            Icon::Settings,
        ),
        (
            Action::NowPlaying,
            "Open immersive Now Playing".into(),
            Icon::Fullscreen,
        ),
        (
            Action::Mini,
            "Toggle mini player".into(),
            Icon::PictureInPicture,
        ),
        (Action::Lyrics, "Toggle lyrics panel".into(), Icon::Lyrics),
        (Action::Shuffle, "Toggle shuffle".into(), Icon::Shuffle),
        (Action::Like, "Like current track".into(), Icon::Favorite),
        (
            Action::Eq,
            if eq_on {
                "Turn equalizer off"
            } else {
                "Turn equalizer on"
            }
            .into(),
            Icon::GraphicEq,
        ),
        (
            Action::Sleep30,
            "Sleep timer: 30 minutes".into(),
            Icon::Bedtime,
        ),
    ]
}

/// Tracks first, then matching actions, then a web search entry; at most ten.
pub fn build_items(
    query: &str,
    tracks: &[Track],
    actions: Vec<(Action, String, Icon)>,
) -> Vec<Item> {
    let s = query.trim().to_lowercase();
    let acts = actions
        .into_iter()
        .filter(|(_, label, _)| s.is_empty() || label.to_lowercase().contains(&s))
        .map(|(a, l, i)| Item::Action(a, l, i));
    let mut seen = std::collections::HashSet::new();
    let trk: Vec<Item> = if s.chars().count() > 1 {
        tracks
            .iter()
            .filter(|t| {
                format!("{} {}", t.title, t.artist)
                    .to_lowercase()
                    .contains(&s)
                    && seen.insert(t.id.clone())
            })
            .take(6)
            .cloned()
            .map(Item::Track)
            .collect()
    } else {
        Vec::new()
    };
    let web = (!s.is_empty()).then(|| Item::Search(query.trim().to_string()));
    trk.into_iter()
        .chain(acts)
        .chain(web)
        .take(MAX_ITEMS)
        .collect()
}

impl App {
    pub(crate) fn palette_items(&self) -> Vec<Item> {
        let mut tracks: Vec<Track> = self.player.queue.list.clone();
        tracks.extend(self.store.likes.iter().cloned());
        tracks.extend(self.store.history.iter().map(|h| h.track.clone()));
        tracks.extend(self.local.iter().cloned());
        build_items(
            &self.ui.palette.query,
            &tracks,
            actions(self.settings().eq_on),
        )
    }

    pub(crate) fn open_palette(&mut self) -> Task<Message> {
        self.ui.palette_open = !self.ui.palette_open;
        if self.ui.palette_open {
            self.ui.palette = State::default();
            return iced::widget::operation::focus(INPUT_ID);
        }
        Task::none()
    }

    pub(crate) fn on_palette(&mut self, msg: Msg) -> Task<Message> {
        let items = self.palette_items();
        match msg {
            Msg::Query(q) => {
                self.ui.palette = State {
                    query: q,
                    selected: 0,
                };
                Task::none()
            }
            Msg::Move(delta) => {
                let n = items.len().max(1) as isize;
                self.ui.palette.selected =
                    (self.ui.palette.selected as isize + delta).rem_euclid(n) as usize;
                Task::none()
            }
            Msg::Hover(i) => {
                self.ui.palette.selected = i;
                Task::none()
            }
            Msg::Submit => {
                let i = self.ui.palette.selected;
                self.on_palette(Msg::Run(i))
            }
            Msg::Close => {
                self.ui.palette_open = false;
                Task::none()
            }
            Msg::Run(i) => {
                self.ui.palette_open = false;
                match items.into_iter().nth(i) {
                    Some(Item::Track(t)) => self.player_input(PlayerInput::Play(t, None)),
                    Some(Item::Search(q)) => self.start_search(q),
                    Some(Item::Action(a, ..)) => self.run_action(a),
                    None => Task::none(),
                }
            }
        }
    }

    fn run_action(&mut self, action: Action) -> Task<Message> {
        match action {
            Action::Go(v) => {
                self.ui.view = Some(v);
                self.retarget_rail();
                Task::none()
            }
            Action::ImportPlaylist => {
                self.ui.importing = true;
                Task::none()
            }
            Action::NowPlaying => {
                if self.player.track.is_some() {
                    self.ui.now_playing = true;
                }
                Task::none()
            }
            Action::Mini => self.update(Message::ToggleMini),
            Action::Lyrics => self.update(Message::ToggleLyrics),
            Action::Shuffle => self.player_input(PlayerInput::ToggleShuffle),
            Action::Like => self.toggle_like(),
            Action::Eq => {
                let on = self.settings().eq_on;
                self.set_setting(|s| s.eq_on = !on)
            }
            Action::Sleep30 => self.player_input(PlayerInput::SetSleep(SleepRequest::Minutes(30))),
        }
    }

    pub(crate) fn palette_view(&self, p: Palette) -> Element<'_, Message> {
        let items = self.palette_items();
        let sel = self.ui.palette.selected;
        let input = text_input("Type a command or search…", &self.ui.palette.query)
            .id(INPUT_ID)
            .on_input(|q| Message::Palette(Msg::Query(q)))
            .on_submit(Message::Palette(Msg::Submit))
            .size(17)
            .padding(0)
            .style(move |_, _| iced::widget::text_input::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border::default(),
                icon: p.on_surface_variant,
                placeholder: p.on_surface_variant,
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            });
        let head = container(
            row![
                icon(Icon::Search, 22.0).color(p.on_surface_variant),
                input,
                container(text("Esc").size(12))
                    .padding([1, 7])
                    .style(styles::kbd(p)),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
        )
        .padding([0, 20])
        .height(64)
        .center_y(64);
        let divider = container(Space::new().height(1))
            .width(Length::Fill)
            .style(styles::fill(p.outline_variant, 0.0));

        let mut list = column![].spacing(0);
        for (i, item) in items.iter().enumerate() {
            let (glyph, label, hint): (Icon, String, Option<String>) = match item {
                Item::Track(t) => (Icon::PlayArrow, t.title.clone(), Some(t.artist.clone())),
                Item::Search(q) => (Icon::Search, format!("Search SoundCloud for “{q}”"), None),
                Item::Action(_, l, ic) => (*ic, l.clone(), None),
            };
            let on = i == sel;
            let fg = if on {
                p.on_primary_container
            } else {
                p.on_surface
            };
            let mut line = row![
                icon(glyph, 20.0).color(fg),
                text(label)
                    .size(14)
                    .color(fg)
                    .width(Length::Fill)
                    .wrapping(text::Wrapping::None)
            ]
            .spacing(14)
            .align_y(Alignment::Center);
            if let Some(h) = hint {
                line = line.push(
                    container(
                        text(h)
                            .size(12)
                            .color(crate::theme::alpha(fg, 0.7))
                            .wrapping(text::Wrapping::None),
                    )
                    .max_width(200)
                    .clip(true),
                );
            }
            list = list.push(
                mouse_area(
                    button(line)
                        .width(Length::Fill)
                        .height(48)
                        .padding([0, 12])
                        .style(move |_, _| button::Style {
                            background: on.then(|| p.primary_container.into()),
                            text_color: fg,
                            border: iced::border::rounded(14),
                            ..button::Style::default()
                        })
                        .on_press(Message::Palette(Msg::Run(i))),
                )
                .on_enter(Message::Palette(Msg::Hover(i))),
            );
        }
        if items.is_empty() {
            list = list.push(
                container(text("No matches").color(p.on_surface_variant))
                    .padding(16)
                    .center_x(Length::Fill),
            );
        }
        let panel = container(column![
            head,
            divider,
            scrollable(container(list).padding(8)).height(Length::Shrink)
        ])
        .width(620)
        .style(styles::popup(p, 28.0));
        stack![
            mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::fill(iced::Color::from_rgba8(0, 0, 0, 0.6), 0.0))
            )
            .on_press(Message::Palette(Msg::Close)),
            container(mouse_area(panel).on_press(Message::Nop))
                .width(Length::Fill)
                .center_x(Length::Fill)
                .padding(iced::Padding {
                    top: self.size.height * 0.14,
                    ..iced::Padding::ZERO
                })
        ]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: u64, title: &str, artist: &str) -> Track {
        serde_json::from_value(
            serde_json::json!({"id": id, "source": "local", "title": title, "artist": artist}),
        )
        .unwrap()
    }

    #[test]
    fn empty_query_lists_actions_only() {
        let items = build_items("", &[t(1, "Song", "A")], actions(false));
        assert_eq!(items.len(), 10);
        assert!(items.iter().all(|i| matches!(i, Item::Action(..))));
    }

    #[test]
    fn tracks_come_first_deduped_and_a_search_entry_ends_the_list() {
        let tracks = [
            t(1, "Midnight City", "M83"),
            t(1, "Midnight City", "M83"),
            t(2, "City Lights", "X"),
        ];
        let items = build_items("city", &tracks, actions(false));
        assert!(matches!(&items[0], Item::Track(x) if x.id.key() == "1"));
        assert!(matches!(&items[1], Item::Track(x) if x.id.key() == "2"));
        assert!(matches!(items.last(), Some(Item::Search(q)) if q == "city"));
        assert_eq!(
            items.iter().filter(|i| matches!(i, Item::Track(_))).count(),
            2
        );
    }

    #[test]
    fn one_character_queries_skip_track_search() {
        let items = build_items("m", &[t(1, "Midnight", "M83")], actions(false));
        assert!(items.iter().all(|i| !matches!(i, Item::Track(_))));
    }

    #[test]
    fn eq_label_follows_state() {
        assert!(actions(true)
            .iter()
            .any(|(_, l, _)| l == "Turn equalizer off"));
        assert!(actions(false)
            .iter()
            .any(|(_, l, _)| l == "Turn equalizer on"));
    }
}
