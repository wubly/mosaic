//! One function per CSS class in `globals.css`.

use iced::widget::{button, container, scrollable, text};
use iced::{border, Background, Border, Color, Shadow, Theme, Vector};

use super::palette::{alpha, mix, Palette};

fn bg(c: Color) -> Option<Background> {
    Some(Background::Color(c))
}

fn round(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

/// Stand-in for `filter: brightness(x)`.
fn brighten(c: Color, amount: f32) -> Color {
    mix(Color::WHITE, c, amount)
}

pub fn primary(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let (background, text_color) = match status {
            button::Status::Disabled => (alpha(p.primary, 0.38), alpha(p.on_primary, 0.38)),
            button::Status::Hovered | button::Status::Pressed => {
                (brighten(p.primary, 0.08), p.on_primary)
            }
            button::Status::Active => (p.primary, p.on_primary),
        };
        button::Style {
            background: bg(background),
            text_color,
            border: round(20.0),
            ..button::Style::default()
        }
    }
}

pub fn tonal(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let base = mix(p.primary, p.surface_container_high, 0.18);
        let fill = match status {
            button::Status::Hovered | button::Status::Pressed => brighten(base, 0.1),
            _ => base,
        };
        button::Style {
            background: bg(fill),
            text_color: p.on_surface,
            border: round(20.0),
            ..button::Style::default()
        }
    }
}

pub fn ghost(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| Background::Color(mix(p.primary, Color::TRANSPARENT, 0.08))),
        text_color: p.primary,
        border: round(20.0),
        ..button::Style::default()
    }
}

/// `.icon`, `.icon.on`.
pub fn icon_button(p: Palette, on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, color) = if on {
            (Some(p.primary_container), p.on_primary_container)
        } else if hovered {
            (
                Some(mix(p.on_surface, Color::TRANSPARENT, 0.10)),
                p.on_surface,
            )
        } else {
            (None, p.on_surface_variant)
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: color,
            border: round(20.0),
            ..button::Style::default()
        }
    }
}

/// Icon button tinted with the primary colour (`.icon.like.on-like`).
pub fn icon_button_accent(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let mut style = icon_button(p, false)(theme, status);
        style.text_color = p.primary;
        style
    }
}

pub fn chip(p: Palette, on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, color) = if on {
            (Some(p.primary_container), p.on_primary_container)
        } else if hovered {
            (
                Some(mix(p.on_surface, Color::TRANSPARENT, 0.10)),
                p.on_surface,
            )
        } else {
            (None, p.on_surface_variant)
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: color,
            border: round(16.0),
            ..button::Style::default()
        }
    }
}

/// A button that only shows a hover wash.
pub fn plain(p: Palette, radius: f32) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: matches!(status, button::Status::Hovered | button::Status::Pressed)
            .then(|| Background::Color(mix(p.on_surface, Color::TRANSPARENT, 0.08))),
        text_color: p.on_surface,
        border: round(radius),
        ..button::Style::default()
    }
}

/// Rail item: no chrome of its own; the pill is drawn by the rail.
pub fn bare(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, _| button::Style {
        background: None,
        text_color: p.on_surface,
        border: Border::default(),
        ..button::Style::default()
    }
}

pub fn window_button(p: Palette, close: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let background = hovered.then(|| {
            if close {
                Background::Color(Color::from_rgb8(0xc4, 0x2b, 0x1c))
            } else {
                Background::Color(Color::from_rgba8(0xff, 0xff, 0xff, 0.1))
            }
        });
        button::Style {
            background,
            text_color: if close && hovered {
                Color::WHITE
            } else {
                p.on_surface_variant
            },
            border: Border::default(),
            ..button::Style::default()
        }
    }
}

/// `.glass`: translucent surface; the ambient image behind it is already blurred.
pub fn glass(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(mix(p.surface, Color::TRANSPARENT, 0.58)),
        border: Border {
            color: Color::from_rgba8(0xff, 0xff, 0xff, 13.0 / 255.0),
            width: 1.0,
            radius: 28.0.into(),
        },
        text_color: Some(p.on_surface),
        ..container::Style::default()
    }
}

pub fn surface(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.surface),
        text_color: Some(p.on_surface),
        ..container::Style::default()
    }
}

pub fn card(p: Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.surface_container_high),
        border: round(radius),
        text_color: Some(p.on_surface),
        ..container::Style::default()
    }
}

pub fn popup(p: Palette, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.surface_container_high),
        border: Border {
            color: Color::from_rgba8(0xff, 0xff, 0xff, 18.0 / 255.0),
            width: 1.0,
            radius: radius.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.55),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 40.0,
        },
        text_color: Some(p.on_surface),
        ..container::Style::default()
    }
}

pub fn error_box(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.error_container),
        text_color: Some(p.on_error_container),
        border: round(12.0),
        ..container::Style::default()
    }
}

pub fn kbd(p: Palette) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(p.surface_container_highest),
        border: Border {
            color: p.outline_variant,
            width: 1.0,
            radius: 6.0.into(),
        },
        text_color: Some(p.on_surface),
        ..container::Style::default()
    }
}

pub fn fill(color: Color, radius: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: bg(color),
        border: round(radius),
        ..container::Style::default()
    }
}

pub fn muted(p: Palette) -> impl Fn(&Theme) -> text::Style {
    move |_| text::Style {
        color: Some(p.on_surface_variant),
    }
}

pub fn colored(color: Color) -> impl Fn(&Theme) -> text::Style {
    move |_| text::Style { color: Some(color) }
}

pub fn error_text(p: Palette) -> impl Fn(&Theme) -> text::Style {
    colored(p.error)
}

/// 10 px scrollbar, thumb `#ffffff1f`, radius 5.
pub fn scrollbar(_: Palette) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(Color::from_rgba8(0xff, 0xff, 0xff, 31.0 / 255.0)),
                border: border::rounded(5),
            },
        };
        style.vertical_rail = rail;
        style.horizontal_rail = rail;
        style.container = container::Style::default();
        style
    }
}
