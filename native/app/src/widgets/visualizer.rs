//! Port of `Visualizer.js`: 40 mirrored bars driven by the analyser.

use iced::mouse::Cursor;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Size, Theme};

pub const BARS: usize = 40;
const GAP: f32 = 4.0;

struct Viz {
    levels: [f32; BARS],
    color: Color,
}

/// One smoothing step: attack 0.5, release 0.12.
pub fn smooth(level: f32, target: f32) -> f32 {
    level + (target - level) * if target > level { 0.5 } else { 0.12 }
}

/// FFT bin read by bar `i`: the spectrum is mirrored around the centre.
pub fn bin_for(i: usize) -> usize {
    let half = BARS as f32 / 2.0;
    (((i as f32 - half + 0.5).abs() / half).powf(1.4) * 110.0).floor() as usize + 1
}

pub fn is_silent(levels: &[f32; BARS]) -> bool {
    levels.iter().all(|l| *l < 0.01)
}

pub fn visualizer<'a, Message: 'a>(
    levels: [f32; BARS],
    color: Color,
    height: f32,
) -> Element<'a, Message> {
    Canvas::new(Viz { levels, color })
        .width(Length::Fill)
        .height(height)
        .into()
}

impl<Message> canvas::Program<Message> for Viz {
    type State = ();

    fn draw(
        &self,
        _: &(),
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let bw = (w - GAP * (BARS as f32 - 1.0)) / BARS as f32;
        for (i, level) in self.levels.iter().enumerate() {
            let bh = (level * h).max(4.0);
            let x = i as f32 * (bw + GAP);
            let bar = Path::rounded_rectangle(
                Point::new(x, (h - bh) / 2.0),
                Size::new(bw, bh),
                (bw / 2.0).into(),
            );
            frame.fill(
                &bar,
                Color {
                    a: 0.45 + level * 0.55,
                    ..self.color
                },
            );
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bins_mirror_around_the_centre() {
        assert_eq!(bin_for(19), bin_for(20));
        assert_eq!(bin_for(0), bin_for(39));
        assert!(bin_for(0) > bin_for(19));
        assert_eq!(
            bin_for(19),
            1 + ((0.5f32 / 20.0).powf(1.4) * 110.0).floor() as usize
        );
    }

    #[test]
    fn smoothing_attacks_fast_and_releases_slowly() {
        assert!((smooth(0.0, 1.0) - 0.5).abs() < 1e-6);
        assert!((smooth(1.0, 0.0) - 0.88).abs() < 1e-6);
        assert!(is_silent(&[0.0; BARS]));
    }
}
