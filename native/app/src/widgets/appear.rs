//! Motion that runs inside the widget tree, so it needs no animation key in `App` and only asks
//! for frames while it moves.
//!
//! - [`enter`]: content rises into place and grows from slightly smaller when it first shows,
//!   and again whenever its `key` changes (new search results, another page). `index` staggers
//!   a row of cards so they arrive one after another.
//! - [`pop`]: a short bounce when a value flips (a heart filling, play turning to pause). The
//!   value belongs to `group` (a track): when the group changes, as rows of a virtual list get
//!   reused while scrolling, nothing plays.
//!
//! Only drawing is transformed; layout and hit testing stay where the content will land, so
//! nothing around it moves and clicks during the motion go to the right place.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Renderer as _};
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{overlay, Clipboard, Shell};
use iced::{mouse, window, Element, Event, Length, Rectangle, Size, Theme, Transformation, Vector};

use crate::anim::{reduced_motion, Ease};

const ENTER: Duration = Duration::from_millis(380);
const POP: Duration = Duration::from_millis(280);
/// Delay between neighbours in a staggered row; capped so a long row doesn't trail on.
const STAGGER: Duration = Duration::from_millis(28);
const MAX_STAGGER: u32 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Enter { rise: u16, scale: bool },
    Pop,
}

pub struct Appear<'a, Message> {
    content: Element<'a, Message>,
    kind: Kind,
    group: u64,
    key: u64,
    delay: Duration,
}

/// Rises `rise` px (and, with `scale`, grows from 96 %) into place, `index` places after the
/// first of its row. Runs again when `key` changes.
pub fn enter<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    key: u64,
    index: usize,
    rise: u16,
    scale: bool,
) -> Element<'a, Message> {
    Element::new(Appear {
        content: content.into(),
        kind: Kind::Enter { rise, scale },
        group: 0,
        key,
        delay: STAGGER * (index as u32).min(MAX_STAGGER),
    })
}

/// Bounces once when `value` changes while `group` stays the same.
pub fn pop<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    group: u64,
    value: u64,
) -> Element<'a, Message> {
    Element::new(Appear {
        content: content.into(),
        kind: Kind::Pop,
        group,
        key: value,
        delay: Duration::ZERO,
    })
}

/// A key for a list's entrance: changes when the list's first few items do, so new results
/// come in again while a list that only grew at the end doesn't.
pub fn list_key<T: std::hash::Hash>(items: impl Iterator<Item = T>) -> u64 {
    key_of(items.take(4).collect::<Vec<_>>())
}

/// A stable hash for keys (`enter`) and groups (`pop`).
pub fn key_of(value: impl std::hash::Hash) -> u64 {
    use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
    BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
}

struct State {
    group: u64,
    key: u64,
    /// When the motion started (the first frame after it was triggered).
    start: Option<Instant>,
    running: bool,
    /// Progress 0..=1 at the last frame.
    t: f32,
}

impl State {
    fn trigger(&mut self) {
        self.start = None;
        self.running = !reduced_motion();
        self.t = if self.running { 0.0 } else { 1.0 };
    }
}

impl<Message> Appear<'_, Message> {
    fn duration(&self) -> Duration {
        let base = match self.kind {
            Kind::Enter { .. } => ENTER,
            Kind::Pop => POP,
        };
        base.mul_f32(crate::anim::slow_motion())
    }

    /// The transformation at progress `t` for content in `bounds`, or `None` when at rest.
    fn transformation(&self, t: f32, bounds: Rectangle) -> Option<Transformation> {
        if t >= 1.0 {
            return None;
        }
        let (dy, scale) = match self.kind {
            Kind::Enter { rise, scale } => {
                let e = Ease::Decelerate.apply(t);
                let s = if scale { 0.96 + 0.04 * e } else { 1.0 };
                (f32::from(rise) * (1.0 - e), s)
            }
            // Up to 1.22× and back, quick out and settling in.
            Kind::Pop => (
                0.0,
                1.0 + 0.22 * (std::f32::consts::PI * t).sin() * (1.0 - t * 0.35),
            ),
        };
        let c = bounds.center();
        Some(
            Transformation::translate(c.x, c.y + dy)
                * Transformation::scale(scale)
                * Transformation::translate(-c.x, -c.y),
        )
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Appear<'_, Message> {
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let mut state = State {
            group: self.group,
            key: self.key,
            start: None,
            running: false,
            t: 1.0,
        };
        // Entrances play on first show; a pop only plays on a change.
        if matches!(self.kind, Kind::Enter { .. }) {
            state.trigger();
        }
        tree::State::new(state)
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.group != self.group {
            // Another track in this place (a reused row): show it as it is.
            state.group = self.group;
            state.key = self.key;
            state.running = false;
            state.t = 1.0;
        } else if state.key != self.key {
            state.key = self.key;
            state.trigger();
        }
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let draw = |r: &mut iced::Renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                r,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        };
        match self.transformation(state.t, layout.bounds()) {
            Some(m) => renderer.with_transformation(m, draw),
            None => draw(renderer),
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let duration = self.duration();
            let delay = self.delay.mul_f32(crate::anim::slow_motion());
            let state = tree.state.downcast_mut::<State>();
            if state.running {
                let start = *state.start.get_or_insert(*now);
                let elapsed = now.saturating_duration_since(start);
                state.t = if elapsed < delay {
                    0.0
                } else {
                    ((elapsed - delay).as_secs_f32() / duration.as_secs_f32()).min(1.0)
                };
                if state.t >= 1.0 {
                    state.running = false;
                } else {
                    shell.request_redraw_at(*now + crate::app::FRAME);
                }
            }
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appear(kind: Kind) -> Appear<'static, ()> {
        Appear {
            content: Element::new(iced::widget::Space::new()),
            kind,
            group: 0,
            key: 0,
            delay: Duration::ZERO,
        }
    }

    #[test]
    fn at_rest_nothing_is_transformed() {
        let r = Rectangle::new(iced::Point::new(10.0, 10.0), Size::new(100.0, 50.0));
        for kind in [
            Kind::Enter {
                rise: 16,
                scale: true,
            },
            Kind::Pop,
        ] {
            assert!(appear(kind).transformation(1.0, r).is_none());
        }
    }

    #[test]
    fn entering_starts_low_and_small_and_pops_return_to_size() {
        let r = Rectangle::new(iced::Point::new(0.0, 0.0), Size::new(100.0, 100.0));
        let enter = appear(Kind::Enter {
            rise: 16,
            scale: true,
        });
        let start = enter.transformation(0.0, r).unwrap();
        // The centre moves down by the rise; the scale is 96 %.
        let c = iced::Point::new(50.0, 50.0) * start;
        assert!((c.y - 66.0).abs() < 0.01, "{c:?}");
        assert!((start.scale_factor() - 0.96).abs() < 0.001);
        let pop = appear(Kind::Pop);
        assert!((pop.transformation(0.0, r).unwrap().scale_factor() - 1.0).abs() < 0.001);
        assert!(pop.transformation(0.5, r).unwrap().scale_factor() > 1.1);
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(key_of(("a", 1)), key_of(("a", 1)));
        assert_ne!(key_of(("a", 1)), key_of(("a", 2)));
    }
}
