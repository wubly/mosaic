use iced::widget::{button, container, Space};
use iced::{Element, Length};

use crate::theme::{styles, Palette};

/// `.switch`: 52×32 track with a knob that grows when on.
pub fn switch<'a, Message: Clone + 'a>(
    on: bool,
    p: Palette,
    on_toggle: Message,
) -> Element<'a, Message> {
    let (knob, color) = if on {
        (24.0, p.on_primary)
    } else {
        (16.0, p.outline)
    };
    let knob =
        container(Space::new().width(knob).height(knob)).style(styles::fill(color, knob / 2.0));
    let track = container(knob)
        .width(52)
        .height(32)
        .padding([0, if on { 3 } else { 6 }])
        .align_x(if on {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        })
        .align_y(iced::alignment::Vertical::Center)
        .style(move |_| container::Style {
            background: Some(
                if on {
                    p.primary
                } else {
                    p.surface_container_highest
                }
                .into(),
            ),
            border: iced::Border {
                color: if on { p.primary } else { p.outline },
                width: 2.0,
                radius: 16.0.into(),
            },
            ..container::Style::default()
        });
    button(track)
        .padding(0)
        .width(Length::Shrink)
        .style(styles::bare(p))
        .on_press(on_toggle)
        .into()
}
