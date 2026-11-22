//! Port of `PlayerBar.js` and `Controls.js`.

use iced::widget::{button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Length};
use mosaic_core::format::fmt_secs;
use mosaic_core::model::Repeat;

use super::artwork_of;
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, outlined, Icon};
use crate::widgets::spinner::spinner;
use crate::widgets::wavy_slider::{flat_slider, wavy_slider, WAVELENGTH};

fn tip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: String,
    p: Palette,
) -> Element<'a, Message> {
    tooltip(
        content,
        container(text(label).size(12))
            .padding([4, 8])
            .style(styles::popup(p, 8.0)),
        tooltip::Position::Top,
    )
    .gap(6)
    .into()
}

fn icon_btn<'a>(
    glyph: iced::widget::Text<'a>,
    label: String,
    on: bool,
    p: Palette,
    msg: Message,
) -> Element<'a, Message> {
    tip(
        button(container(glyph).center(Length::Fill))
            .width(40)
            .height(40)
            .padding(0)
            .style(styles::icon_button(p, on))
            .on_press(msg),
        label,
        p,
    )
}

/// What the seek bar shows: positions relative to the silence trim (`Controls.js`).
pub fn shown_times(current: f64, duration: f64, offset: f64, tail: f64) -> (f64, f64) {
    let shown_duration = (duration - offset - tail).max(0.0);
    let cap = if shown_duration > 0.0 {
        shown_duration
    } else {
        f64::INFINITY
    };
    ((current - offset).max(0.0).min(cap), shown_duration)
}

impl App {
    /// Current position with the engine's 100 ms steps smoothed while playing.
    pub(crate) fn display_position(&self) -> f64 {
        let t = self.player.time;
        if self.player.playing && !self.player.loading {
            let ahead = self
                .now
                .saturating_duration_since(self.pos_at)
                .as_secs_f64()
                .min(0.5);
            let end = if t.duration > 0.0 {
                t.duration
            } else {
                f64::INFINITY
            };
            (t.current + ahead * self.settings().speed).min(end)
        } else {
            t.current
        }
    }

    pub(crate) fn player_bar(&self, p: Palette) -> Element<'_, Message> {
        let bar_h = self.settings().bar_height as f32;
        let volume = self.settings().volume;
        let volume_glyph = if volume == 0.0 {
            Icon::VolumeOff
        } else if volume < 0.5 {
            Icon::VolumeDown
        } else {
            Icon::VolumeUp
        };
        let mute_label = if volume == 0.0 {
            "Unmute (M)"
        } else {
            "Mute (M)"
        };
        let volume_row = row![
            icon_btn(
                icon(volume_glyph, 22.0),
                mute_label.into(),
                false,
                p,
                Message::Player(PlayerInput::SetVolume(if volume == 0.0 {
                    0.8
                } else {
                    0.0
                })),
            ),
            container(flat_slider(volume as f32, p, Message::VolumeFraction)).width(104),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        let Some(track) = self.player.track.clone() else {
            return self.idle_bar(p, volume_row);
        };

        let thumb = (bar_h - 36.0).clamp(44.0, 120.0);
        let liked = self.store.is_liked(&track.id);
        let artist: Element<'_, Message> = if self.player.error.is_empty() {
            text(track.artist.clone())
                .size(14)
                .style(styles::muted(p))
                .wrapping(text::Wrapping::None)
                .into()
        } else {
            text(self.player.error.clone())
                .size(14)
                .style(styles::error_text(p))
                .wrapping(text::Wrapping::None)
                .into()
        };
        let thumb_btn = tip(
            button(self.art(artwork_of(&track), thumb, 16.0))
                .padding(0)
                .style(styles::bare(p))
                .on_press(Message::OpenNowPlaying),
            "Open Now Playing (F)".into(),
            p,
        );
        let now = row![
            thumb_btn,
            container(column![
                container(
                    text(track.title.clone())
                        .size(16)
                        .wrapping(text::Wrapping::None)
                )
                .clip(true),
                container(artist).clip(true),
            ])
            .width(Length::Fill),
            icon_btn(
                if liked {
                    icon(Icon::Favorite, 22.0)
                } else {
                    outlined(Icon::Favorite, 22.0)
                },
                if liked {
                    "Unlike (L)".into()
                } else {
                    "Like (L)".into()
                },
                false,
                p,
                Message::ToggleLike,
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center);

        let lyrics_open = self.settings().lyrics_open;
        let tools = row![
            icon_btn(
                icon(
                    if self.player.sleep != crate::player::Sleep::Off {
                        Icon::Bedtime
                    } else {
                        Icon::Tune
                    },
                    22.0
                ),
                "Equalizer, speed & sleep timer".into(),
                self.tune_button_on(),
                p,
                Message::Tune(crate::views::tune::Msg::Toggle),
            ),
            icon_btn(
                icon(Icon::PictureInPicture, 20.0),
                "Mini player (I)".into(),
                false,
                p,
                Message::ToggleMini
            ),
            icon_btn(
                if lyrics_open {
                    icon(Icon::Lyrics, 22.0)
                } else {
                    outlined(Icon::Lyrics, 22.0)
                },
                "Lyrics (Y)".into(),
                lyrics_open,
                p,
                Message::ToggleLyrics,
            ),
            container(volume_row).padding(iced::Padding {
                left: 8.0,
                ..iced::Padding::ZERO
            }),
        ]
        .spacing(4)
        .align_y(Alignment::Center);

        container(
            row![
                container(now).width(Length::FillPortion(10)),
                container(self.controls(p, bar_h <= 96.0)).width(Length::FillPortion(13)),
                container(tools)
                    .width(Length::FillPortion(10))
                    .align_x(iced::alignment::Horizontal::Right),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        )
        .padding([12, 24])
        .width(Length::Fill)
        .height(bar_h)
        .center_y(bar_h)
        .style(styles::glass(p))
        .into()
    }

    fn idle_bar<'a>(
        &'a self,
        p: Palette,
        volume_row: impl Into<Element<'a, Message>>,
    ) -> Element<'a, Message> {
        let bar_h = self.settings().bar_height as f32;
        let thumb = container(icon(Icon::MusicNote, 24.0).color(p.on_surface_variant))
            .center(56)
            .style(styles::fill(p.surface_container_highest, 16.0));
        let now = row![
            thumb,
            column![
                text("Nothing playing").size(16),
                text("Pick a track to get started")
                    .size(14)
                    .style(styles::muted(p)),
            ]
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        let disabled = |glyph: Icon, size: f32| {
            button(container(icon(glyph, size)).center(Length::Fill))
                .width(48)
                .height(48)
                .padding(0)
                .style(styles::icon_button(p, false))
        };
        let transport = row![
            disabled(Icon::SkipPrevious, 30.0),
            disabled(Icon::PlayArrow, 32.0),
            disabled(Icon::SkipNext, 30.0)
        ]
        .spacing(6)
        .align_y(Alignment::Center);
        let seek = row![
            container(text("0:00").size(12).style(styles::muted(p)))
                .width(36)
                .align_x(iced::alignment::Horizontal::Right),
            flat_slider(0.0, p, |_| Message::Nop),
            container(text("0:00").size(12).style(styles::muted(p))).width(36),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        container(
            row![
                container(now).width(Length::FillPortion(10)),
                container(
                    column![transport, seek]
                        .spacing(2)
                        .align_x(Alignment::Center)
                )
                .width(Length::FillPortion(13)),
                container(
                    container(volume_row)
                        .align_x(iced::alignment::Horizontal::Right)
                        .width(Length::Fill)
                )
                .width(Length::FillPortion(10)),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        )
        .padding([12, 24])
        .width(Length::Fill)
        .height(bar_h)
        .center_y(bar_h)
        .style(styles::glass(p))
        .into()
    }

    fn controls(&self, p: Palette, compact: bool) -> Element<'_, Message> {
        let s = self.settings();
        let pl = &self.player;
        let t = pl.time;
        let (current, duration) =
            shown_times(self.display_position(), t.duration, t.offset, t.tail);

        let radius = self.anim.value("play-radius", 18.0);
        let (pw, ph) = if compact { (52.0, 44.0) } else { (64.0, 56.0) };
        let glyph: Element<'_, Message> = if pl.loading {
            spinner(
                26.0,
                self.now
                    .saturating_duration_since(self.epoch)
                    .as_secs_f32()
                    .fract(),
                p.on_primary,
            )
        } else if pl.playing {
            icon(Icon::Pause, 32.0).color(p.on_primary).into()
        } else {
            icon(Icon::PlayArrow, 32.0).color(p.on_primary).into()
        };
        let play = tip(
            button(container(glyph).center(Length::Fill))
                .width(pw)
                .height(ph)
                .padding(0)
                .style(move |_, status| button::Style {
                    background: Some(
                        if matches!(status, button::Status::Hovered | button::Status::Pressed) {
                            crate::theme::mix(iced::Color::WHITE, p.primary, 0.08)
                        } else {
                            p.primary
                        }
                        .into(),
                    ),
                    text_color: p.on_primary,
                    border: iced::border::rounded(radius),
                    ..button::Style::default()
                })
                .on_press(Message::Player(PlayerInput::Toggle)),
            if pl.playing {
                "Pause".into()
            } else {
                "Play".into()
            },
            p,
        );
        let skip = |glyph: Icon, label: &str, input: PlayerInput| {
            let size = if compact { 40.0 } else { 48.0 };
            tip(
                button(container(icon(glyph, 30.0).color(p.on_surface)).center(Length::Fill))
                    .width(size)
                    .height(size)
                    .padding(0)
                    .style(styles::icon_button(p, false))
                    .on_press(Message::Player(input)),
                label.into(),
                p,
            )
        };
        let mut transport = row![].spacing(6).align_y(Alignment::Center);
        if !compact {
            transport = transport.push(icon_btn(
                icon(Icon::Shuffle, 22.0),
                "Shuffle (S)".into(),
                s.shuffle,
                p,
                Message::Player(PlayerInput::ToggleShuffle),
            ));
        }
        transport = transport
            .push(skip(Icon::SkipPrevious, "Previous", PlayerInput::Prev))
            .push(container(play).padding([0, 4]))
            .push(skip(Icon::SkipNext, "Next", PlayerInput::Next));
        if !compact {
            transport = transport.push(icon_btn(
                icon(
                    if s.repeat == Repeat::One {
                        Icon::RepeatOne
                    } else {
                        Icon::Repeat
                    },
                    22.0,
                ),
                format!("Repeat: {} (R)", repeat_name(s.repeat)),
                s.repeat != Repeat::Off,
                p,
                Message::Player(PlayerInput::CycleRepeat),
            ));
        }

        let fraction = if duration > 0.0 {
            (current / duration) as f32
        } else {
            0.0
        };
        let amp = self.anim.value("seek-amp", 1.0);
        let phase = self.now.saturating_duration_since(self.epoch).as_secs_f32() * WAVELENGTH;
        let seek = row![
            container(text(fmt_secs(current)).size(12).style(styles::muted(p)))
                .width(36)
                .align_x(iced::alignment::Horizontal::Right),
            wavy_slider(fraction, amp, phase, p, Message::SeekFraction),
            container(text(fmt_secs(duration)).size(12).style(styles::muted(p))).width(36),
        ]
        .spacing(12)
        .align_y(Alignment::Center);
        column![transport, seek]
            .spacing(if compact { 0 } else { 2 })
            .align_x(Alignment::Center)
            .width(Length::Fill)
            .into()
    }
}

fn repeat_name(r: Repeat) -> &'static str {
    match r {
        Repeat::Off => "off",
        Repeat::All => "all",
        Repeat::One => "one",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shown_times_exclude_the_trimmed_silence() {
        assert_eq!(shown_times(12.0, 200.0, 2.0, 10.0), (10.0, 188.0));
        assert_eq!(shown_times(1.0, 200.0, 2.0, 0.0), (0.0, 198.0));
        assert_eq!(shown_times(199.0, 200.0, 2.0, 10.0), (188.0, 188.0));
        assert_eq!(shown_times(5.0, 0.0, 0.0, 0.0), (5.0, 0.0));
    }
}
