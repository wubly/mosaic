//! Press-and-drag reordering for the queue and playlists: the dragged row follows the cursor in
//! whole-row steps, and the list is reordered when the button is released.

use iced::widget::{button, container, mouse_area, row, text};
use iced::{Alignment, Element, Length};
use mosaic_core::format::fmt_ms;
use mosaic_core::model::Track;

use super::artwork_of;
use super::common::ROW_H;
use super::track::{Msg as TrackMsg, TrackRef};
use crate::app::{App, Message};
use crate::theme::{mix, Palette};
use crate::widgets::icon::{icon, Icon};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Queue { begin: usize },
    Playlist(String),
}

#[derive(Clone, Debug)]
pub struct State {
    pub kind: Kind,
    pub from: usize,
    pub to: usize,
    pub len: usize,
    pub start_y: f32,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Start { kind: Kind, from: usize, len: usize },
}

pub fn move_item<T>(list: &mut Vec<T>, from: usize, to: usize) {
    if from >= list.len() || to >= list.len() || from == to {
        return;
    }
    let item = list.remove(from);
    list.insert(to, item);
}

/// Source index shown at each display position while `from` is being dragged to `to`.
pub fn preview_order(len: usize, from: usize, to: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    move_item(&mut order, from, to);
    order
}

pub fn target_for(from: usize, len: usize, start_y: f32, y: f32) -> usize {
    let steps = ((y - start_y) / ROW_H).round() as isize;
    (from as isize + steps).clamp(0, len as isize - 1) as usize
}

impl App {
    pub(crate) fn on_reorder(&mut self, msg: Msg) {
        match msg {
            Msg::Start { kind, from, len } => {
                self.ui.reorder = Some(State {
                    kind,
                    from,
                    to: from,
                    len,
                    start_y: self.cursor.y,
                });
            }
        }
    }

    pub(crate) fn reorder_moved(&mut self) {
        if let Some(r) = &mut self.ui.reorder {
            r.to = target_for(r.from, r.len, r.start_y, self.cursor.y);
        }
    }

    pub(crate) fn reorder_finish(&mut self) -> iced::Task<Message> {
        let Some(r) = self.ui.reorder.take() else {
            return iced::Task::none();
        };
        if r.from == r.to {
            return iced::Task::none();
        }
        match r.kind {
            Kind::Playlist(id) => {
                if let Some(p) = self.store.playlists.iter().find(|p| p.id == id) {
                    let mut tracks = p.tracks.clone();
                    move_item(&mut tracks, r.from, r.to);
                    self.store.reorder_playlist(&id, tracks);
                    self.touch();
                }
                iced::Task::none()
            }
            Kind::Queue { begin } => {
                let mut list = self.player.queue.list.clone();
                move_item(&mut list, begin + r.from, begin + r.to);
                self.player_input(crate::player::PlayerInput::ReorderQueue(list))
            }
        }
    }

    /// A reorderable row (`.row-item.q`): grip, play button, duration and a trailing action.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reorder_row<'a>(
        &'a self,
        t: &Track,
        shown_index: usize,
        source_index: usize,
        r: TrackRef,
        kind: &Kind,
        len: usize,
        trailing: Element<'a, Message>,
        p: Palette,
    ) -> Element<'a, Message> {
        let active = self.player.track.as_ref().is_some_and(|c| c.id == t.id);
        let playing = active && self.player.playing;
        let dragging = self
            .ui
            .reorder
            .as_ref()
            .is_some_and(|d| d.from == source_index && &d.kind == kind);
        let idx: Element<'a, Message> = if active {
            self.bars(playing, p)
        } else {
            text((shown_index + 1).to_string())
                .size(14)
                .color(p.on_surface_variant)
                .into()
        };
        let grip = mouse_area(
            container(icon(Icon::DragIndicator, 22.0).color(p.on_surface_variant)).center(28),
        )
        .on_press(Message::Reorder(Msg::Start {
            kind: kind.clone(),
            from: source_index,
            len,
        }))
        .interaction(iced::mouse::Interaction::Grab);
        let main = button(
            row![
                container(idx).center(32).height(40),
                self.art(artwork_of(t), 40.0, 10.0),
                iced::widget::column![
                    container(
                        text(t.title.clone())
                            .size(14)
                            .wrapping(text::Wrapping::None)
                            .color(if active { p.primary } else { p.on_surface })
                    )
                    .clip(true),
                    container(
                        text(t.artist.clone())
                            .size(12)
                            .wrapping(text::Wrapping::None)
                            .color(p.on_surface_variant)
                    )
                    .clip(true),
                ]
                .width(Length::Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(48)
        .padding([0, 4])
        .style(move |_, _| button::Style {
            background: None,
            text_color: p.on_surface,
            ..button::Style::default()
        })
        .on_press(Message::Track(TrackMsg::Click(r)));
        let fill = if dragging {
            mix(p.primary, p.surface_container_high, 0.2)
        } else if active {
            mix(p.primary, p.surface, 0.16)
        } else {
            mix(p.surface, iced::Color::TRANSPARENT, 0.4)
        };
        container(
            row![
                grip,
                main,
                container(
                    text(fmt_ms(t.duration))
                        .size(12)
                        .color(p.on_surface_variant)
                )
                .width(48)
                .align_x(iced::alignment::Horizontal::Right),
                trailing,
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([0, 8])
        .height(48)
        .style(move |_| container::Style {
            background: Some(fill.into()),
            border: iced::border::rounded(14),
            shadow: if dragging {
                iced::Shadow {
                    color: iced::Color::from_rgba8(0, 0, 0, 0.4),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                }
            } else {
                iced::Shadow::default()
            },
            ..container::Style::default()
        })
        .into()
    }

    pub(crate) fn remove_button<'a>(
        &self,
        msg: Message,
        p: Palette,
        tip: &str,
    ) -> Element<'a, Message> {
        super::common::icon_button(Icon::Close, tip, p, msg)
    }

    pub(crate) fn placeholder_button<'a>(&self) -> Element<'a, Message> {
        container(iced::widget::Space::new())
            .width(40)
            .height(40)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_item_shifts_neighbours() {
        let mut v = vec![0, 1, 2, 3, 4];
        move_item(&mut v, 1, 3);
        assert_eq!(v, [0, 2, 3, 1, 4]);
        move_item(&mut v, 4, 0);
        assert_eq!(v, [4, 0, 2, 3, 1]);
        move_item(&mut v, 9, 0);
        assert_eq!(v, [4, 0, 2, 3, 1]);
    }

    #[test]
    fn preview_matches_the_final_order() {
        assert_eq!(preview_order(4, 0, 2), [1, 2, 0, 3]);
        assert_eq!(preview_order(4, 3, 3), [0, 1, 2, 3]);
    }

    #[test]
    fn target_moves_in_whole_rows_and_clamps() {
        assert_eq!(target_for(2, 6, 100.0, 100.0 + ROW_H * 1.4), 3);
        assert_eq!(target_for(2, 6, 100.0, 100.0 - ROW_H * 5.0), 0);
        assert_eq!(target_for(2, 6, 100.0, 9999.0), 5);
        assert_eq!(target_for(0, 1, 0.0, 500.0), 0);
    }
}
