//! Shared page chrome: header, empty state, tabs, play/shuffle buttons and the virtual list.

use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Alignment, Element, Length};

use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::icon::{icon, Icon};
use crate::widgets::logo;

pub const ROW_H: f32 = 54.0;
const OVERSCAN: usize = 8;

/// Rows `start..end` to render for a list whose top is `list_top` px down the scrolled content.
pub fn visible_range(total: usize, scroll_y: f32, view_h: f32, list_top: f32) -> (usize, usize) {
    let past = (scroll_y - list_top).max(0.0);
    let start = ((past / ROW_H).floor() as usize).saturating_sub(OVERSCAN);
    let end = (((past + view_h) / ROW_H).ceil() as usize + OVERSCAN).min(total);
    (start.min(end), end)
}

pub fn primary_button<'a>(
    label: &str,
    glyph: Icon,
    p: Palette,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(
            row![icon(glyph, 20.0), text(label.to_string()).size(14)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .height(40)
    .padding([0, 24])
    .style(styles::primary(p))
    .on_press(msg)
    .into()
}

pub fn tonal_button<'a>(
    label: &str,
    glyph: Icon,
    p: Palette,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(
            row![icon(glyph, 20.0), text(label.to_string()).size(14)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .height(40)
    .padding([0, 20])
    .style(styles::tonal(p))
    .on_press(msg)
    .into()
}

pub fn ghost_button<'a>(
    label: &str,
    glyph: Icon,
    p: Palette,
    msg: Message,
) -> Element<'a, Message> {
    button(
        container(
            row![icon(glyph, 20.0), text(label.to_string()).size(14)]
                .spacing(8)
                .align_y(Alignment::Center),
        )
        .center_y(Length::Fill),
    )
    .height(40)
    .padding([0, 12])
    .style(styles::ghost(p))
    .on_press(msg)
    .into()
}

pub fn icon_button<'a>(glyph: Icon, tip: &str, p: Palette, msg: Message) -> Element<'a, Message> {
    let b = button(container(icon(glyph, 20.0)).center(Length::Fill))
        .width(40)
        .height(40)
        .padding(0)
        .style(styles::icon_button(p, false))
        .on_press(msg);
    iced::widget::tooltip(
        b,
        container(text(tip.to_string()).size(12))
            .padding([4, 8])
            .style(styles::popup(p, 8.0)),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

pub fn empty_state<'a>(
    glyph: Icon,
    title: &str,
    hint: &str,
    extra: Option<Element<'a, Message>>,
    p: Palette,
) -> Element<'a, Message> {
    let mut c = column![
        container(icon(glyph, 32.0).color(p.on_primary_container))
            .center(72)
            .style(styles::fill(p.primary_container, 24.0)),
        text(title.to_string()).size(18),
        text(hint.to_string()).size(14).color(p.on_surface_variant),
    ]
    .spacing(6)
    .align_x(Alignment::Center);
    if let Some(e) = extra {
        c = c.push(container(e).padding(iced::Padding {
            top: 16.0,
            ..iced::Padding::ZERO
        }));
    }
    container(c).center_x(Length::Fill).padding([64, 0]).into()
}

/// The standard scrolling page body (`.library`).
pub fn page<'a>(content: impl Into<Element<'a, Message>>, p: Palette) -> Element<'a, Message> {
    scrollable(
        container(content)
            .padding(iced::Padding {
                top: 32.0,
                right: 40.0,
                bottom: 24.0,
                left: 40.0,
            })
            .width(Length::Fill),
    )
    .on_scroll(|v| Message::Scrolled(v.absolute_offset().y, v.bounds().height))
    .style(styles::scrollbar(p))
    .height(Length::Fill)
    .into()
}

/// Lays out only `start..end` of `total` fixed-height rows between two spacers.
pub fn virtual_rows<'a>(
    total: usize,
    range: (usize, usize),
    build: impl Fn(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let (start, end) = range;
    let mut col = column![Space::new().height(start as f32 * ROW_H)];
    for i in start..end {
        col = col.push(container(build(i)).height(ROW_H).center_y(ROW_H));
    }
    col.push(Space::new().height(total.saturating_sub(end) as f32 * ROW_H))
        .into()
}

impl App {
    /// Width of the main glass panel, for choosing between side-by-side and stacked layouts.
    pub(crate) fn main_width(&self) -> f32 {
        let side = if self.settings().lyrics_open {
            self.settings().side_width as f32 + 16.0
        } else {
            0.0
        };
        self.size.width - 32.0 - 96.0 - side
    }

    pub(crate) fn page_header<'a>(
        &self,
        title: String,
        subtitle: Option<String>,
        actions: Vec<Element<'a, Message>>,
        p: Palette,
    ) -> Element<'a, Message> {
        let mut left = column![
            row![logo::mark(18.0), text("Mosaic").size(14).color(p.primary)]
                .spacing(8)
                .align_y(Alignment::Center),
            text(title).size(28),
        ]
        .spacing(2);
        if let Some(s) = subtitle {
            left = left.push(text(s).size(14).color(p.on_surface_variant));
        }
        let actions = iced::widget::Row::with_children(actions)
            .spacing(8)
            .align_y(Alignment::Center)
            .wrap();
        let pad = iced::Padding {
            bottom: 24.0,
            ..iced::Padding::ZERO
        };
        if self.main_width() < 760.0 {
            column![
                left,
                container(actions).padding(iced::Padding {
                    top: 12.0,
                    ..iced::Padding::ZERO
                })
            ]
            .padding(pad)
            .into()
        } else {
            row![left, Space::new().width(Length::Fill), actions]
                .align_y(Alignment::End)
                .padding(pad)
                .into()
        }
    }

    pub(crate) fn tabs<'a>(
        &self,
        labels: &[(&'static str, bool)],
        on: impl Fn(usize) -> Message,
        p: Palette,
    ) -> Element<'a, Message> {
        let mut r = row![].spacing(0);
        for (i, (label, active)) in labels.iter().enumerate() {
            let color = if *active {
                p.primary
            } else {
                p.on_surface_variant
            };
            r = r.push(
                button(
                    column![
                        container(text(*label).size(14).color(color)).center_y(Length::Fill),
                        container(Space::new().height(3))
                            .width(Length::Fill)
                            .style(styles::fill(
                                if *active {
                                    p.primary
                                } else {
                                    iced::Color::TRANSPARENT
                                },
                                2.0
                            )),
                    ]
                    .width(Length::Shrink)
                    .height(Length::Fill),
                )
                .height(48)
                .padding([0, 24])
                .style(styles::plain(p, 0.0))
                .on_press(on(i)),
            );
        }
        column![
            r,
            container(Space::new().height(1))
                .width(Length::Fill)
                .style(styles::fill(p.outline_variant, 0.0))
        ]
        .padding(iced::Padding {
            bottom: 28.0,
            ..iced::Padding::ZERO
        })
        .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_range_covers_the_viewport_plus_overscan() {
        assert_eq!(visible_range(1000, 0.0, 540.0, 200.0), (0, 18));
        let (s, e) = visible_range(1000, 5000.0, 540.0, 200.0);
        assert!(s <= (4800.0 / ROW_H) as usize - 8 + 1 && e >= ((4800.0 + 540.0) / ROW_H) as usize);
        assert!(e - s < 40);
        assert_eq!(visible_range(5, 99999.0, 500.0, 0.0), (5, 5));
        assert_eq!(visible_range(0, 0.0, 500.0, 0.0), (0, 0));
    }
}
