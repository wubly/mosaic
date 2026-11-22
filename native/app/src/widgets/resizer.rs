use iced::widget::{container, mouse_area, Space};
use iced::{mouse, Element, Length};

use crate::theme::{alpha, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

/// A 16 px strip with a pill that shows on hover or while dragging (`.rz`).
pub fn resizer<'a, Message: Clone + 'a>(
    axis: Axis,
    dragging: bool,
    p: Palette,
    on_press: Message,
    on_reset: Message,
) -> Element<'a, Message> {
    let (w, h) = match axis {
        Axis::Vertical => (4.0, 44.0),
        Axis::Horizontal => (44.0, 4.0),
    };
    let pill = container(Space::new().width(w).height(h)).style(move |_| container::Style {
        background: Some(alpha(p.outline, if dragging { 0.7 } else { 0.0 }).into()),
        border: iced::border::rounded(3),
        ..container::Style::default()
    });
    let strip = container(pill).center(Length::Fill);
    let (width, height) = match axis {
        Axis::Vertical => (Length::Fixed(16.0), Length::Fill),
        Axis::Horizontal => (Length::Fill, Length::Fixed(16.0)),
    };
    mouse_area(container(strip).width(width).height(height))
        .on_press(on_press)
        .on_double_click(on_reset)
        .interaction(match axis {
            Axis::Vertical => mouse::Interaction::ResizingHorizontally,
            Axis::Horizontal => mouse::Interaction::ResizingVertically,
        })
        .into()
}
