//! Port of `Queue.js`.

use iced::widget::{column, container, text};
use iced::{Element, Task};

use super::common::{empty_state, icon_button, page, tonal_button};
use super::reorder::{preview_order, Kind, State as Reorder};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::Palette;
use crate::widgets::icon::Icon;

const WINDOW: usize = 150;

#[derive(Clone, Debug)]
pub enum Msg {
    Clear,
    Remove(usize),
}

/// First queue index shown: a window of 150 around the current track.
pub fn window_start(index: isize, len: usize) -> usize {
    let end_limit = len.saturating_sub(WINDOW);
    ((index - 8).max(0) as usize).min(end_limit)
}

impl App {
    pub(crate) fn on_queue(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Clear => self.player_input(PlayerInput::ClearQueue),
            Msg::Remove(i) => self.player_input(PlayerInput::RemoveFromQueue(i)),
        }
    }

    pub(crate) fn queue_view(&self, p: Palette) -> Element<'_, Message> {
        let q = &self.player.queue;
        let upcoming = q.list.len().saturating_sub((q.index + 1).max(0) as usize);
        let subtitle =
            (!q.list.is_empty()).then(|| format!("{} tracks · {upcoming} up next", q.list.len()));
        let mut actions = Vec::new();
        if q.list.len() > 1 {
            actions.push(tonal_button(
                "Clear queue",
                Icon::ClearAll,
                p,
                Message::Queue(Msg::Clear),
            ));
        }
        let mut body = column![self.page_header("Queue".into(), subtitle, actions, p)].spacing(0);
        if q.list.is_empty() {
            body = body.push(empty_state(
                Icon::QueueMusic,
                "Nothing queued",
                "Play a track, or right-click one and choose “Add to queue”.",
                None,
                p,
            ));
            return page(body, p);
        }
        let begin = window_start(q.index, q.list.len());
        let shown = &q.list[begin..(begin + WINDOW).min(q.list.len())];
        let kind = Kind::Queue { begin };
        let order = match &self.ui.reorder {
            Some(Reorder {
                kind: k,
                from,
                to,
                len,
                ..
            }) if *k == kind => preview_order(*len, *from, *to),
            _ => (0..shown.len()).collect(),
        };
        if begin > 0 {
            body = body.push(
                text(format!("{begin} earlier tracks not shown"))
                    .size(14)
                    .color(p.on_surface_variant),
            );
        }
        let mut rows = column![].spacing(6);
        for (pos, src) in order.into_iter().enumerate() {
            let t = &shown[src];
            let abs = begin + src;
            let active = abs as isize == q.index;
            let trailing = if active {
                self.placeholder_button()
            } else {
                icon_button(
                    Icon::Close,
                    "Remove from queue",
                    p,
                    Message::Queue(Msg::Remove(abs)),
                )
            };
            rows = rows.push(self.reorder_row(
                t,
                begin + pos,
                src,
                TrackRef {
                    list: ListRef::Queue,
                    index: abs,
                },
                &kind,
                shown.len(),
                trailing,
                p,
            ));
        }
        body = body.push(rows);
        let after = q.list.len().saturating_sub(begin + WINDOW);
        if after > 0 {
            body = body.push(
                text(format!("{after} more tracks after these"))
                    .size(14)
                    .color(p.on_surface_variant),
            );
        }
        let spotify = &self.ui.spotify.queue.1;
        if self.playing_spotify() && !spotify.is_empty() {
            body = body
                .push(
                    container(
                        column![
                            text("Up next on Spotify")
                                .size(16)
                                .color(p.on_surface_variant),
                            text("What Spotify plays when your queue here runs out")
                                .size(12)
                                .color(p.on_surface_variant),
                        ]
                        .spacing(2),
                    )
                    .padding(iced::Padding {
                        top: 32.0,
                        bottom: 12.0,
                        ..iced::Padding::ZERO
                    }),
                )
                .push(
                    column(spotify.iter().enumerate().map(|(i, t)| {
                        self.track_row(
                            t,
                            i,
                            TrackRef {
                                list: ListRef::SpotifyQueue,
                                index: i,
                            },
                            true,
                            p,
                        )
                    }))
                    .spacing(6),
                );
        }
        page(body, p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_follows_the_current_track() {
        assert_eq!(window_start(0, 10), 0);
        assert_eq!(window_start(5, 400), 0);
        assert_eq!(window_start(100, 400), 92);
        assert_eq!(window_start(399, 400), 250);
        assert_eq!(window_start(-1, 0), 0);
    }
}
