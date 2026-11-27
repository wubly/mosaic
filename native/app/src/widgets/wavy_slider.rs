//! Port of `WavySlider.js`: a flat or wavy track with a pill thumb.
//!
//! While playing, the slider animates itself: on each redraw it moves the wave and advances the
//! position from where it was last told, then asks for the next redraw. That repaints without
//! running the app's update and view, so playback costs a redraw per frame, not a rebuild.

use std::time::Instant;

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, stroke, Action, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Event, Length, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::Palette;

pub const WAVELENGTH: f32 = 28.0;
pub const AMPLITUDE: f32 = 3.5;
pub const HEIGHT: f32 = 28.0;
const GAP: f32 = 7.0;
const MID: f32 = HEIGHT / 2.0;

/// How the slider moves on its own between rebuilds.
#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// When `fraction` was true.
    pub at: Instant,
    /// Fraction per second (0 when paused).
    pub rate: f32,
    /// Where the wave's scroll starts from.
    pub epoch: Instant,
}

pub struct WavySlider<'a, Message> {
    /// 0..=1
    pub fraction: f32,
    /// Wave height factor, 1 = full, 0.02 = flat.
    pub amplitude: f32,
    pub motion: Option<Motion>,
    pub palette: Palette,
    pub on_change: Box<dyn Fn(f32) -> Message + 'a>,
}

#[derive(Default)]
pub struct State {
    held: bool,
    /// The time of the latest redraw.
    now: Option<Instant>,
}

pub fn wavy_slider<'a, Message: 'a>(
    fraction: f32,
    amplitude: f32,
    motion: Option<Motion>,
    palette: Palette,
    on_change: impl Fn(f32) -> Message + 'a,
) -> Element<'a, Message> {
    Canvas::new(WavySlider {
        fraction: fraction.clamp(0.0, 1.0),
        amplitude,
        motion,
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
    wavy_slider(fraction, 0.0, None, palette, on_change)
}

fn fraction_at(bounds: Rectangle, x: f32) -> f32 {
    if bounds.width <= 0.0 {
        return 0.0;
    }
    ((x - bounds.x) / bounds.width).clamp(0.0, 1.0)
}

impl<Message> WavySlider<'_, Message> {
    /// Fraction and wave scroll at `now`.
    fn at(&self, now: Option<Instant>) -> (f32, f32) {
        match (self.motion, now) {
            (Some(m), Some(now)) => {
                let since = now.saturating_duration_since(m.at).as_secs_f32().min(0.5);
                let phase = now.saturating_duration_since(m.epoch).as_secs_f32() * WAVELENGTH;
                ((self.fraction + m.rate * since).min(1.0), phase)
            }
            (Some(m), None) => (
                self.fraction,
                m.at.saturating_duration_since(m.epoch).as_secs_f32() * WAVELENGTH,
            ),
            _ => (self.fraction, 0.0),
        }
    }
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
            Event::Window(iced::window::Event::RedrawRequested(now)) => {
                state.now = Some(*now);
                let moving = self.motion.is_some() && self.amplitude > 0.01;
                moving.then(|| Action::request_redraw_at(*now + crate::app::FRAME))
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
        let (fraction, phase) = self.at(state.now);
        let x = fraction * w;
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
            // Built only up to `active_end`: canvas clipping is unreliable on some backends.
            frame.stroke(
                &wave(active_end, self.amplitude, phase),
                line(self.palette.primary),
            );
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

/// The wave from x = 0 to `end`, centred on `MID`, scrolled left by `phase` and
/// flattened by `amplitude`. Sampled as a polyline so it stops exactly at `end`.
fn wave(end: f32, amplitude: f32, phase: f32) -> Path {
    let k = amplitude.clamp(0.0, 1.0) * AMPLITUDE;
    let y = |x: f32| MID - k * (std::f32::consts::TAU * (x + phase) / WAVELENGTH).sin();
    let steps = (end / 2.0).ceil().max(1.0) as i32;
    Path::new(|p| {
        p.move_to(Point::new(0.0, y(0.0)));
        for i in 1..=steps {
            let x = end * i as f32 / steps as f32;
            p.line_to(Point::new(x, y(x)));
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
