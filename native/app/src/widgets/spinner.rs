use iced::mouse::Cursor;
use iced::widget::canvas::{self, stroke, Canvas, Frame, Geometry, Path, Stroke};
use iced::{Color, Element, Point, Radians, Rectangle, Renderer, Theme};

struct Spinner {
    turns: f32,
    color: Color,
}

impl<Message> canvas::Program<Message> for Spinner {
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
        let c = Point::new(bounds.width / 2.0, bounds.height / 2.0);
        let r = bounds.width * 9.0 / 24.0;
        let start = self.turns * std::f32::consts::TAU;
        // `M12 3a9 9 0 1 0 9 9`: three quarters of a circle.
        let arc = Path::new(|p| {
            p.arc(canvas::path::Arc {
                center: c,
                radius: r,
                start_angle: Radians(start - std::f32::consts::FRAC_PI_2),
                end_angle: Radians(start + std::f32::consts::PI),
            });
        });
        frame.stroke(
            &arc,
            Stroke::default()
                .with_width(bounds.width * 3.0 / 24.0)
                .with_color(self.color)
                .with_line_cap(stroke::LineCap::Round),
        );
        vec![frame.into_geometry()]
    }
}

/// `turns` is the rotation in full turns; the CSS spin is one turn per second.
pub fn spinner<'a, Message: 'a>(size: f32, turns: f32, color: Color) -> Element<'a, Message> {
    Canvas::new(Spinner { turns, color })
        .width(size)
        .height(size)
        .into()
}
