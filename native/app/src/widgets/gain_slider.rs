//! Port of `GainSlider.js`: a vertical −12..+12 dB slider in 0.5 dB steps.

use iced::mouse::{self, Cursor};
use iced::widget::canvas::{self, Action, Canvas, Frame, Geometry, Path};
use iced::{Color, Element, Event, Point, Rectangle, Renderer, Size, Theme};

use crate::theme::Palette;

const W: f32 = 40.0;
const H: f32 = 132.0;
const TRACK: f32 = 6.0;
const HANDLE: f32 = 20.0;

struct Gain<'a, Message> {
    value: f32,
    enabled: bool,
    palette: Palette,
    on_change: Box<dyn Fn(f32) -> Message + 'a>,
    on_reset: Message,
}

#[derive(Default)]
struct State {
    held: bool,
}

pub fn gain_slider<'a, Message: Clone + 'a>(
    value: f32,
    enabled: bool,
    palette: Palette,
    on_change: impl Fn(f32) -> Message + 'a,
    on_reset: Message,
) -> Element<'a, Message> {
    Canvas::new(Gain {
        value,
        enabled,
        palette,
        on_change: Box::new(on_change),
        on_reset,
    })
    .width(W)
    .height(H)
    .into()
}

pub fn value_at(bounds: Rectangle, y: f32) -> f32 {
    let fraction = 1.0 - ((y - bounds.y) / bounds.height).clamp(0.0, 1.0);
    ((fraction * 24.0 - 12.0) * 2.0).round() / 2.0
}

impl<Message: Clone> canvas::Program<Message> for Gain<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let pos = cursor.position_in(bounds)?;
                state.held = true;
                Some(
                    Action::publish((self.on_change)(value_at(bounds, bounds.y + pos.y)))
                        .and_capture(),
                )
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) if state.held => {
                Some(Action::publish((self.on_change)(value_at(bounds, position.y))).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.held => {
                state.held = false;
                Some(Action::request_redraw())
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right))
                if cursor.is_over(bounds) =>
            {
                Some(Action::publish(self.on_reset.clone()).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _: &State,
        renderer: &Renderer,
        _: &Theme,
        bounds: Rectangle,
        _: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let p = &self.palette;
        let cx = W / 2.0;
        let accent = if self.enabled { p.primary } else { p.outline };
        let percent = (self.value + 12.0) / 24.0;
        let handle_y = (1.0 - percent) * (H - HANDLE) + HANDLE / 2.0;

        let track = Path::rounded_rectangle(
            Point::new(cx - TRACK / 2.0, 0.0),
            Size::new(TRACK, H),
            3.0.into(),
        );
        frame.fill(&track, p.surface_container_highest);
        let mid = Path::rectangle(Point::new(cx - TRACK, H / 2.0), Size::new(TRACK * 2.0, 1.0));
        frame.fill(&mid, p.outline_variant);

        let (top, bottom) = if self.value >= 0.0 {
            (handle_y, H / 2.0)
        } else {
            (H / 2.0, handle_y)
        };
        if bottom > top {
            let fill = Path::rounded_rectangle(
                Point::new(cx - TRACK / 2.0, top),
                Size::new(TRACK, bottom - top),
                3.0.into(),
            );
            frame.fill(&fill, accent);
        }
        let shadow = Path::circle(Point::new(cx, handle_y + 1.0), HANDLE / 2.0);
        frame.fill(&shadow, Color::from_rgba8(0, 0, 0, 0.3));
        frame.fill(
            &Path::circle(Point::new(cx, handle_y), HANDLE / 2.0),
            accent,
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_snaps_to_half_decibels() {
        let b = Rectangle::new(Point::ORIGIN, Size::new(40.0, 132.0));
        assert_eq!(value_at(b, 0.0), 12.0);
        assert_eq!(value_at(b, 132.0), -12.0);
        assert_eq!(value_at(b, 66.0), 0.0);
        assert_eq!(value_at(b, 33.0), 6.0);
        assert_eq!(value_at(b, 1000.0), -12.0);
    }
}
