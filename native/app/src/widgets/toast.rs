use iced::widget::{column, container, text};
use iced::{Alignment, Element, Length};

use crate::theme::{styles, Palette};

/// Seconds a toast stays up.
pub const LIFETIME_MS: u64 = 2600;
/// Newest three are kept (`t.slice(-2)` + the new one).
pub const MAX: usize = 3;

pub fn view<'a, Message: 'a>(messages: &[(u64, String)], p: Palette) -> Element<'a, Message> {
    let stack = column(messages.iter().map(|(_, m)| {
        container(text(m.clone()).size(14))
            .padding([12, 20])
            .style(styles::toast(p))
            .into()
    }))
    .spacing(8)
    .align_x(Alignment::Center);
    container(stack)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            bottom: 128.0,
            ..iced::Padding::ZERO
        })
        .into()
}
