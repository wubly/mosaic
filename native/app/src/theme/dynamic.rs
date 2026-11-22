use std::collections::HashMap;
use std::time::{Duration, Instant};

use iced::Color;
use material_colors::color::Rgb;
use material_colors::image::extract_color;
#[allow(deprecated)]
use material_colors::palette::CorePalette;

use super::palette::Palette;

pub const DEFAULT_SEED: Rgb = Rgb {
    red: 0x67,
    green: 0x50,
    blue: 0xa4,
};
const TRANSITION: Duration = Duration::from_millis(900);

fn color(rgb: Rgb) -> Color {
    Color::from_rgb8(rgb.red, rgb.green, rgb.blue)
}

/// Port of `applyScheme`: Material You's dark scheme for `seed` (the tones come straight from
/// the JS). Always dark: a light scheme off bright artwork turned the whole window white.
#[allow(deprecated)] // `CorePalette` is what `themeFromSourceColor` builds on
pub fn palette_from_seed(seed: Rgb) -> Palette {
    let p = CorePalette::of(seed);
    let base = Palette::default();
    Palette {
        primary: color(p.primary.tone(80)),
        on_primary: color(p.primary.tone(20)),
        primary_container: color(p.primary.tone(30)),
        on_primary_container: color(p.primary.tone(90)),
        surface: color(p.neutral.tone(6)),
        on_surface: color(p.neutral.tone(90)),
        on_surface_variant: color(p.neutral_variant.tone(80)),
        surface_container: color(p.neutral.tone(12)),
        surface_container_high: color(p.neutral.tone(17)),
        surface_container_highest: color(p.neutral.tone(22)),
        outline: color(p.neutral_variant.tone(60)),
        outline_variant: color(p.neutral_variant.tone(30)),
        ..base
    }
}

/// `sourceColorFromImage` on RGBA pixels (callers downscale to ≤128 px first).
pub fn seed_from_rgba(rgba: &[u8]) -> Option<Rgb> {
    let pixels: Vec<Rgb> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] == 255)
        .map(|p| Rgb {
            red: p[0],
            green: p[1],
            blue: p[2],
        })
        .collect();
    (!pixels.is_empty()).then(|| extract_color(&pixels))
}

/// CSS `ease`: cubic-bezier(.25, .1, .25, 1).
pub fn css_ease(t: f32) -> f32 {
    crate::anim::cubic_bezier(0.25, 0.1, 0.25, 1.0, t)
}

/// The palette currently on screen, animating between targets.
pub struct DynamicTheme {
    from: Palette,
    to: Palette,
    started: Instant,
    cache: HashMap<String, Palette>,
    animating: bool,
    pub current: Palette,
}

impl Default for DynamicTheme {
    fn default() -> Self {
        let base = Palette::default();
        DynamicTheme {
            from: base,
            to: base,
            started: Instant::now(),
            cache: HashMap::new(),
            animating: false,
            current: base,
        }
    }
}

impl DynamicTheme {
    pub fn cached(&self, key: &str) -> Option<Palette> {
        self.cache.get(key).copied()
    }

    pub fn remember(&mut self, key: String, palette: Palette) {
        self.cache.insert(key, palette);
    }

    pub fn transition_to(&mut self, target: Palette, now: Instant) {
        if target == self.to {
            return;
        }
        self.from = self.current;
        self.to = target;
        self.started = now;
        self.animating = !crate::anim::reduced_motion();
        if !self.animating {
            self.current = target;
        }
    }

    pub fn is_animating(&self) -> bool {
        self.animating
    }

    pub fn tick(&mut self, now: Instant) {
        if !self.animating {
            return;
        }
        let t =
            now.saturating_duration_since(self.started).as_secs_f32() / TRANSITION.as_secs_f32();
        if t >= 1.0 {
            self.current = self.to;
            self.animating = false;
        } else {
            self.current = self.from.lerp(&self.to, css_ease(t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_seed_generates_a_dark_scheme() {
        let p = palette_from_seed(DEFAULT_SEED);
        assert!(p.surface.r < 0.1 && p.surface.g < 0.1 && p.surface.b < 0.1);
        assert!(p.on_surface.r > 0.8);
        assert!(p.primary.r + p.primary.g + p.primary.b > 1.5);
        assert_eq!(p.error, Palette::default().error);
    }

    #[test]
    fn seed_picks_the_dominant_colour() {
        let mut rgba = Vec::new();
        for _ in 0..400 {
            rgba.extend_from_slice(&[220, 30, 40, 255]);
        }
        let seed = seed_from_rgba(&rgba).expect("seed");
        assert!(seed.red > 150 && seed.green < 100, "got {seed:?}");
        assert!(seed_from_rgba(&[]).is_none());
    }

    #[test]
    fn transition_runs_for_900ms_then_settles() {
        let mut t = DynamicTheme::default();
        let start = Instant::now();
        let target = palette_from_seed(Rgb {
            red: 200,
            green: 30,
            blue: 40,
        });
        t.transition_to(target, start);
        assert!(t.is_animating());
        t.tick(start + Duration::from_millis(450));
        assert!(t.is_animating());
        assert_ne!(t.current, target);
        t.tick(start + Duration::from_millis(950));
        assert!(!t.is_animating());
        assert_eq!(t.current, target);
    }
}
