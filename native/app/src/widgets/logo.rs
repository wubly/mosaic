use iced::mouse::Cursor;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Path};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme};

const TONES: [[Option<u32>; 3]; 3] = [
    [Some(0xeaddff), Some(0xd0bcff), Some(0xb69df8)],
    [Some(0xd0bcff), None, Some(0x9a82db)],
    [Some(0xb69df8), Some(0x9a82db), Some(0x7f67be)],
];

pub struct Mark;

fn rgb(v: u32) -> Color {
    Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl<Message> canvas::Program<Message> for Mark {
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
        let k = bounds.width / 512.0;
        let (pad, gap) = (84.0, 14.0);
        let cell = (512.0 - pad * 2.0 - gap * 2.0) / 3.0;

        let backdrop = Path::rounded_rectangle(Point::ORIGIN, bounds.size(), (112.0 * k).into());
        frame.fill(
            &backdrop,
            canvas::Gradient::Linear(
                canvas::gradient::Linear::new(
                    Point::ORIGIN,
                    Point::new(bounds.width, bounds.height),
                )
                .add_stop(0.0, rgb(0x2b2930))
                .add_stop(1.0, rgb(0x141218)),
            ),
        );
        for (r, row) in TONES.iter().enumerate() {
            for (q, tone) in row.iter().enumerate() {
                let x = (pad + q as f32 * (cell + gap)) * k;
                let y = (pad + r as f32 * (cell + gap)) * k;
                let square = Path::rounded_rectangle(
                    Point::new(x, y),
                    Size::new(cell * k, cell * k),
                    (30.0 * k).into(),
                );
                frame.fill(&square, rgb(tone.unwrap_or(0xd0bcff)));
                if tone.is_none() {
                    let c = cell * k;
                    let triangle = Path::new(|p| {
                        p.move_to(Point::new(x + c * 0.38, y + c * 0.26));
                        p.line_to(Point::new(x + c * 0.38, y + c * 0.74));
                        p.line_to(Point::new(x + c * 0.76, y + c * 0.5));
                        p.close();
                    });
                    frame.fill(&triangle, rgb(0x381e72));
                }
            }
        }
        vec![frame.into_geometry()]
    }
}

/// The 3×3 mosaic mark from `Logo.js`.
pub fn mark<'a, Message: 'a>(size: f32) -> Canvas<Mark, Message> {
    Canvas::new(Mark).width(size).height(size)
}
