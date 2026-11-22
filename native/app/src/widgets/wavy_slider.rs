//! Port of `WavySlider.js`: a flat or wavy track with a pill thumb.

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, stroke, Action, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector};

use crate::theme::Palette;

pub const WAVELENGTH: f32 = 28.0;
pub const AMPLITUDE: f32 = 3.5;
pub const HEIGHT: f32 = 28.0;
const GAP: f32 = 7.0;
const MID: f32 = HEIGHT / 2.0;

pub struct WavySlider<'a, Message> {
    /// 0..=1
    pub fraction: f32,
    /// Wave height factor, 1 = full, 0.02 = flat.
    pub amplitude: f32,
    /// Horizontal scroll of the wave in pixels.
    pub phase: f32,
    pub palette: Palette,
    pub on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

#[derive(Default)]
pub struct State {
    held: bool,
}

pub fn wavy_slider<'a, Message: 'a>(
    fraction: f32,
    amplitude: f32,
    phase: f32,
    palette: Palette,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    Canvas::new(WavySlider {
        fraction: fraction.clamp(0.0, 1.0),
        amplitude,
        phase,
        palette,
        on_change: Box::new(on_change),
    })
    .width(Length::Fill)
    .height(HEIGHT)
    .into()
}

/// A flat slider (volume, or a seek bar with nothing loaded).
pub fn flat_slider<'a, Message: 'a>(
    fraction: f32,
    palette: Palette,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    wavy_slider(fraction, 0.0, 0.0, palette, on_change)
}

fn fraction_at(bounds: Rectangle, x: f32) -> f32 {
    if bounds.width <= 0.0 {
        return 0.0;
    }
    ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
}

impl<Message> canvas::Program<Message> for WavySlider<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                state.held = true;
                Some(
                    Action::publish((self.on_change)(fraction_at(bounds, bounds.x + pos.x)))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.held => Some(
                Action::publish((self.on_change)(fraction_at(bounds, position.x))).and_capture(),
            ),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.held => {
                state.held = false;
                Some(Action::request_redraw())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let w = bounds.width;
        let x = self.fraction * w;
        let active_end = (x - GAP).max(0.0);
        let rest_start = (x + GAP).min(w);
        let line = |color: Color| {
            Stroke::default()
                .with_width(4.0)
                .with_color(color)
                .with_line_cap(stroke::LineCap::Round)
        };

        if rest_start < w {
            let rest = Path::line(Point::new(rest_start, MID), Point::new(w, MID));
            frame.stroke(&rest, line(self.palette.outline_variant));
        }
        if active_end > 0.0 {
            let clip = Rectangle::new(Point::ORIGIN, Size::new(active_end, HEIGHT));
            frame.with_clip(clip, |f| {
                f.translate(Vector::new(0.0, MID));
                f.stroke(
                    &wave(w, self.amplitude, self.phase),
                    line(self.palette.primary),
                );
            });
        }
        let thumb_w = if state.held { 3.0 } else { 4.0 };
        let thumb = Path::rounded_rectangle(
            Point::new(x - 2.0, 3.0),
            Size::new(thumb_w, HEIGHT - 6.0),
            2.0.into(),
        );
        frame.fill(&thumb, self.palette.primary);
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _: &State,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// `M0 0 Q7 -2A 14 0 T28 0 …`, scrolled left by `phase` and flattened by `amplitude`.
fn wave(width: f32, amplitude: f32, phase: f32) -> Path {
    let half = WAVELENGTH / 2.0;
    let count = (width / half).ceil() as i32 + 3;
    let offset = -(phase.rem_euclid(WAVELENGTH));
    let k = amplitude.clamp(0.0, 1.0);
    Path::new(|p| {
        p.move_to(Point::new(offset, 0.0));
        for i in 0..count {
            let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
            let x0 = offset + half * i as f32;
            p.quadratic_curve_to(
                Point::new(x0 + half / 2.0, sign * 2.0 * AMPLITUDE * k),
                Point::new(x0 + half, 0.0),
            );
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_maps_to_a_clamped_fraction() {
        let b = Rectangle::new(Point::new(100.0, 0.0), Size::new(200.0, 28.0));
        assert_eq!(fraction_at(b, 100.0), 0.0);
        assert_eq!(fraction_at(b, 200.0), 0.5);
        assert_eq!(fraction_at(b, 999.0), 1.0);
        assert_eq!(fraction_at(b, -5.0), 0.0);
    }
}
