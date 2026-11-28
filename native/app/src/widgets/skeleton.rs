//! A loading placeholder that gently pulses. Every block shares one clock, so a page of them
//! breathes together; each asks for frames (at 30 fps, plenty for a slow pulse) only while it
//! is on screen, and stops the moment the content replaces it.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad, Renderer as _};
use iced::advanced::widget::{tree, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{mouse, window, Color, Element, Event, Length, Rectangle, Size, Theme};

use crate::theme::{mix, Palette};

const PERIOD: f32 = 1.4;
const STEP: Duration = Duration::from_millis(33);

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// How far into the pulse `now` is, 0..=1 and back (a smooth wave).
pub fn wave(now: Instant) -> f32 {
    let t = now.saturating_duration_since(epoch()).as_secs_f32() / PERIOD;
    0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
}

pub struct Skeleton {
    width: Length,
    height: Length,
    radius: f32,
    low: Color,
    high: Color,
}

/// A placeholder block in the palette's container colours.
pub fn block<'a, Message: 'a>(
    width: impl Into<Length>,
    height: impl Into<Length>,
    radius: f32,
    p: Palette,
) -> Element<'a, Message> {
    Element::new(Skeleton {
        width: width.into(),
        height: height.into(),
        radius,
        low: p.surface_container_high,
        high: mix(p.on_surface, p.surface_container_highest, 0.06),
    })
}

#[derive(Default)]
struct State {
    level: f32,
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Skeleton {
    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            level: wave(Instant::now()),
        })
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let level = tree.state.downcast_ref::<State>().level;
        renderer.fill_quad(
            Quad {
                bounds: layout.bounds(),
                border: iced::border::rounded(self.radius),
                ..Quad::default()
            },
            mix(self.high, self.low, level),
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &iced::Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            if crate::anim::reduced_motion() {
                return;
            }
            tree.state.downcast_mut::<State>().level = wave(*now);
            shell.request_redraw_at(*now + STEP);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_stays_in_range_and_cycles() {
        let start = epoch();
        for ms in (0..3000).step_by(70) {
            let w = wave(start + Duration::from_millis(ms));
            assert!((0.0..=1.0).contains(&w), "{w}");
        }
        assert!(wave(start) < 0.01);
        assert!(wave(start + Duration::from_secs_f32(PERIOD / 2.0)) > 0.99);
    }
}
