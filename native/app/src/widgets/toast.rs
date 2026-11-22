use std::time::{Duration, Instant};

use iced::widget::{column, container, text};
use iced::{Alignment, Color, Element, Length, Shadow, Vector};

use crate::anim::{Ease, OVERLAY_MS};
use crate::theme::{alpha, Palette};

/// Seconds a toast stays up.
pub const LIFETIME_MS: u64 = 2600;
/// Newest three are kept (`t.slice(-2)` + the new one).
pub const MAX: usize = 3;

/// A toast that is still on screen, with how far it has faded in (0..=1).
pub struct Shown {
    pub message: String,
    pub t: f32,
}

/// Toasts stay in the list for one fade-out after they expire.
pub fn visible(expires: Instant, now: Instant) -> bool {
    now < expires + Duration::from_millis(OVERLAY_MS)
}

/// Eased 0..=1 visibility: fades and rises in after birth, fades out after expiry.
pub fn progress(born: Instant, expires: Instant, now: Instant) -> f32 {
    let fade = Duration::from_millis(OVERLAY_MS).as_secs_f32();
    let enter = (now.saturating_duration_since(born).as_secs_f32() / fade).min(1.0);
    let exit = if now > expires {
        1.0 - (now - expires).as_secs_f32() / fade
    } else {
        1.0
    };
    Ease::Out.apply(enter.min(1.0)) * exit.clamp(0.0, 1.0)
}

pub fn view<'a, Message: 'a>(messages: &[Shown], p: Palette) -> Element<'a, Message> {
    let stack = column(messages.iter().map(|m| {
        let t = m.t;
        container(text(m.message.clone()).size(14).color(alpha(p.surface, t)))
            .padding([12, 20])
            .style(move |_| container::Style {
                background: Some(alpha(p.on_surface, t).into()),
                text_color: Some(alpha(p.surface, t)),
                border: iced::border::rounded(12),
                shadow: Shadow {
                    color: alpha(Color::BLACK, 0.5 * t),
                    offset: Vector::new(0.0, 6.0),
                    blur_radius: 20.0,
                },
                ..container::Style::default()
            })
            .into()
    }))
    .spacing(8)
    .align_x(Alignment::Center);
    // Newest toast sits lowest; each one rises 12 px into place as it fades in.
    let rise = messages.last().map_or(0.0, |m| 12.0 * (1.0 - m.t));
    container(stack)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            bottom: 128.0 - rise,
            ..iced::Padding::ZERO
        })
        .into()
}
