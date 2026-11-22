//! Text measurement for the lyrics layout, using the same shaper the renderer uses.

use iced::advanced::graphics::text::Paragraph as Raw;
use iced::advanced::text::paragraph::Plain;
use iced::advanced::text::{Alignment, LineHeight, Shaping, Text, Wrapping};
use iced::{alignment, Font, Pixels, Size};

pub fn width(content: &str, size: f32, font: Font) -> f32 {
    let p: Plain<Raw> = Plain::new(Text {
        content: content.to_string(),
        bounds: Size::new(f32::INFINITY, f32::INFINITY),
        size: Pixels(size),
        line_height: LineHeight::default(),
        font,
        align_x: Alignment::Default,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
    });
    p.min_width()
}

/// Number of visual lines when words of the given widths are wrapped greedily into `avail`.
pub fn wrapped_lines(widths: &[f32], space: f32, avail: f32) -> usize {
    let mut lines = 1;
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        let need = if i == 0 || used == 0.0 {
            *w
        } else {
            space + *w
        };
        if used > 0.0 && used + need > avail {
            lines += 1;
            used = *w;
        } else {
            used += need;
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_greedily() {
        assert_eq!(wrapped_lines(&[], 4.0, 100.0), 1);
        assert_eq!(wrapped_lines(&[40.0, 40.0], 4.0, 100.0), 1);
        assert_eq!(wrapped_lines(&[40.0, 40.0, 40.0], 4.0, 100.0), 2);
        assert_eq!(wrapped_lines(&[150.0, 10.0], 4.0, 100.0), 2);
        assert_eq!(wrapped_lines(&[50.0; 6], 0.0, 100.0), 3);
    }

    #[test]
    fn wider_text_measures_wider() {
        let font = Font::DEFAULT;
        assert!(width("wide text here", 22.0, font) > width("a", 22.0, font));
    }
}
