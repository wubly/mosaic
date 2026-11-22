//! Port of `Onboarding.js`, its steps and `MosaicArt.js`.

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Color, Element, Length, Task};
use mosaic_core::model::Track;

use super::common::{icon_button, primary_button, tonal_button};
use super::import::Step as ImportStep;
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::logo;
use crate::widgets::switch::switch;

pub const STEPS: usize = 5;
const ORDER: [usize; 9] = [0, 1, 2, 5, 8, 7, 6, 3, 4];
const LIT: [usize; 5] = [3, 5, 7, 8, 9];
const TONES: [Option<u32>; 9] = [
    Some(0xeaddff),
    Some(0xd0bcff),
    Some(0xb69df8),
    Some(0xd0bcff),
    None,
    Some(0x9a82db),
    Some(0xb69df8),
    Some(0x9a82db),
    Some(0x7f67be),
];

#[derive(Clone, Debug)]
pub struct Imported {
    pub name: String,
    pub total: usize,
    pub tracks: Vec<Track>,
}

#[derive(Debug)]
pub struct State {
    pub step: usize,
    pub name: String,
    pub folders: Vec<String>,
    pub imports: Vec<Imported>,
    pub presence: bool,
    pub lyrics: bool,
    pub busy: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            step: 0,
            name: String::new(),
            folders: Vec::new(),
            imports: Vec::new(),
            presence: true,
            lyrics: true,
            busy: false,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Next,
    Back,
    Name(String),
    PickFolder,
    Picked(Vec<String>),
    RemoveFolder(usize),
    RemoveImport(usize),
    Presence(bool),
    Lyrics(bool),
    Finish,
}

/// Which of the nine tiles are lit on `step` (`MosaicArt.js`).
pub fn lit_tiles(step: usize) -> [bool; 9] {
    let lit = LIT[step.min(STEPS - 1)];
    let mut on = [false; 9];
    for (i, flag) in on.iter_mut().enumerate() {
        *flag = ORDER
            .iter()
            .position(|o| *o == i)
            .is_some_and(|rank| rank < lit);
    }
    on
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .unwrap_or(path)
}

impl App {
    pub(crate) fn begin_onboarding(&mut self) {
        self.ui.onboarding = Some(State::default());
        self.ui.import = Default::default();
    }

    /// Called by the import flow when matching finishes during onboarding.
    pub(crate) fn onboarding_collect_import(&mut self) {
        let Some(data) = self.ui.import.data.take() else {
            return;
        };
        let tracks: Vec<Track> = self
            .ui
            .import
            .results
            .iter()
            .filter_map(|r| r.as_ref().and_then(Option::as_ref))
            .map(|m| m.track.clone())
            .collect();
        if let Some(o) = &mut self.ui.onboarding {
            o.imports.push(Imported {
                name: data.name,
                total: data.tracks.len(),
                tracks,
            });
        }
        self.ui.import = Default::default();
    }

    pub(crate) fn on_onboarding(&mut self, msg: Msg) -> Task<Message> {
        let Some(o) = self.ui.onboarding.as_mut() else {
            return Task::none();
        };
        match msg {
            Msg::Next => {
                o.step = (o.step + 1).min(STEPS - 1);
                Task::none()
            }
            Msg::Back => {
                o.step = o.step.saturating_sub(1);
                self.ui.import.run += 1;
                self.ui.import = crate::views::import::State {
                    run: self.ui.import.run,
                    ..Default::default()
                };
                Task::none()
            }
            Msg::Name(n) => {
                o.name = n;
                Task::none()
            }
            Msg::PickFolder => Task::perform(
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
                |p| Message::Onboarding(Msg::Picked(p)),
            ),
            Msg::Picked(paths) => {
                for p in paths {
                    if !o.folders.contains(&p) {
                        o.folders.push(p);
                    }
                }
                Task::none()
            }
            Msg::RemoveFolder(i) => {
                if i < o.folders.len() {
                    o.folders.remove(i);
                }
                Task::none()
            }
            Msg::RemoveImport(i) => {
                if i < o.imports.len() {
                    o.imports.remove(i);
                }
                Task::none()
            }
            Msg::Presence(v) => {
                o.presence = v;
                Task::none()
            }
            Msg::Lyrics(v) => {
                o.lyrics = v;
                Task::none()
            }
            Msg::Finish => {
                o.busy = true;
                let done = self.ui.onboarding.take().unwrap_or_default();
                let name = done.name.trim().to_string();
                self.services.update_config(|c| {
                    if !name.is_empty() {
                        c.name = Some(name.clone());
                    }
                    c.onboarded = Some(true);
                });
                self.store.add_sources(&done.folders);
                for imp in done.imports.iter().filter(|i| !i.tracks.is_empty()) {
                    self.store.create_playlist(&imp.name, &imp.tracks);
                }
                self.touch();
                let (presence, lyrics) = (done.presence, done.lyrics);
                let settings = self.set_setting(move |s| {
                    s.presence = presence;
                    s.lyrics_open = lyrics;
                });
                self.ui.local.scanning = !done.folders.is_empty();
                Task::batch([settings, self.scan()])
            }
        }
    }

    fn mosaic_art(&self, step: usize, p: Palette) -> Element<'_, Message> {
        let on = lit_tiles(step);
        let tile = |i: usize| -> Element<'_, Message> {
            let rgb = TONES[i].unwrap_or(0xd0bcff);
            let base = Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
            let color = if on[i] {
                base
            } else {
                crate::theme::alpha(base, 0.14)
            };
            let inner: Element<'_, Message> = if TONES[i].is_none() {
                icon(Icon::PlayArrow, 38.0)
                    .color(Color::from_rgb8(0x38, 0x1e, 0x72))
                    .into()
            } else {
                Space::new().into()
            };
            container(inner)
                .center(96)
                .style(styles::fill(color, 26.0))
                .into()
        };
        let grid = column((0..3).map(|r| row((0..3).map(|c| tile(r * 3 + c))).spacing(12).into()))
            .spacing(12);
        container(grid)
            .center(Length::Fill)
            .style(styles::fill(p.surface_container, 0.0))
            .into()
    }

    fn step_actions<'a>(
        &self,
        back: bool,
        next_label: &str,
        next: Option<Message>,
        p: Palette,
    ) -> Element<'a, Message> {
        let mut r = row![].align_y(Alignment::Center);
        if back {
            r = r.push(
                button(
                    row![
                        icon(Icon::ArrowBack, 18.0).color(p.primary),
                        text("Back").size(14).color(p.primary)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .height(40)
                .padding([0, 12])
                .style(styles::ghost(p))
                .on_press(Message::Onboarding(Msg::Back)),
            );
        }
        r = r.push(Space::new().width(Length::Fill));
        let label = row![
            text(next_label.to_string()).size(14),
            icon(Icon::ArrowForward, 18.0)
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        r = r.push(
            button(container(label).center_y(Length::Fill))
                .height(40)
                .padding([0, 24])
                .style(styles::primary(p))
                .on_press_maybe(next),
        );
        container(r)
            .padding(iced::Padding {
                top: 40.0,
                ..iced::Padding::ZERO
            })
            .width(Length::Fill)
            .into()
    }

    fn underline_input<'a>(
        &self,
        placeholder: &str,
        value: &str,
        on_input: impl Fn(String) -> Message + 'a,
        on_submit: Message,
        size: f32,
        p: Palette,
    ) -> Element<'a, Message> {
        text_input(placeholder, value)
            .on_input(on_input)
            .on_submit(on_submit)
            .size(size)
            .padding([12, 0])
            .style(move |_, status| iced::widget::text_input::Style {
                background: Color::TRANSPARENT.into(),
                border: iced::Border {
                    color: if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
                        p.primary
                    } else {
                        p.outline
                    },
                    width: if matches!(status, iced::widget::text_input::Status::Focused { .. }) {
                        2.0
                    } else {
                        1.0
                    },
                    radius: 0.0.into(),
                },
                icon: p.on_surface_variant,
                placeholder: crate::theme::alpha(p.on_surface_variant, 0.6),
                value: p.on_surface,
                selection: crate::theme::alpha(p.primary, 0.35),
            })
            .into()
    }

    pub(crate) fn onboarding_view(&self, o: &State, p: Palette) -> Element<'_, Message> {
        let h1 = |s: &str| text(s.to_string()).size(38).line_height(iced::Pixels(46.0));
        let sub = |s: &str| {
            text(s.to_string())
                .size(16)
                .line_height(iced::Pixels(24.0))
                .color(p.on_surface_variant)
        };
        let step: Element<'_, Message> = match o.step {
            0 => column![
                container(logo::mark(44.0)).padding(iced::Padding { bottom: 28.0, ..iced::Padding::ZERO }),
                h1("Your music, pieced together."),
                sub("SoundCloud and the files on your computer, in one player. Setup takes about a minute."),
                Space::new().height(Length::Fill),
                self.step_actions(false, "Get started", Some(Message::Onboarding(Msg::Next)), p),
            ]
            .spacing(12)
            .height(Length::Fill)
            .into(),
            1 => column![
                h1("What should I call you?"),
                sub("It's only used for the greeting on the home screen."),
                container(self.underline_input("Your name", &o.name, |n| Message::Onboarding(Msg::Name(n)), Message::Onboarding(Msg::Next), 20.0, p)).padding(iced::Padding { top: 20.0, ..iced::Padding::ZERO }),
                Space::new().height(Length::Fill),
                self.step_actions(true, if o.name.trim().is_empty() { "Skip" } else { "Continue" }, Some(Message::Onboarding(Msg::Next)), p),
            ]
            .spacing(12)
            .height(Length::Fill)
            .into(),
            2 => {
                let chips = iced::widget::Row::with_children(o.folders.iter().enumerate().map(|(i, f)| {
                    container(
                        row![
                            icon(Icon::Folder, 16.0).color(p.on_surface_variant),
                            text(base_name(f).to_string()).size(14).color(p.on_surface_variant),
                            icon_button(Icon::Close, "Remove", p, Message::Onboarding(Msg::RemoveFolder(i))),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding(iced::Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 12.0 })
                    .style(move |_| container::Style {
                        border: iced::Border { color: p.outline_variant, width: 1.0, radius: 8.0.into() },
                        ..container::Style::default()
                    })
                    .into()
                }))
                .spacing(8)
                .wrap();
                column![
                    h1("Got music on this computer?"),
                    sub("Pick a folder and it shows up next to SoundCloud. You can add more any time, or drag files into the window."),
                    container(tonal_button("Choose a folder", Icon::CreateNewFolder, p, Message::Onboarding(Msg::PickFolder))).padding(iced::Padding { top: 16.0, ..iced::Padding::ZERO }),
                    chips,
                    Space::new().height(Length::Fill),
                    self.step_actions(true, if o.folders.is_empty() { "Skip for now" } else { "Continue" }, Some(Message::Onboarding(Msg::Next)), p),
                ]
                .spacing(12)
                .height(Length::Fill)
                .into()
            }
            3 => {
                let imp = &self.ui.import;
                let busy = matches!(imp.step, ImportStep::Fetching | ImportStep::Matching);
                let go = button(container(text(if imp.step == ImportStep::Fetching { "Reading…" } else { "Import" }).size(14)).center_y(Length::Fill))
                    .height(40)
                    .padding([0, 20])
                    .style(styles::tonal(p))
                    .on_press_maybe((!busy && !imp.url.trim().is_empty()).then_some(Message::Import(super::import::Msg::Start)));
                let input = text_input("https://open.spotify.com/playlist/…", &imp.url)
                    .on_input_maybe((!busy).then_some(|u| Message::Import(super::import::Msg::Url(u))))
                    .on_submit(Message::Import(super::import::Msg::Start))
                    .size(16)
                    .padding([12, 0])
                    .style(move |_, _| iced::widget::text_input::Style {
                        background: Color::TRANSPARENT.into(),
                        border: iced::Border { color: p.outline, width: 1.0, radius: 0.0.into() },
                        icon: p.on_surface_variant,
                        placeholder: crate::theme::alpha(p.on_surface_variant, 0.6),
                        value: p.on_surface,
                        selection: crate::theme::alpha(p.primary, 0.35),
                    });
                let mut status = column![].spacing(10);
                if imp.step == ImportStep::Matching {
                    let total = imp.results.len();
                    let done = imp.done();
                    status = status
                        .push(text(format!("Finding “{}” on SoundCloud… {done} / {total}", imp.data.as_ref().map(|d| d.name.clone()).unwrap_or_default())).size(14).color(p.on_surface_variant))
                        .push(
                            container(container(Space::new().width(Length::FillPortion((done * 1000).checked_div(total).map_or(1, |v| v as u16 + 1))).height(6)).style(styles::fill(p.primary, 3.0)))
                                .width(Length::Fill)
                                .style(styles::fill(p.outline_variant, 3.0)),
                        );
                }
                if imp.step == ImportStep::Input && !imp.error.is_empty() {
                    status = status.push(text(imp.error.clone()).size(14).color(p.error));
                }
                let chips = iced::widget::Row::with_children(o.imports.iter().enumerate().map(|(i, im)| {
                    container(
                        row![
                            icon(Icon::PlaylistPlay, 18.0).color(p.on_surface_variant),
                            text(format!("{} · {}/{}", im.name, im.tracks.len(), im.total)).size(14).color(p.on_surface_variant),
                            icon_button(Icon::Close, "Remove", p, Message::Onboarding(Msg::RemoveImport(i))),
                        ]
                        .spacing(6)
                        .align_y(Alignment::Center),
                    )
                    .padding(iced::Padding { top: 0.0, right: 2.0, bottom: 0.0, left: 12.0 })
                    .style(move |_| container::Style {
                        border: iced::Border { color: p.outline_variant, width: 1.0, radius: 8.0.into() },
                        ..container::Style::default()
                    })
                    .into()
                }))
                .spacing(8)
                .wrap();
                column![
                    h1("Coming from Spotify?"),
                    sub("Paste a playlist or album link and the songs get matched on SoundCloud. Public ones just work."),
                    container(row![container(input).width(Length::Fill), go].spacing(12).align_y(Alignment::Center)).padding(iced::Padding { top: 16.0, ..iced::Padding::ZERO }),
                    status,
                    chips,
                    Space::new().height(Length::Fill),
                    self.step_actions(true, if o.imports.is_empty() { "Skip for now" } else { "Continue" }, (!busy).then_some(Message::Onboarding(Msg::Next)), p),
                ]
                .spacing(12)
                .height(Length::Fill)
                .into()
            }
            _ => {
                let opt = |title: &str, hint: &str, on: bool, msg: Message| {
                    container(
                        row![
                            column![text(title.to_string()).size(16), text(hint.to_string()).size(13).color(p.on_surface_variant)].spacing(2).width(Length::Fill),
                            switch(on, p, msg),
                        ]
                        .spacing(24)
                        .align_y(Alignment::Center),
                    )
                    .padding([10, 0])
                };
                column![
                    h1("Two quick things"),
                    sub("Both can be changed later in Settings."),
                    column![
                        opt("Discord status", "Show what you're playing on your Discord profile.", o.presence, Message::Onboarding(Msg::Presence(!o.presence))),
                        opt("Lyrics panel", "Words follow the music in a panel on the right.", o.lyrics, Message::Onboarding(Msg::Lyrics(!o.lyrics))),
                    ]
                    .padding(iced::Padding { top: 16.0, ..iced::Padding::ZERO }),
                    Space::new().height(Length::Fill),
                    container(
                        row![
                            button(row![icon(Icon::ArrowBack, 18.0).color(p.primary), text("Back").size(14).color(p.primary)].spacing(8).align_y(Alignment::Center))
                                .height(40)
                                .padding([0, 12])
                                .style(styles::ghost(p))
                                .on_press(Message::Onboarding(Msg::Back)),
                            Space::new().width(Length::Fill),
                            primary_button(if o.busy { "Setting up…" } else { "Start listening" }, Icon::Check, p, Message::Onboarding(Msg::Finish)),
                        ]
                        .align_y(Alignment::Center),
                    )
                    .padding(iced::Padding { top: 40.0, ..iced::Padding::ZERO }),
                ]
                .spacing(12)
                .height(Length::Fill)
                .into()
            }
        };
        let bars = row((0..STEPS).map(|i| {
            container(Space::new().width(28).height(4))
                .style(styles::fill(
                    if i <= o.step {
                        p.primary
                    } else {
                        p.outline_variant
                    },
                    2.0,
                ))
                .into()
        }))
        .spacing(8);
        let column = column![
            text(format!("{:02} / {:02}", o.step + 1, STEPS))
                .size(13)
                .color(p.on_surface_variant),
            container(step).height(Length::Fill).padding(iced::Padding {
                top: 40.0,
                ..iced::Padding::ZERO
            }),
            container(bars).padding(iced::Padding {
                top: 40.0,
                ..iced::Padding::ZERO
            }),
        ]
        .max_width(460)
        .height(460);
        row![
            container(self.mosaic_art(o.step, p))
                .width(Length::Fill)
                .height(Length::Fill),
            container(column)
                .width(Length::Fill)
                .height(Length::Fill)
                .center_y(Length::Fill)
                .padding(iced::Padding {
                    top: 56.0,
                    right: 64.0,
                    bottom: 32.0,
                    left: 64.0
                }),
        ]
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_light_up_in_the_js_order() {
        assert_eq!(lit_tiles(0).iter().filter(|b| **b).count(), 3);
        assert_eq!(lit_tiles(4), [true; 9]);
        let first = lit_tiles(0);
        assert!(first[0] && first[1] && first[2] && !first[3]);
        let second = lit_tiles(1);
        assert!(second[5] && second[8] && !second[7]);
    }

    #[test]
    fn folder_names_come_from_the_last_component() {
        assert_eq!(base_name("/home/u/Music/"), "Music");
        assert_eq!(base_name("C:\\Music"), "Music");
    }
}
