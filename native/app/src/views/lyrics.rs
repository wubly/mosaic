//! Port of `LyricsPanel.js` and `LyricsPicker.js`. Synced lyrics only; the playing line lights up
//! as a whole (word timing is too often off to sweep through a line).

use std::cell::RefCell;
use std::collections::HashMap;

use iced::widget::{
    button, column, container, mouse_area, row, scrollable, stack, text, text_input, Space,
};
use iced::{Alignment, Element, Font, Length, Task};
use mosaic_core::lyrics_timing::{build_timings, LyricLine, WordTiming};
use mosaic_core::model::{Track, TrackId};
use mosaic_services::lyrics::service::{Candidates, LyricsArgs};
use mosaic_services::lyrics::{Lyrics, LyricsKind};

use super::artwork_of;
use super::common;
use crate::app::{App, Message};
use crate::text_measure;
use crate::theme::{alpha, mix, styles, Palette};
use crate::widgets::icon::{icon, outlined, Icon};

pub const SCROLL_ID: &str = "lyrics-scroll";
const LOOKAHEAD: f64 = 0.04;
const LINE_FONT: Font = Font {
    weight: iced::font::Weight::Bold,
    ..Font::with_name("Roboto Flex")
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Panel,
    Large,
    Bare,
}

impl Variant {
    fn font_size(self) -> f32 {
        match self {
            Variant::Panel => 22.0,
            Variant::Large => 36.0,
            Variant::Bare => 20.0,
        }
    }

    fn line_height(self) -> f32 {
        match self {
            Variant::Panel => 30.0,
            Variant::Large => 46.0,
            Variant::Bare => 28.0,
        }
    }

    fn line_pad(self) -> f32 {
        match self {
            Variant::Panel => 8.0,
            Variant::Large => 10.0,
            Variant::Bare => 6.0,
        }
    }
}

pub fn kind_label(kind: LyricsKind) -> &'static str {
    match kind {
        LyricsKind::Word => "Word synced",
        LyricsKind::Synced => "Synced",
        LyricsKind::Plain => "Unsynced",
        LyricsKind::Instrumental => "Instrumental",
        LyricsKind::None => "No lyrics",
    }
}

#[derive(Clone, Debug)]
pub enum Msg {
    Loaded(TrackId, Box<Lyrics>),
    SeekTo(f64),
    OpenPicker,
    ClosePicker,
    Candidates(TrackId, Box<Candidates>),
    Query(String),
    Search,
    Pick(Option<String>),
    Picked(TrackId, Box<Lyrics>),
}

/// Per-word widths for every line, and the width of a space.
type Widths = (Vec<Vec<f32>>, f32);

#[derive(Default)]
pub struct Picker {
    pub candidates: Option<Candidates>,
    pub query: String,
}

#[derive(Default)]
pub struct State {
    pub track: Option<TrackId>,
    pub lyrics: Option<Lyrics>,
    pub timings: Vec<Vec<WordTiming>>,
    pub active: isize,
    pub picker: Option<Picker>,
    /// word widths per font size, filled on first use.
    widths: RefCell<HashMap<u32, Widths>>,
}

/// Index of the line playing at `t`, or -1 before the first line.
pub fn active_line(lines: &[LyricLine], t: f64) -> isize {
    lines.partition_point(|l| l.t <= t) as isize - 1
}

pub fn line_opacity(distance: f32) -> f32 {
    let at = |d: f32| {
        if d < 0.5 {
            1.0
        } else if d < 2.5 {
            0.55
        } else if d < 3.5 {
            0.28
        } else {
            0.22
        }
    };
    // Smooth between the steps so the moving highlight eases instead of snapping.
    let lo = distance.floor();
    let t = distance - lo;
    at(lo) + (at(lo + 1.0) - at(lo)) * t
}

impl State {
    pub fn lines(&self) -> &[LyricLine] {
        self.lyrics.as_ref().map_or(&[], |l| l.synced.as_slice())
    }

    pub fn set(&mut self, lyrics: Lyrics) {
        self.timings = build_timings(&lyrics.synced);
        self.lyrics = Some(lyrics);
        self.active = -1;
        self.widths.borrow_mut().clear();
    }

    pub fn clear(&mut self, track: Option<TrackId>) {
        *self = State {
            track,
            ..State::default()
        };
    }

    fn widths(&self, v: Variant) -> Widths {
        let key = v.font_size() as u32;
        if let Some(hit) = self.widths.borrow().get(&key) {
            return hit.clone();
        }
        let size = v.font_size();
        let computed: Vec<Vec<f32>> = self
            .timings
            .iter()
            .map(|line| {
                line.iter()
                    .map(|w| text_measure::width(&w.w, size, LINE_FONT))
                    .collect()
            })
            .collect();
        let space = text_measure::width("a a", size, LINE_FONT)
            - 2.0 * text_measure::width("a", size, LINE_FONT);
        self.widths
            .borrow_mut()
            .insert(key, (computed.clone(), space));
        (computed, space)
    }

    /// Top and height of every line inside the scrolled content (excluding the box padding).
    pub fn line_boxes(&self, v: Variant, avail: f32) -> Vec<(f32, f32)> {
        let (widths, space) = self.widths(v);
        let mut y = 0.0;
        widths
            .iter()
            .map(|w| {
                let n = text_measure::wrapped_lines(w, space, avail);
                let h = n as f32 * v.line_height() + v.line_pad() * 2.0;
                let top = y;
                y += h;
                (top, h)
            })
            .collect()
    }
}

fn args_for(track: &Track) -> LyricsArgs {
    LyricsArgs {
        id: track.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        duration_ms: track.duration,
        lrc_path: track.lrc_path.clone(),
    }
}

impl App {
    pub(crate) fn fetch_lyrics(&mut self) -> Task<Message> {
        let Some(track) = self.player.track.clone() else {
            self.ui.lyrics.clear(None);
            return Task::none();
        };
        self.ui.lyrics.clear(Some(track.id.clone()));
        self.anim.reset("lyr-pos", -1.0, 170.0, 26.0);
        let services = self.services.clone();
        let id = track.id.clone();
        Task::perform(
            async move { services.get_lyrics(&args_for(&track)).await },
            move |l| Message::Lyrics(Msg::Loaded(id.clone(), Box::new(l))),
        )
    }

    pub(crate) fn lyrics_box_height(&self) -> f32 {
        (self.size.height - 40.0 - 32.0 - self.settings().bar_height as f32 - 16.0 - 260.0)
            .max(120.0)
    }

    pub(crate) fn lyrics_avail_width(&self, v: Variant) -> f32 {
        match v {
            Variant::Panel => self.settings().side_width as f32 - 56.0,
            Variant::Bare => (self.size.width - 40.0).max(200.0),
            Variant::Large => ((self.size.width - 128.0 - 56.0) * 1.15 / 2.15 - 24.0).max(240.0),
        }
    }

    /// Follows the playhead; returns a scroll task when the active line changed.
    pub(crate) fn lyrics_tick(&mut self) -> Task<Message> {
        let v = self.current_lyrics_variant();
        let st = &self.ui.lyrics;
        if st.lines().is_empty() {
            return Task::none();
        }
        let t = self.display_position() + LOOKAHEAD;
        let i = active_line(st.lines(), t);
        let mut task = Task::none();
        if i != st.active {
            self.ui.lyrics.active = i;
            self.anim.spring("lyr-pos", i as f32, 170.0, 26.0);
            let boxes = self.ui.lyrics.line_boxes(v, self.lyrics_avail_width(v));
            if let Some((top, h)) = boxes.get(i.max(0) as usize) {
                let view_h = if v == Variant::Large {
                    self.size.height - 40.0 - 200.0
                } else {
                    self.lyrics_box_height()
                };
                let pad = box_padding(v, view_h).0;
                let target = pad + top + h / 2.0 - view_h / 2.0;
                self.anim.spring("lyr-scroll", target.max(0.0), 170.0, 28.0);
            }
        }
        if self.anim.is_spring_active("lyr-scroll") {
            let y = self.anim.value("lyr-scroll", 0.0);
            task = iced::widget::operation::scroll_to(
                SCROLL_ID,
                scrollable::AbsoluteOffset { x: 0.0, y },
            );
        }
        task
    }

    pub(crate) fn current_lyrics_variant(&self) -> Variant {
        if self.ui.mini {
            Variant::Bare
        } else if self.ui.now_playing {
            Variant::Large
        } else {
            Variant::Panel
        }
    }

    pub(crate) fn on_lyrics(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Loaded(id, lyrics) => {
                if self.ui.lyrics.track.as_ref() == Some(&id) {
                    self.ui.lyrics.set(*lyrics);
                    self.anim.reset("lyr-scroll", 0.0, 170.0, 28.0);
                    return iced::widget::operation::scroll_to(
                        SCROLL_ID,
                        scrollable::AbsoluteOffset { x: 0.0, y: 0.0 },
                    );
                }
                Task::none()
            }
            Msg::SeekTo(t) => {
                self.pos_at = self.now;
                let seek = self.player_input(crate::player::PlayerInput::Seek(t));
                Task::batch([seek, self.lyrics_tick()])
            }
            Msg::OpenPicker => {
                let Some(track) = self.player.track.clone() else {
                    return Task::none();
                };
                self.ui.lyrics.picker = Some(Picker::default());
                let services = self.services.clone();
                let id = track.id.clone();
                Task::perform(
                    async move { services.get_candidates(&args_for(&track), "").await },
                    move |c| Message::Lyrics(Msg::Candidates(id.clone(), Box::new(c))),
                )
            }
            Msg::ClosePicker => {
                self.ui.lyrics.picker = None;
                Task::none()
            }
            Msg::Candidates(id, c) => {
                if self.ui.lyrics.track.as_ref() == Some(&id) {
                    if let Some(p) = &mut self.ui.lyrics.picker {
                        if p.query.is_empty() {
                            p.query = c.query.clone();
                        }
                        p.candidates = Some(*c);
                    }
                }
                Task::none()
            }
            Msg::Query(q) => {
                if let Some(p) = &mut self.ui.lyrics.picker {
                    p.query = q;
                }
                Task::none()
            }
            Msg::Search => {
                let Some(track) = self.player.track.clone() else {
                    return Task::none();
                };
                let query = self
                    .ui
                    .lyrics
                    .picker
                    .as_ref()
                    .map(|p| p.query.clone())
                    .unwrap_or_default();
                if let Some(p) = &mut self.ui.lyrics.picker {
                    p.candidates = None;
                }
                let services = self.services.clone();
                let id = track.id.clone();
                Task::perform(
                    async move { services.get_candidates(&args_for(&track), &query).await },
                    move |c| Message::Lyrics(Msg::Candidates(id.clone(), Box::new(c))),
                )
            }
            Msg::Pick(cand) => {
                let Some(track) = self.player.track.clone() else {
                    return Task::none();
                };
                self.toast(if cand.is_some() {
                    "Lyrics changed"
                } else {
                    "Using the automatic match"
                });
                self.ui.lyrics.picker = None;
                let services = self.services.clone();
                let id = track.id.clone();
                Task::perform(
                    async move {
                        services
                            .select_lyrics(&args_for(&track), cand.as_deref())
                            .await
                            .unwrap_or_else(|_| mosaic_services::lyrics::parse::empty_lyrics())
                    },
                    move |l| Message::Lyrics(Msg::Picked(id.clone(), Box::new(l))),
                )
            }
            Msg::Picked(id, lyrics) => self.on_lyrics(Msg::Loaded(id, lyrics)),
        }
    }

    fn source_pill(&self, p: Palette) -> Element<'_, Message> {
        let kind = self.ui.lyrics.lyrics.as_ref().map(|l| l.meta.kind);
        let label = kind.map_or("Finding lyrics…", kind_label);
        let highlighted = matches!(kind, Some(LyricsKind::Word | LyricsKind::Synced));
        let (bg, fg) = if highlighted {
            (p.primary_container, p.on_primary_container)
        } else {
            (
                crate::theme::mix(p.on_surface, iced::Color::TRANSPARENT, 0.10),
                p.on_surface_variant,
            )
        };
        // A fixed-height button puts its content at the top; centre it in the pill.
        button(
            iced::widget::container(
                row![
                    outlined(Icon::Subtitles, 16.0).color(fg),
                    text(label).size(12).color(fg),
                    icon(Icon::ExpandMore, 18.0).color(fg)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .center_y(Length::Fill),
        )
        .height(30)
        .padding(iced::Padding {
            top: 0.0,
            right: 8.0,
            bottom: 0.0,
            left: 12.0,
        })
        .style(move |_, status| button::Style {
            background: Some(
                if matches!(status, button::Status::Hovered) {
                    crate::theme::mix(p.on_surface, bg, 0.12)
                } else {
                    bg
                }
                .into(),
            ),
            text_color: fg,
            border: iced::border::rounded(15),
            ..button::Style::default()
        })
        .on_press(Message::Lyrics(Msg::OpenPicker))
        .into()
    }

    fn lyric_lines(&self, v: Variant, p: Palette) -> Element<'_, Message> {
        let st = &self.ui.lyrics;
        let (_, space) = st.widths(v);
        let pos = self.anim.value("lyr-pos", st.active as f32);
        let size = v.font_size();
        let h = v.line_height();
        let mut col = column![].width(Length::Fill);
        for (i, line) in st.lines().iter().enumerate() {
            let distance = (i as f32 - pos).abs();
            // The playing line is fully lit; the glow eases between lines as `pos` springs.
            let lit = (1.0 - distance).clamp(0.0, 1.0);
            let op = line_opacity(distance);
            let color = alpha(p.on_surface, (0.42 + 0.58 * lit) * op.max(lit));
            let mut words = iced::widget::Row::new().spacing(space).width(Length::Fill);
            for w in &st.timings[i] {
                words = words.push(
                    text(w.w.clone())
                        .size(size)
                        .font(LINE_FONT)
                        .line_height(iced::Pixels(h))
                        .wrapping(text::Wrapping::None)
                        .color(color),
                );
            }
            let words = if v == Variant::Bare {
                words.wrap().align_x(Alignment::Center)
            } else {
                words.wrap()
            }
            .vertical_spacing(0);
            let align = if v == Variant::Bare {
                iced::alignment::Horizontal::Center
            } else {
                iced::alignment::Horizontal::Left
            };
            col = col.push(
                mouse_area(
                    container(words)
                        .width(Length::Fill)
                        .padding([v.line_pad(), 0.0])
                        .align_x(align),
                )
                .on_press(Message::Lyrics(Msg::SeekTo(line.t)))
                .interaction(iced::mouse::Interaction::Pointer),
            );
        }
        col.into()
    }

    pub(crate) fn lyrics_body(&self, v: Variant, p: Palette) -> Element<'_, Message> {
        let st = &self.ui.lyrics;
        let view_h = if v == Variant::Large {
            self.size.height - 40.0 - 200.0
        } else {
            self.lyrics_box_height()
        };
        let (top, bottom) = box_padding(v, view_h);
        let body: Element<'_, Message> = match &st.lyrics {
            None => text("Finding lyrics…")
                .size(14)
                .color(p.on_surface_variant)
                .into(),
            Some(l) if !l.synced.is_empty() => self.lyric_lines(v, p),
            Some(_) if v == Variant::Large => column![
                text("No synced lyrics for this track.")
                    .size(16)
                    .color(p.on_surface_variant),
                self.source_pill(p)
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
            Some(_) if v == Variant::Bare => column![
                text("No synced lyrics for this track.")
                    .size(14)
                    .color(p.on_surface_variant),
                self.source_pill(p)
            ]
            .spacing(10)
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into(),
            Some(_) => column![
                text("No synced lyrics for this track.")
                    .size(14)
                    .color(p.on_surface_variant),
                self.source_pill(p)
            ]
            .spacing(10)
            .into(),
        };
        let side = if v == Variant::Large {
            12.0
        } else if v == Variant::Bare {
            4.0
        } else {
            28.0
        };
        let scroll = scrollable(container(body).width(Length::Fill).padding(iced::Padding {
            top,
            right: side,
            bottom,
            left: side,
        }))
        .id(SCROLL_ID)
        .style(move |theme, status| {
            let mut s = styles::scrollbar(p)(theme, status);
            s.vertical_rail.scroller.background = iced::Color::TRANSPARENT.into();
            s
        })
        .height(Length::Fill);
        // Lines fade into a soft shadow at the edges instead of being cut off flat.
        let shade = match v {
            Variant::Large => mix(p.surface, iced::Color::TRANSPARENT, 0.5),
            _ => mix(p.surface, iced::Color::TRANSPARENT, 0.6),
        };
        let edge = |down: bool, height: f32| {
            let (from, to) = if down {
                (shade, iced::Color::TRANSPARENT)
            } else {
                (iced::Color::TRANSPARENT, shade)
            };
            container(Space::new())
                .width(Length::Fill)
                .height(height)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Gradient(
                        iced::gradient::Linear::new(std::f32::consts::PI)
                            .add_stop(0.0, from)
                            .add_stop(1.0, to)
                            .into(),
                    )),
                    ..container::Style::default()
                })
        };
        let scroll = crate::widgets::smooth::smooth(scroll);
        if st.lines().is_empty() {
            return scroll;
        }
        stack![
            scroll,
            column![
                edge(true, 48.0),
                Space::new().height(Length::Fill),
                edge(false, 72.0),
            ]
            .width(Length::Fill)
            .height(Length::Fill),
        ]
        .into()
    }

    pub(crate) fn lyrics_panel(&self, p: Palette) -> Element<'_, Message> {
        let Some(track) = &self.player.track else {
            return container(
                column![
                    container(icon(Icon::Lyrics, 32.0).color(p.on_primary_container))
                        .center(72)
                        .style(styles::fill(p.primary_container, 24.0)),
                    text("No lyrics yet").size(18),
                    text("Play a song and its lyrics show up here, line by line.")
                        .size(13)
                        .color(p.on_surface_variant),
                ]
                .spacing(6)
                .align_x(Alignment::Center)
                .max_width(220),
            )
            .center(Length::Fill)
            .padding(32)
            .into();
        };
        let head = column![
            container(self.art(artwork_of(track), 168.0, 20.0)),
            text(track.title.clone())
                .size(18)
                .line_height(iced::Pixels(24.0))
                .align_x(iced::alignment::Horizontal::Center),
            text(track.artist.to_uppercase())
                .size(12)
                .color(p.on_surface_variant),
            self.source_pill(p),
        ]
        .spacing(8)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .padding(iced::Padding {
            top: 28.0,
            right: 24.0,
            bottom: 12.0,
            left: 24.0,
        });
        let base: Element<'_, Message> = column![head, self.lyrics_body(Variant::Panel, p)].into();
        self.with_picker(base, p)
    }

    pub(crate) fn with_picker<'a>(
        &'a self,
        base: Element<'a, Message>,
        p: Palette,
    ) -> Element<'a, Message> {
        match &self.ui.lyrics.picker {
            Some(picker) => {
                let t = self.anim.value("pres-picker", 1.0).clamp(0.0, 1.0);
                let sheet =
                    common::fade_up(self.picker_view(picker, p), t, 12.0, p.surface_container);
                stack![base, sheet].into()
            }
            None => base,
        }
    }

    fn kind_badge<'a>(&self, kind: LyricsKind, p: Palette) -> Element<'a, Message> {
        let (bg, fg, border) = match kind {
            LyricsKind::Word => (Some(p.primary), p.on_primary, iced::Color::TRANSPARENT),
            LyricsKind::Synced => (
                Some(p.primary_container),
                p.on_primary_container,
                iced::Color::TRANSPARENT,
            ),
            _ => (None, p.on_surface_variant, p.outline_variant),
        };
        container(text(kind_label(kind)).size(11).color(fg))
            .padding([0, 8])
            .height(20)
            .center_y(20)
            .style(move |_| container::Style {
                background: bg.map(Into::into),
                border: iced::Border {
                    color: border,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..container::Style::default()
            })
            .into()
    }

    fn picker_view<'a>(&'a self, picker: &'a Picker, p: Palette) -> Element<'a, Message> {
        let current = self.ui.lyrics.lyrics.as_ref().map(|l| &l.meta);
        let header = row![
            text("Lyrics sources").size(18),
            Space::new().width(Length::Fill),
            super::common::icon_button(
                Icon::Close,
                "Close (Esc)",
                p,
                Message::Lyrics(Msg::ClosePicker)
            ),
        ]
        .align_y(Alignment::Center);
        let search = container(
            row![
                icon(Icon::Search, 20.0).color(p.on_surface_variant),
                text_input("Search LRCLIB", &picker.query)
                    .on_input(|q| Message::Lyrics(Msg::Query(q)))
                    .on_submit(Message::Lyrics(Msg::Search))
                    .size(14)
                    .padding(0)
                    .style(move |_, _| iced::widget::text_input::Style {
                        background: iced::Color::TRANSPARENT.into(),
                        border: iced::Border::default(),
                        icon: p.on_surface_variant,
                        placeholder: p.on_surface_variant,
                        value: p.on_surface,
                        selection: alpha(p.primary, 0.35),
                    }),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([0, 12])
        .height(40)
        .center_y(40)
        .style(styles::fill(p.surface_container_high, 20.0));
        let mut body = column![
            header,
            search,
            text("Synced lyrics from LRCLIB for this song and artist")
                .size(12)
                .color(p.on_surface_variant),
        ]
        .spacing(12);
        let mut list = column![].spacing(2);
        let is_custom = current.is_some_and(|m| m.origin == "custom");
        if is_custom {
            list = list.push(
                button(
                    row![
                        icon(Icon::AutoAwesome, 20.0).color(p.primary),
                        column![
                            text("Use automatic match").size(14).color(p.primary),
                            text("Let Mosaic choose the best version")
                                .size(12)
                                .color(p.on_surface_variant)
                        ],
                    ]
                    .spacing(12)
                    .align_y(Alignment::Center),
                )
                .width(Length::Fill)
                .padding([10, 12])
                .style(styles::plain(p, 14.0))
                .on_press(Message::Lyrics(Msg::Pick(None))),
            );
        }
        match &picker.candidates {
            None => {
                for _ in 0..5 {
                    list = list.push(
                        container(Space::new().height(64))
                            .width(Length::Fill)
                            .style(styles::fill(p.surface_container_high, 14.0)),
                    );
                }
            }
            Some(c) if c.list.is_empty() => {
                list = list.push(
                    container(
                        text("Nothing found. Try another search.")
                            .size(14)
                            .color(p.on_surface_variant),
                    )
                    .padding(16)
                    .center_x(Length::Fill),
                );
            }
            Some(c) => {
                for cand in &c.list {
                    let on = if cand.id == "file" {
                        current.is_some_and(|m| m.origin == "file")
                    } else {
                        current.and_then(|m| m.id.as_ref()) == Some(&cand.id)
                    };
                    let mut tags = row![self.kind_badge(cand.kind, p)]
                        .spacing(8)
                        .align_y(Alignment::Center);
                    if cand.duration > 0 {
                        let diff = if cand.diff != 0 {
                            format!(" ({}{}s)", if cand.diff > 0 { "+" } else { "" }, cand.diff)
                        } else {
                            String::new()
                        };
                        tags = tags.push(
                            text(format!(
                                "{}:{:02}{diff}",
                                cand.duration / 60,
                                cand.duration % 60
                            ))
                            .size(12)
                            .color(p.on_surface_variant),
                        );
                    }
                    let sub = [cand.artist.as_str(), cand.album.as_str()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    let mut line = row![column![
                        text(cand.title.clone())
                            .size(14)
                            .wrapping(text::Wrapping::None),
                        text(sub)
                            .size(12)
                            .color(p.on_surface_variant)
                            .wrapping(text::Wrapping::None),
                        tags,
                    ]
                    .spacing(2)
                    .width(Length::Fill)]
                    .align_y(Alignment::Center);
                    if on {
                        line = line.push(icon(Icon::Check, 22.0).color(p.primary));
                    }
                    list = list.push(
                        button(line)
                            .width(Length::Fill)
                            .padding([10, 12])
                            .style(move |_, status| button::Style {
                                background: if on {
                                    Some(
                                        crate::theme::mix(
                                            p.primary,
                                            iced::Color::TRANSPARENT,
                                            0.16,
                                        )
                                        .into(),
                                    )
                                } else if matches!(status, button::Status::Hovered) {
                                    Some(
                                        crate::theme::mix(
                                            p.on_surface,
                                            iced::Color::TRANSPARENT,
                                            0.08,
                                        )
                                        .into(),
                                    )
                                } else {
                                    None
                                },
                                text_color: p.on_surface,
                                border: iced::border::rounded(14),
                                ..button::Style::default()
                            })
                            .on_press(Message::Lyrics(Msg::Pick(Some(cand.id.clone())))),
                    );
                }
            }
        }
        body = body.push(crate::widgets::smooth::smooth(
            scrollable(list)
                .style(styles::scrollbar(p))
                .height(Length::Fill),
        ));
        mouse_area(
            container(body)
                .padding(iced::Padding {
                    top: 16.0,
                    right: 16.0,
                    bottom: 12.0,
                    left: 16.0,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(p.surface_container.into()),
                    border: iced::border::rounded(28),
                    ..container::Style::default()
                }),
        )
        .on_press(Message::Nop)
        .into()
    }
}

/// Top/bottom padding of the scrolled content (CSS `padding: 96px … 160px`, `30vh` for large).
fn box_padding(v: Variant, view_h: f32) -> (f32, f32) {
    match v {
        Variant::Panel => (96.0, 160.0),
        Variant::Bare => (56.0, 80.0),
        Variant::Large => (view_h * 0.35, view_h * 0.35),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: f64) -> LyricLine {
        LyricLine {
            t,
            text: "x".into(),
            words: None,
        }
    }

    #[test]
    fn active_line_follows_the_playhead() {
        let lines = [line(1.0), line(5.0), line(9.0)];
        assert_eq!(active_line(&lines, 0.5), -1);
        assert_eq!(active_line(&lines, 1.0), 0);
        assert_eq!(active_line(&lines, 6.0), 1);
        assert_eq!(active_line(&lines, 99.0), 2);
        assert_eq!(active_line(&[], 3.0), -1);
    }

    #[test]
    fn opacity_steps_with_distance() {
        assert_eq!(line_opacity(0.0), 1.0);
        assert!((line_opacity(1.0) - 0.55).abs() < 1e-6);
        assert!((line_opacity(2.0) - 0.55).abs() < 1e-6);
        assert!((line_opacity(3.0) - 0.28).abs() < 1e-6);
        assert!((line_opacity(6.0) - 0.22).abs() < 1e-6);
        assert!(line_opacity(0.5) < 1.0 && line_opacity(0.5) > 0.55);
    }

    #[test]
    fn labels_match_the_js() {
        assert_eq!(kind_label(LyricsKind::Plain), "Unsynced");
        assert_eq!(kind_label(LyricsKind::Word), "Word synced");
        assert_eq!(kind_label(LyricsKind::None), "No lyrics");
    }
}
