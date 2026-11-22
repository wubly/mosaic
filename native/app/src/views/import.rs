//! Port of `ImportDialog.js` and `useSpotifyImport.js`.

use std::collections::HashSet;
use std::time::Duration;

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space,
};
use iced::{Alignment, Element, Length, Task};
use mosaic_services::spotify::embed::{Imported, RemoteTrack};
use mosaic_services::spotify::import::{Outcome, EMBED_LIMIT};
use mosaic_services::spotify::matcher::Match;

use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};

const CONCURRENCY: usize = 4;
const MATCH_TIMEOUT: Duration = Duration::from_secs(30);
pub const BAD_URL: &str = "That doesn’t look like a Spotify playlist or album link.";
pub const UNREADABLE: &str =
    "Couldn’t read that playlist. If it’s private or has more than 100 tracks, connect Spotify in Settings → Beta and try again.";
pub const TRUNCATED: &str =
    "Only the first 100 tracks could be read. Connect Spotify in Settings → Beta to import playlists over 100 tracks.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Step {
    #[default]
    Input,
    Fetching,
    Matching,
    Review,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Url(String),
    Start,
    Fetched(u64, Box<Outcome>),
    Matched(u64, usize, Box<Option<Match>>),
    Name(String),
    Toggle(usize),
    Create,
    Close,
}

#[derive(Default)]
pub struct State {
    pub step: Step,
    pub url: String,
    pub error: String,
    pub data: Option<Imported>,
    /// `None` = pending, `Some(None)` = no match, `Some(Some(m))` = matched.
    pub results: Vec<Option<Option<Match>>>,
    pub skip: HashSet<usize>,
    pub name: String,
    pub run: u64,
    pub next: usize,
}

impl State {
    pub fn done(&self) -> usize {
        self.results.iter().filter(|r| r.is_some()).count()
    }

    pub fn matched(&self) -> Vec<&Match> {
        self.results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| {
                r.as_ref()
                    .and_then(Option::as_ref)
                    .filter(|_| !self.skip.contains(&i))
            })
            .collect()
    }

    pub fn missing(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r, Some(None)))
            .count()
    }
}

impl App {
    fn match_task(&self, run: u64, index: usize, track: RemoteTrack) -> Task<Message> {
        let services = self.services.clone();
        Task::perform(
            async move {
                tokio::time::timeout(MATCH_TIMEOUT, services.find_match(&track))
                    .await
                    .ok()
                    .flatten()
            },
            move |m| Message::Import(Msg::Matched(run, index, Box::new(m))),
        )
    }

    pub(crate) fn on_import(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Url(u) => {
                self.ui.import.url = u;
                Task::none()
            }
            Msg::Start => {
                let url = self.ui.import.url.trim().to_string();
                if url.is_empty() {
                    return Task::none();
                }
                self.ui.import.run += 1;
                let run = self.ui.import.run;
                self.ui.import.error.clear();
                self.ui.import.step = Step::Fetching;
                let services = self.services.clone();
                Task::perform(
                    async move { services.spotify_import(&url).await },
                    move |o| Message::Import(Msg::Fetched(run, Box::new(o))),
                )
            }
            Msg::Fetched(run, outcome) => {
                if run != self.ui.import.run {
                    return Task::none();
                }
                match *outcome {
                    Outcome::BadUrl => {
                        self.ui.import.step = Step::Input;
                        self.ui.import.error = BAD_URL.into();
                        Task::none()
                    }
                    Outcome::Unavailable => {
                        self.ui.import.step = Step::Input;
                        self.ui.import.error = UNREADABLE.into();
                        Task::none()
                    }
                    Outcome::Imported(data) => {
                        let n = data.tracks.len();
                        self.ui.import.name = data.name.clone();
                        self.ui.import.results = vec![None; n];
                        self.ui.import.skip.clear();
                        self.ui.import.step = Step::Matching;
                        let first: Vec<_> = data.tracks.iter().take(CONCURRENCY).cloned().collect();
                        self.ui.import.next = first.len();
                        self.ui.import.data = Some(data);
                        Task::batch(
                            first
                                .into_iter()
                                .enumerate()
                                .map(|(i, t)| self.match_task(run, i, t)),
                        )
                    }
                }
            }
            Msg::Matched(run, index, found) => {
                if run != self.ui.import.run {
                    return Task::none();
                }
                if let Some(slot) = self.ui.import.results.get_mut(index) {
                    *slot = Some(*found);
                }
                let next = self.ui.import.next;
                let upcoming = self
                    .ui
                    .import
                    .data
                    .as_ref()
                    .and_then(|d| d.tracks.get(next))
                    .cloned();
                match upcoming {
                    Some(t) => {
                        self.ui.import.next += 1;
                        self.match_task(run, next, t)
                    }
                    None => {
                        if self.ui.import.done() == self.ui.import.results.len() {
                            self.ui.import.step = Step::Review;
                        }
                        Task::none()
                    }
                }
            }
            Msg::Name(n) => {
                self.ui.import.name = n;
                Task::none()
            }
            Msg::Toggle(i) => {
                if !self.ui.import.skip.remove(&i) {
                    self.ui.import.skip.insert(i);
                }
                Task::none()
            }
            Msg::Create => {
                let tracks: Vec<_> = self
                    .ui
                    .import
                    .matched()
                    .into_iter()
                    .map(|m| m.track.clone())
                    .collect();
                let name = self.ui.import.name.trim().to_string();
                let id = self.store.create_playlist(&name, &tracks);
                self.touch();
                self.toast(format!(
                    "Created “{}” with {} tracks",
                    if name.is_empty() {
                        "Untitled playlist"
                    } else {
                        &name
                    },
                    tracks.len()
                ));
                self.ui.library.open = Some(id);
                self.ui.view = Some(crate::widgets::rail::View::Playlists);
                self.retarget_rail();
                self.on_import(Msg::Close)
            }
            Msg::Close => {
                self.ui.importing = false;
                self.ui.import.run += 1;
                self.ui.import = State {
                    run: self.ui.import.run,
                    ..State::default()
                };
                Task::none()
            }
        }
    }

    fn dialog_input(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.import;
        let busy = s.step == Step::Fetching;
        let mut input = text_input("Paste a Spotify playlist or album link", &s.url)
            .size(15)
            .padding([0, 20])
            .style(move |_, _| iced::widget::text_input::Style {
                background: p.surface_container_highest.into(),
                border: iced::border::rounded(26),
                icon: p.on_surface_variant,
                placeholder: p.on_surface_variant,
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            });
        if !busy {
            input = input
                .on_input(|u| Message::Import(Msg::Url(u)))
                .on_submit(Message::Import(Msg::Start));
        }
        let go = button(
            container(text(if busy { "Reading…" } else { "Import" }).size(14))
                .center_y(Length::Fill),
        )
        .height(40)
        .padding([0, 24])
        .style(styles::primary(p))
        .on_press_maybe((!busy && !s.url.trim().is_empty()).then_some(Message::Import(Msg::Start)));
        let mut col = column![row![
            container(input).height(52).center_y(52).width(Length::Fill),
            go
        ]
        .spacing(12)
        .align_y(Alignment::Center)]
        .spacing(16);
        if !s.error.is_empty() {
            col = col.push(
                container(text(s.error.clone()).size(14))
                    .padding([12, 16])
                    .width(Length::Fill)
                    .style(styles::error_box(p)),
            );
        }
        col.push(
            text("Works with public playlists of up to 100 tracks right away. Connect Spotify in Settings → Beta to import longer or private playlists.")
                .size(12)
                .color(p.on_surface_variant),
        )
        .padding(iced::Padding { top: 12.0, right: 28.0, bottom: 28.0, left: 28.0 })
        .into()
    }

    fn dialog_matching(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.import;
        let total = s.results.len();
        let done = s.done();
        let frac = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        column![
            container(icon(Icon::CloudDownload, 32.0).color(p.on_primary_container))
                .center(72)
                .style(styles::fill(p.primary_container, 24.0)),
            text(s.data.as_ref().map(|d| d.name.clone()).unwrap_or_default()).size(18),
            text(format!(
                "Finding {total} songs on SoundCloud… {done} / {total}"
            ))
            .size(14)
            .color(p.on_surface_variant),
            container(
                container(
                    Space::new()
                        .width(Length::FillPortion((frac * 1000.0) as u16 + 1))
                        .height(6)
                )
                .style(styles::fill(p.primary, 3.0))
            )
            .width(420)
            .style(styles::fill(p.outline_variant, 3.0)),
        ]
        .spacing(10)
        .align_x(Alignment::Center)
        .padding([28, 40])
        .into()
    }

    fn dialog_review(&self, p: Palette) -> Element<'_, Message> {
        let s = &self.ui.import;
        let Some(data) = &s.data else {
            return Space::new().into();
        };
        let matched = s.matched().len();
        let total = data.tracks.len();
        let missing = s.missing();
        let name = text_input("Playlist name", &s.name)
            .on_input(|n| Message::Import(Msg::Name(n)))
            .size(20)
            .padding([6, 0])
            .style(move |_, _| iced::widget::text_input::Style {
                background: iced::Color::TRANSPARENT.into(),
                border: iced::Border {
                    color: p.outline_variant,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                icon: p.on_surface_variant,
                placeholder: p.on_surface_variant,
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            });
        let mut head = column![
            name,
            text(format!(
                "{matched} of {total} songs will be added{}",
                if missing > 0 {
                    format!(" · {missing} not found")
                } else {
                    String::new()
                }
            ))
            .size(14)
            .color(p.on_surface_variant),
        ]
        .spacing(4)
        .padding(iced::Padding {
            top: 4.0,
            right: 28.0,
            bottom: 12.0,
            left: 28.0,
        });
        if data.via == "embed" && total >= EMBED_LIMIT && !self.ui.spotify.status.connected {
            head = head.push(text(TRUNCATED).size(12).color(p.on_surface_variant));
        }
        let mut list = column![].spacing(0);
        for (i, t) in data.tracks.iter().enumerate() {
            let found = s
                .results
                .get(i)
                .and_then(|r| r.as_ref())
                .and_then(Option::as_ref);
            let off = found.is_none() || s.skip.contains(&i);
            let dim = if off { 0.45 } else { 1.0 };
            let check: Element<'_, Message> = match found {
                Some(_) => button(
                    container(
                        icon(
                            if off {
                                Icon::CheckBoxBlank
                            } else {
                                Icon::CheckBox
                            },
                            22.0,
                        )
                        .color(p.primary),
                    )
                    .center(Length::Fill),
                )
                .width(36)
                .height(36)
                .padding(0)
                .style(styles::icon_button(p, false))
                .on_press(Message::Import(Msg::Toggle(i)))
                .into(),
                None => container(icon(Icon::SearchOff, 22.0).color(p.on_surface_variant))
                    .center(36)
                    .into(),
            };
            let col = |title: String, sub: String| {
                column![
                    text(title)
                        .size(14)
                        .wrapping(text::Wrapping::None)
                        .color(crate::theme::alpha(p.on_surface, dim)),
                    text(sub)
                        .size(12)
                        .wrapping(text::Wrapping::None)
                        .color(crate::theme::alpha(p.on_surface_variant, dim)),
                ]
                .width(Length::FillPortion(1))
            };
            let right = match found {
                Some(m) => col(
                    m.track.title.clone(),
                    format!("{} · {}% match", m.track.artist, (m.score * 100.0).round()),
                ),
                None => column![text("No match on SoundCloud")
                    .size(12)
                    .color(p.on_surface_variant)]
                .width(Length::FillPortion(1)),
            };
            list = list.push(
                container(
                    row![
                        check,
                        col(t.title.clone(), t.artist.clone()),
                        icon(Icon::ArrowForward, 16.0).color(p.on_surface_variant),
                        right
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .padding([8, 12]),
            );
        }
        let foot = row![
            button(container(text("Cancel").size(14).color(p.primary)).center_y(Length::Fill))
                .height(40)
                .padding([0, 12])
                .style(styles::ghost(p))
                .on_press(Message::Import(Msg::Close)),
            Space::new().width(Length::Fill),
            button(
                container(text(format!("Create playlist · {matched}")).size(14))
                    .center_y(Length::Fill)
            )
            .height(40)
            .padding([0, 24])
            .style(styles::primary(p))
            .on_press_maybe((matched > 0).then_some(Message::Import(Msg::Create))),
        ]
        .align_y(Alignment::Center)
        .padding([16, 24]);
        column![
            head,
            scrollable(container(list).padding([0, 16]))
                .style(styles::scrollbar(p))
                .height(Length::Fill),
            foot
        ]
        .into()
    }

    pub(crate) fn import_view(&self, p: Palette) -> Element<'_, Message> {
        let body: Element<'_, Message> = match self.ui.import.step {
            Step::Input | Step::Fetching => self.dialog_input(p),
            Step::Matching => self.dialog_matching(p),
            Step::Review => self.dialog_review(p),
        };
        let head = row![
            column![
                text("Import from Spotify").size(24),
                text("Bring a playlist or album over. Songs are matched on SoundCloud.")
                    .size(14)
                    .color(p.on_surface_variant),
            ]
            .width(Length::Fill),
            super::common::icon_button(Icon::Close, "Close (Esc)", p, Message::Import(Msg::Close)),
        ]
        .align_y(Alignment::Start)
        .padding(iced::Padding {
            top: 24.0,
            right: 24.0,
            bottom: 8.0,
            left: 28.0,
        });
        let height = (self.size.height * 0.78).min(720.0);
        let dialog = container(column![head, body])
            .width(720.0_f32.min(self.size.width * 0.94))
            .height(if self.ui.import.step == Step::Review {
                Length::Fixed(height)
            } else {
                Length::Shrink
            })
            .style(styles::popup(p, 28.0));
        stack![
            mouse_area(
                container(Space::new())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .style(styles::fill(iced::Color::from_rgba8(0, 0, 0, 0.6), 0.0))
            )
            .on_press(Message::Import(Msg::Close)),
            container(mouse_area(dialog).on_press(Message::Nop)).center(Length::Fill)
        ]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(i: u64) -> Match {
        Match {
            track: serde_json::from_value(
                serde_json::json!({"id": i, "source": "soundcloud", "title": "t", "artist": "a"}),
            )
            .unwrap(),
            score: 0.9,
        }
    }

    #[test]
    fn counts_follow_results_and_skips() {
        let mut s = State {
            results: vec![Some(Some(m(1))), Some(None), None, Some(Some(m(2)))],
            ..State::default()
        };
        assert_eq!((s.done(), s.missing(), s.matched().len()), (3, 1, 2));
        s.skip.insert(0);
        assert_eq!(s.matched().len(), 1);
    }
}
