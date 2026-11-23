//! Port of `NowPlaying.js` and `MiniPlayer.js`.

use iced::widget::{button, column, container, row, stack, text, Space};
use iced::{Alignment, Element, Length, Task};

use super::lyrics::Variant;
use crate::app::{App, Message};
use crate::theme::{alpha, mix, styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::visualizer::{is_silent, smooth, visualizer, BARS};

pub const MINI_SIZE: iced::Size = iced::Size::new(380.0, 660.0);
pub const MINI_MIN: iced::Size = iced::Size::new(340.0, 440.0);
pub const NORMAL_MIN: iced::Size = iced::Size::new(980.0, 640.0);

impl App {
    /// Updates the bar levels from the analyser; returns whether anything is still moving.
    pub(crate) fn viz_tick(&mut self) {
        let want = self.ui.now_playing;
        if !want
            || self
                .now
                .saturating_duration_since(self.viz_last)
                .as_millis()
                < 32
        {
            return;
        }
        self.viz_last = self.now;
        let playing = self.player.playing;
        let mut data = [0u8; mosaic_audio::ANALYSER_BINS];
        if let Some(a) = &mut self.analyser {
            a.byte_frequency_data(&mut data);
        }
        for i in 0..BARS {
            let target = if playing {
                f32::from(data[crate::widgets::visualizer::bin_for(i).min(data.len() - 1)]) / 255.0
            } else {
                0.0
            };
            self.viz[i] = smooth(self.viz[i], target);
        }
    }

    pub(crate) fn viz_active(&self) -> bool {
        self.ui.now_playing && (self.player.playing || !is_silent(&self.viz))
    }

    pub(crate) fn toggle_mini(&mut self) -> Task<Message> {
        self.ui.mini = !self.ui.mini;
        let id = self.main;
        if self.ui.mini {
            self.normal_size = self.size;
            self.ui.now_playing = false;
            self.ui.youtube.video = None;
            Task::batch([
                iced::window::set_min_size(id, Some(MINI_MIN)),
                iced::window::resize(id, MINI_SIZE),
                iced::window::set_level(id, iced::window::Level::AlwaysOnTop),
            ])
        } else {
            Task::batch([
                iced::window::set_level(id, iced::window::Level::Normal),
                iced::window::set_min_size(id, Some(NORMAL_MIN)),
                iced::window::resize(id, self.normal_size),
            ])
        }
    }

    pub(crate) fn now_playing_view(&self, p: Palette) -> Element<'_, Message> {
        let Some(track) = &self.player.track else {
            return Space::new().into();
        };
        let art_size = (self.size.height * 0.44)
            .min((self.size.width - 128.0) / 2.15)
            .max(120.0);
        let left = column![
            self.art(&track.artwork, art_size, 28.0),
            text(track.title.clone())
                .size(28)
                .line_height(iced::Pixels(36.0))
                .align_x(iced::alignment::Horizontal::Center),
            text(track.artist.clone())
                .size(16)
                .color(p.on_surface_variant),
            container(crate::views::track::provider_tag(
                track,
                13.0,
                p.on_surface_variant
            ))
            .padding([4, 0]),
            container(visualizer::<Message>(self.viz, p.primary, 64.0))
                .width(art_size)
                .padding(iced::Padding {
                    top: 12.0,
                    ..iced::Padding::ZERO
                }),
        ]
        .spacing(4)
        .align_x(Alignment::Center);
        let right = container(self.with_picker(self.lyrics_body(Variant::Large, p), p))
            .width(Length::Fill)
            .height(Length::Fill);
        let body = row![
            container(left)
                .width(Length::FillPortion(100))
                .center(Length::Fill),
            right,
        ]
        .spacing(56)
        .padding(iced::Padding {
            top: 16.0,
            right: 64.0,
            bottom: 0.0,
            left: 64.0,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let controls = container(container(self.controls_view(p)).max_width(720))
            .center_x(Length::Fill)
            .padding(iced::Padding {
                top: 8.0,
                right: 24.0,
                bottom: 28.0,
                left: 24.0,
            });
        let close = container(
            button(container(icon(Icon::ArrowDown, 30.0)).center(Length::Fill))
                .width(40)
                .height(40)
                .padding(0)
                .style(styles::icon_button(p, false))
                .on_press(Message::CloseNowPlaying),
        )
        .padding(iced::Padding {
            top: 48.0,
            left: 24.0,
            ..iced::Padding::ZERO
        });
        let content = column![body, controls].padding(iced::Padding {
            top: 40.0,
            ..iced::Padding::ZERO
        });
        stack![
            self.ambient_view(p),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(styles::fill(
                    alpha(mix(p.surface, iced::Color::TRANSPARENT, 0.62), 0.62),
                    0.0
                )),
            content,
            close,
        ]
        .into()
    }

    pub(crate) fn mini_view(&self, p: Palette) -> Element<'_, Message> {
        let content: Element<'_, Message> = match &self.player.track {
            Some(t) => column![
                container(self.art(&t.artwork, 210.0, 24.0)).center_x(Length::Fill),
                column![
                    text(t.title.clone())
                        .size(18)
                        .wrapping(text::Wrapping::None),
                    text(t.artist.clone()).size(14).color(p.on_surface_variant),
                    crate::views::track::provider_tag(t, 12.0, p.on_surface_variant),
                ]
                .spacing(2)
                .align_x(Alignment::Center)
                .width(Length::Fill),
                self.lyrics_body(Variant::Bare, p),
                self.controls_view(p),
            ]
            .spacing(10)
            .align_x(Alignment::Center)
            .into(),
            None => column![
                container(text("Nothing playing yet.").color(p.on_surface_variant))
                    .center_x(Length::Fill)
                    .padding(iced::Padding {
                        top: 80.0,
                        ..iced::Padding::ZERO
                    }),
                Space::new().height(Length::Fill),
                self.controls_view(p),
            ]
            .into(),
        };
        stack![
            self.ambient_view(p),
            container(content).padding(iced::Padding {
                top: 48.0,
                right: 20.0,
                bottom: 16.0,
                left: 20.0
            }),
        ]
        .into()
    }
}
