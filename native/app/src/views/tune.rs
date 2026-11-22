//! Port of `TunePopover.js` and `tune/*`.

use iced::widget::{button, column, container, mouse_area, row, stack, text, Space};
use iced::{Alignment, Element, Length};
use mosaic_core::model::Settings;

use crate::app::{App, Message};
use crate::player::SleepRequest;
use crate::theme::{styles, Palette};
use crate::widgets::gain_slider::gain_slider;
use crate::widgets::icon::{icon, Icon};
use crate::widgets::switch::switch;

pub const BANDS: [&str; 5] = ["60", "230", "910", "3.6k", "14k"];
pub const PRESETS: [(&str, [f64; 5]); 7] = [
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass boost", [7.0, 4.0, 0.0, 0.0, 0.0]),
    ("Treble boost", [0.0, 0.0, 0.0, 4.0, 7.0]),
    ("Vocal", [-2.0, 0.0, 4.0, 3.0, 0.0]),
    ("Electronic", [5.0, 2.0, -2.0, 2.0, 5.0]),
    ("Acoustic", [4.0, 2.0, 1.0, 3.0, 3.0]),
    ("Late night", [3.0, 1.0, -2.0, -3.0, -4.0]),
];
pub const SPEEDS: [f64; 6] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0];
pub const TIMERS: [u32; 4] = [15, 30, 45, 60];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Equalizer,
    Speed,
    Sleep,
}

#[derive(Debug, Clone)]
pub enum Msg {
    Toggle,
    Close,
    Tab(Tab),
    EqToggle,
    EqGain(usize, f32),
    EqPreset(usize),
    Speed(f64),
    Sleep(SleepRequest),
}

pub fn gain_label(gain: f64) -> String {
    if gain > 0.0 {
        format!("+{}", gain)
    } else {
        format!("{}", gain)
    }
}

pub fn speed_label(speed: f64) -> String {
    let s = format!("{speed:.2}");
    format!("{}×", s.strip_suffix('0').unwrap_or(&s))
}

pub fn countdown(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// Applies an EQ edit the way `EqualizerPane.js` does.
pub fn apply_eq(settings: &mut Settings, msg: &Msg) {
    match msg {
        Msg::EqToggle => settings.eq_on = !settings.eq_on,
        Msg::EqGain(i, value) => {
            settings.eq_gains[*i] = f64::from(*value);
            settings.eq_preset = "Custom".into();
            settings.eq_on = true;
        }
        Msg::EqPreset(i) => {
            let (name, gains) = PRESETS[*i];
            settings.eq_gains = gains;
            settings.eq_preset = name.into();
            settings.eq_on = name != "Flat";
        }
        _ => {}
    }
}

fn chip<'a>(label: String, on: bool, p: Palette, msg: Message) -> Element<'a, Message> {
    let mut content = row![];
    if on {
        content = content.push(icon(Icon::Check, 16.0));
    }
    let content = content
        .push(text(label).size(13))
        .spacing(6)
        .align_y(Alignment::Center);
    let bg = if on {
        p.primary_container
    } else {
        p.surface_container_highest
    };
    let fg = if on {
        p.on_primary_container
    } else {
        p.on_surface
    };
    button(content)
        .padding([6, if on { 10 } else { 14 }])
        .style(move |_, status| {
            let mut c = bg;
            if matches!(status, button::Status::Hovered) && !on {
                c = crate::theme::mix(p.on_surface, bg, 0.14);
            }
            button::Style {
                background: Some(c.into()),
                text_color: fg,
                border: iced::border::rounded(8),
                ..button::Style::default()
            }
        })
        .on_press(msg)
        .into()
}

fn chips<'a>(items: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    iced::widget::Row::with_children(items)
        .spacing(8)
        .wrap()
        .into()
}

impl App {
    pub(crate) fn tune_button_on(&self) -> bool {
        let s = self.settings();
        self.ui.tune.is_some()
            || s.eq_on
            || s.speed != 1.0
            || self.player.sleep != crate::player::Sleep::Off
    }

    fn eq_pane(&self, p: Palette) -> Element<'_, Message> {
        let s = self.settings();
        let header = row![
            column![
                text("Equalizer").size(16),
                text(if s.eq_on {
                    s.eq_preset.clone()
                } else {
                    "Off".into()
                })
                .size(13)
                .style(styles::muted(p)),
            ],
            Space::new().width(Length::Fill),
            switch(s.eq_on, p, Message::Tune(Msg::EqToggle)),
        ]
        .align_y(Alignment::Center);

        let scale = column![
            text("+12").size(11),
            Space::new().height(Length::Fill),
            text("0").size(11),
            Space::new().height(Length::Fill),
            text("−12").size(11),
        ]
        .width(26)
        .height(132)
        .align_x(Alignment::End);
        let mut gains = row![].spacing(0).width(Length::Fill);
        for (i, gain) in s.eq_gains.iter().enumerate() {
            gains = gains.push(
                column![
                    text(gain_label(*gain)).size(12).style(styles::muted(p)),
                    gain_slider(
                        *gain as f32,
                        s.eq_on,
                        p,
                        move |v| Message::Tune(Msg::EqGain(i, v)),
                        Message::Tune(Msg::EqGain(i, 0.0)),
                    ),
                    text(BANDS[i]).size(12).style(styles::muted(p)),
                ]
                .spacing(8)
                .align_x(Alignment::Center)
                .width(Length::Fill),
            );
        }
        let card = container(row![scale, gains].spacing(6).padding([14, 14])).style(move |_| {
            container::Style {
                background: Some(
                    crate::theme::alpha(p.surface_container, if s.eq_on { 1.0 } else { 0.5 })
                        .into(),
                ),
                border: iced::border::rounded(20),
                ..container::Style::default()
            }
        });
        let presets = chips(
            PRESETS
                .iter()
                .enumerate()
                .map(|(i, (name, _))| {
                    chip(
                        (*name).into(),
                        s.eq_preset == *name,
                        p,
                        Message::Tune(Msg::EqPreset(i)),
                    )
                })
                .collect(),
        );
        column![header, card, presets].spacing(16).into()
    }

    fn speed_pane(&self, p: Palette) -> Element<'_, Message> {
        let speed = self.settings().speed;
        column![
            container(text(speed_label(speed)).size(44).color(p.primary))
                .center_x(Length::Fill)
                .padding([12, 0]),
            chips(
                SPEEDS
                    .iter()
                    .map(|v| chip(
                        format!("{v}×"),
                        speed == *v,
                        p,
                        Message::Tune(Msg::Speed(*v))
                    ))
                    .collect()
            ),
            text("Pitch is preserved, so vocals keep their natural sound.")
                .size(12)
                .style(styles::muted(p)),
        ]
        .spacing(16)
        .into()
    }

    fn sleep_pane(&self, p: Palette) -> Element<'_, Message> {
        use crate::player::Sleep;
        let sleep = self.player.sleep;
        let label = match sleep {
            Sleep::Timer { left, .. } => countdown(left),
            Sleep::EndOfTrack => "End of track".into(),
            Sleep::Off | Sleep::Fading => "Off".into(),
        };
        let mut items: Vec<Element<'_, Message>> = TIMERS
            .iter()
            .map(|m| {
                chip(
                    format!("{m} min"),
                    false,
                    p,
                    Message::Tune(Msg::Sleep(SleepRequest::Minutes(*m))),
                )
            })
            .collect();
        items.push(chip(
            "End of track".into(),
            sleep == Sleep::EndOfTrack,
            p,
            Message::Tune(Msg::Sleep(SleepRequest::EndOfTrack)),
        ));
        if sleep != Sleep::Off {
            items.push(chip(
                "Turn off".into(),
                false,
                p,
                Message::Tune(Msg::Sleep(SleepRequest::Off)),
            ));
        }
        column![
            container(text(label).size(44).color(p.primary))
                .center_x(Length::Fill)
                .padding([12, 0]),
            chips(items),
            text("Music fades out gently, then pauses.")
                .size(12)
                .style(styles::muted(p)),
        ]
        .spacing(16)
        .into()
    }

    /// The popover and a click-away layer, positioned over the tune button.
    /// The popover; `t` is its open/close progress (0..=1).
    pub(crate) fn tune_overlay(&self, tab: Tab, t: f32, p: Palette) -> Element<'_, Message> {
        let tabs = row([
            (Tab::Equalizer, "Equalizer"),
            (Tab::Speed, "Speed"),
            (Tab::Sleep, "Sleep"),
        ]
        .map(|(t, label)| {
            let on = t == tab;
            let color = if on { p.primary } else { p.on_surface_variant };
            let label = column![
                text(label).size(14).color(color),
                container(Space::new().height(3))
                    .width(Length::Fill)
                    .style(styles::fill(
                        if on {
                            p.primary
                        } else {
                            iced::Color::TRANSPARENT
                        },
                        2.0
                    )),
            ]
            .spacing(10)
            .align_x(Alignment::Center);
            button(
                container(label)
                    .center_x(Length::Fill)
                    .padding(iced::Padding {
                        top: 11.0,
                        ..iced::Padding::ZERO
                    }),
            )
            .width(Length::Fill)
            .height(44)
            .padding(0)
            // Rounded, and inset from the popover's corners, so the hover fill stays inside.
            .style(styles::plain(p, 14.0))
            .on_press(Message::Tune(Msg::Tab(t)))
            .into()
        }))
        .spacing(4)
        .padding(iced::Padding {
            top: 8.0,
            right: 14.0,
            bottom: 0.0,
            left: 14.0,
        });
        let pane = match tab {
            Tab::Equalizer => self.eq_pane(p),
            Tab::Speed => self.speed_pane(p),
            Tab::Sleep => self.sleep_pane(p),
        };
        // A new tab's pane slides in from the side it lives on.
        let slide = 1.0 - self.anim.value("tune-pane", 1.0).clamp(0.0, 1.0);
        let dir = self.anim.value("tune-dir", 1.0);
        let shift = 28.0 * slide * dir;
        let popover = container(column![
            tabs,
            container(pane)
                .padding(iced::Padding {
                    top: 16.0,
                    right: (20.0 - shift).max(0.0),
                    bottom: 20.0,
                    left: (20.0 + shift).max(0.0),
                })
                .clip(true)
        ])
        .width(372)
        // Unfolds upward from the bar while rising into place.
        .max_height(560.0 * ease_out(t))
        .clip(true)
        .style(styles::popup(p, 28.0));
        let bar = self.settings().bar_height as f32;
        let placed = container(mouse_area(popover).on_press(Message::Tune(Msg::Tab(tab))))
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                right: 244.0,
                bottom: bar + 16.0 + 18.0 - 16.0 * (1.0 - t),
                ..iced::Padding::ZERO
            });
        stack![
            mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Tune(Msg::Close)),
            placed
        ]
        .into()
    }
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn tab_index(t: Tab) -> f32 {
    match t {
        Tab::Equalizer => 0.0,
        Tab::Speed => 1.0,
        Tab::Sleep => 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_js() {
        assert_eq!(gain_label(3.0), "+3");
        assert_eq!(gain_label(-2.5), "-2.5");
        assert_eq!(gain_label(0.0), "0");
        assert_eq!(speed_label(1.0), "1.0×");
        assert_eq!(speed_label(1.5), "1.5×");
        assert_eq!(speed_label(1.25), "1.25×");
        assert_eq!(countdown(75), "1:15");
    }

    #[test]
    fn eq_edits_follow_the_pane() {
        let mut s = Settings::default();
        apply_eq(&mut s, &Msg::EqGain(2, 4.5));
        assert!(s.eq_on);
        assert_eq!(s.eq_preset, "Custom");
        assert_eq!(s.eq_gains[2], 4.5);
        apply_eq(&mut s, &Msg::EqPreset(1));
        assert_eq!(s.eq_gains, [7.0, 4.0, 0.0, 0.0, 0.0]);
        assert!(s.eq_on);
        apply_eq(&mut s, &Msg::EqPreset(0));
        assert!(!s.eq_on);
        assert_eq!(s.eq_preset, "Flat");
    }
}
