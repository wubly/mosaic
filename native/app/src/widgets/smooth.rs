//! Smooth wheel scrolling around a `scrollable`.
//!
//! iced jumps a whole wheel notch (60 px) at once. This wrapper lets the scrollable handle the
//! wheel as usual, so nesting and capture work unchanged, then puts the offset back and eases
//! toward where it was going over the next frames. Notches that arrive mid-glide add up.
//!
//! A horizontal carousel also takes the vertical wheel while the pointer is over it, and hands it
//! back to the page once it can't move any further that way.

use std::time::{Duration, Instant};

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::operation::scrollable::Scrollable;
use iced::advanced::widget::{tree, Operation, Tree, Widget};
use iced::advanced::{overlay, Clipboard, Shell};
use iced::widget::scrollable::AbsoluteOffset;
use iced::{mouse, window, Element, Event, Length, Rectangle, Size, Theme, Vector};

/// Pixels per wheel line, as iced's scrollable uses.
const LINE: f32 = 60.0;
/// How quickly the glide closes the gap (per second); ~120 ms to settle most of the way.
const RATE: f32 = 18.0;

pub struct Smooth<'a, Message> {
    content: Element<'a, Message>,
    horizontal: bool,
}

/// Wraps a vertical scrollable.
pub fn smooth<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Smooth {
        content: content.into(),
        horizontal: false,
    })
}

/// Wraps a horizontal carousel: the vertical wheel scrolls it sideways.
pub fn carousel<'a, Message: 'a>(content: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    Element::new(Smooth {
        content: content.into(),
        horizontal: true,
    })
}

#[derive(Default)]
struct State {
    /// Where the glide is heading, while one runs.
    target: Option<Vector>,
    /// Where the glide is now.
    current: Vector,
    last: Option<Instant>,
}

/// Bounds, content bounds and offset of the wrapped scrollable.
#[derive(Clone, Copy)]
struct Probe {
    bounds: Rectangle,
    content: Rectangle,
    offset: Vector,
}

impl Probe {
    fn max(&self) -> Vector {
        Vector::new(
            (self.content.width - self.bounds.width).max(0.0),
            (self.content.height - self.bounds.height).max(0.0),
        )
    }

    fn clamp(&self, v: Vector) -> Vector {
        let max = self.max();
        Vector::new(v.x.clamp(0.0, max.x), v.y.clamp(0.0, max.y))
    }
}

/// Reads (and optionally sets) the offset of the first scrollable, without descending into it.
struct Op {
    found: Option<Probe>,
    set: Option<Vector>,
}

impl Operation for Op {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        // Look through plain containers but stop at the first scrollable.
        if self.found.is_none() {
            operate(self);
        }
    }

    fn scrollable(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        content: Rectangle,
        translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if self.found.is_some() {
            return;
        }
        self.found = Some(Probe {
            bounds,
            content,
            offset: translation,
        });
        if let Some(v) = self.set {
            state.scroll_to(AbsoluteOffset {
                x: Some(v.x),
                y: Some(v.y),
            });
        }
    }
}

impl<Message> Smooth<'_, Message> {
    fn run(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        set: Option<Vector>,
    ) -> Option<Probe> {
        let mut op = Op { found: None, set };
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut op);
        op.found
    }
}

fn wheel_pixels(delta: mouse::ScrollDelta) -> (Vector, bool) {
    match delta {
        mouse::ScrollDelta::Lines { x, y } => (Vector::new(x * LINE, y * LINE), true),
        mouse::ScrollDelta::Pixels { x, y } => (Vector::new(x, y), false),
    }
}

impl<Message> Widget<Message, Theme, iced::Renderer> for Smooth<'_, Message> {
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
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
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
        match event {
            Event::Window(window::Event::RedrawRequested(now)) => {
                self.glide(tree, layout, renderer, *now, shell);
            }
            // Grabbing the scrollbar (or anything else) takes over from a glide.
            Event::Mouse(mouse::Event::ButtonPressed(_)) => {
                tree.state.downcast_mut::<State>().target = None;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta })
                if cursor.is_over(layout.bounds()) && !shell.is_event_captured() =>
            {
                let before = self.run(tree, layout, renderer, None);
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
                let after = self.run(tree, layout, renderer, None);
                if let (Some(before), Some(after)) = (before, after) {
                    self.on_wheel(tree, layout, renderer, *delta, before, after, shell);
                }
                return;
            }
            _ => {}
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

impl<Message> Smooth<'_, Message> {
    #[allow(clippy::too_many_arguments)]
    fn on_wheel(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        delta: mouse::ScrollDelta,
        before: Probe,
        after: Probe,
        shell: &mut Shell<'_, Message>,
    ) {
        let (pixels, notched) = wheel_pixels(delta);
        let moved = after.offset - before.offset;
        let state = tree.state.downcast_mut::<State>();
        if moved.x.abs() > 0.01 || moved.y.abs() > 0.01 {
            if !notched {
                // Touchpads already scroll smoothly; just stay out of the way.
                state.target = None;
                return;
            }
            let from = state.target.unwrap_or(before.offset);
            if state.target.is_none() {
                state.current = before.offset;
                state.last = None;
            }
            state.target = Some(before.clamp(from + moved));
            let current = state.current;
            let _ = self.run(tree, layout, renderer, Some(current));
            shell.request_redraw();
            return;
        }
        if !self.horizontal || shell.is_event_captured() {
            return;
        }
        // A vertical wheel over a carousel moves it sideways, until it runs out of room.
        let dy = if pixels.y.abs() > pixels.x.abs() {
            pixels.y
        } else {
            pixels.x
        };
        let base = state.target.unwrap_or(before.offset);
        let goal = before.clamp(Vector::new(base.x - dy, base.y));
        if (goal.x - base.x).abs() < 0.5 {
            return;
        }
        shell.capture_event();
        if notched {
            if state.target.is_none() {
                state.current = before.offset;
                state.last = None;
            }
            state.target = Some(goal);
            shell.request_redraw();
        } else {
            state.target = None;
            let _ = self.run(tree, layout, renderer, Some(goal));
        }
    }

    fn glide(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        now: Instant,
        shell: &mut Shell<'_, Message>,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let Some(target) = state.target else { return };
        let dt = state
            .last
            .map_or(Duration::from_millis(16), |l| {
                now.saturating_duration_since(l)
            })
            .min(Duration::from_millis(50));
        state.last = Some(now);
        let k = 1.0 - (-dt.as_secs_f32() * RATE).exp();
        let mut current = state.current + (target - state.current) * k;
        let gap = target - current;
        if gap.x.abs() < 0.5 && gap.y.abs() < 0.5 {
            current = target;
            state.target = None;
            state.last = None;
        } else {
            // At the app's frame rate, not every vsync (see `app::FRAME`).
            shell.request_redraw_at(now + crate::app::FRAME);
        }
        state.current = current;
        let _ = self.run(tree, layout, renderer, Some(current));
    }
}
