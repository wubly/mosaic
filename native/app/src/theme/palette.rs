use iced::Color;

/// The `--md-*` custom properties from `globals.css` `:root`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub primary: Color,
    pub on_primary: Color,
    pub primary_container: Color,
    pub on_primary_container: Color,
    pub surface: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub surface_container: Color,
    pub surface_container_high: Color,
    pub surface_container_highest: Color,
    pub outline: Color,
    pub outline_variant: Color,
    pub error: Color,
    pub error_container: Color,
    pub on_error_container: Color,
}

const fn hex(v: u32) -> Color {
    Color {
        r: ((v >> 16) & 0xff) as f32 / 255.0,
        g: ((v >> 8) & 0xff) as f32 / 255.0,
        b: (v & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

impl Default for Palette {
    fn default() -> Self {
        Palette {
            primary: hex(0xd0bcff),
            on_primary: hex(0x381e72),
            primary_container: hex(0x4f378b),
            on_primary_container: hex(0xeaddff),
            surface: hex(0x141218),
            on_surface: hex(0xe6e0e9),
            on_surface_variant: hex(0xcac4d0),
            surface_container: hex(0x211f26),
            surface_container_high: hex(0x2b2930),
            surface_container_highest: hex(0x36343b),
            outline: hex(0x938f99),
            outline_variant: hex(0x49454f),
            error: hex(0xf2b8b5),
            error_container: hex(0x8c1d18),
            on_error_container: hex(0xf9dedc),
        }
    }
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Interpolates in linear sRGB (used for the palette transition).
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    let mix = |x: f32, y: f32| to_srgb(to_linear(x) + (to_linear(y) - to_linear(x)) * t);
    Color {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: a.a + (b.a - a.a) * t,
    }
}

/// CSS `color-mix(in srgb, a p%, b)`: premultiplied-alpha mix, `p` is `a`'s share.
pub fn mix(a: Color, b: Color, p: f32) -> Color {
    let alpha = a.a * p + b.a * (1.0 - p);
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |x: f32, y: f32| (x * a.a * p + y * b.a * (1.0 - p)) / alpha;
    Color {
        r: channel(a.r, b.r),
        g: channel(a.g, b.g),
        b: channel(a.b, b.b),
        a: alpha,
    }
}

pub fn alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

impl Palette {
    pub fn lerp(&self, to: &Palette, t: f32) -> Palette {
        let f = |a: Color, b: Color| lerp(a, b, t);
        Palette {
            primary: f(self.primary, to.primary),
            on_primary: f(self.on_primary, to.on_primary),
            primary_container: f(self.primary_container, to.primary_container),
            on_primary_container: f(self.on_primary_container, to.on_primary_container),
            surface: f(self.surface, to.surface),
            on_surface: f(self.on_surface, to.on_surface),
            on_surface_variant: f(self.on_surface_variant, to.on_surface_variant),
            surface_container: f(self.surface_container, to.surface_container),
            surface_container_high: f(self.surface_container_high, to.surface_container_high),
            surface_container_highest: f(
                self.surface_container_highest,
                to.surface_container_highest,
            ),
            outline: f(self.outline, to.outline),
            outline_variant: f(self.outline_variant, to.outline_variant),
            // Error colors don't follow the artwork.
            error: self.error,
            error_container: self.error_container,
            on_error_container: self.on_error_container,
        }
    }

    pub fn iced_theme(&self) -> iced::Theme {
        iced::Theme::custom(
            "Mosaic",
            iced::theme::Palette {
                background: self.surface,
                text: self.on_surface,
                primary: self.primary,
                success: self.primary,
                warning: self.primary,
                danger: self.error,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_matches_css_color_mix() {
        let a = hex(0xd0bcff);
        let b = hex(0x2b2930);
        let m = mix(a, b, 0.18);
        let near = |x: f32, y: f32| (x - y).abs() < 0.002;
        assert!(near(m.r, 0.18 * a.r + 0.82 * b.r));
        assert!(near(m.b, 0.18 * a.b + 0.82 * b.b));
        assert_eq!(m.a, 1.0);
    }

    #[test]
    fn mix_with_transparent_keeps_hue_and_scales_alpha() {
        let a = hex(0xe6e0e9);
        let m = mix(a, Color::TRANSPARENT, 0.1);
        assert!((m.a - 0.1).abs() < 1e-6);
        assert!((m.r - a.r).abs() < 1e-6);
        assert_eq!(
            mix(Color::TRANSPARENT, Color::TRANSPARENT, 0.5),
            Color::TRANSPARENT
        );
    }

    #[test]
    fn lerp_endpoints_and_midpoint_in_linear_space() {
        let (a, b) = (Color::BLACK, Color::WHITE);
        assert_eq!(lerp(a, b, 0.0), a);
        assert!((lerp(a, b, 1.0).r - 1.0).abs() < 1e-6);
        let mid = lerp(a, b, 0.5).r;
        assert!(
            (mid - 0.735).abs() < 0.01,
            "linear midpoint is ~0.735 in sRGB, got {mid}"
        );
    }
}
