//! Track tile, row and context menu (`TrackTile.js`, `TrackRow.js`, `TrackMenu.js`).

use iced::widget::{button, column, container, hover, mouse_area, row, stack, text, Space};
use iced::{Alignment, Element, Length, Point};
use mosaic_core::format::fmt_ms;
use mosaic_core::model::{Source, Track};
use mosaic_core::relevance::filter_along;

use super::artwork_of;
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{mix, styles, Palette};
use crate::widgets::icon::{icon, outlined, Icon};

/// Which list a track came from, so a click can queue the whole list without cloning it into
/// every message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListRef {
    Results,
    Charts,
    More,
    Recent,
    Device,
    Queue,
    Likes,
    History,
    Local,
    Playlist(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackRef {
    pub list: ListRef,
    pub index: usize,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Click(TrackRef),
    Like(TrackRef),
    OpenMenu(TrackRef),
    CloseMenu,
    Menu(MenuAction),
}

#[derive(Clone, Debug)]
pub enum MenuAction {
    Play,
    PlayNext,
    Queue,
    Like,
    Submenu,
    AddTo(String),
    NewPlaylist,
    Radio,
    OpenExternal,
    CopyLink,
}

pub struct MenuState {
    pub track: Track,
    pub at: Point,
    pub sub: bool,
}

pub fn source_name(t: &Track) -> &'static str {
    match t.source {
        Source::Spotify => "Spotify",
        Source::YouTube => "YouTube",
        _ => "SoundCloud",
    }
}

fn is_playable_source(t: &Track) -> bool {
    !matches!(t.source, Source::YouTube | Source::Other(_))
}

impl App {
    pub(crate) fn resolve_list(&self, list: &ListRef) -> Vec<Track> {
        match list {
            ListRef::Results => filter_along(self.ui.discover.results.clone(), None),
            ListRef::Charts => filter_along(self.ui.discover.charts.1.clone(), None),
            ListRef::More => self.ui.discover.more.1.clone(),
            ListRef::Recent => self.recent_tracks(),
            ListRef::Device => self.device_hits(),
            ListRef::Queue => self.player.queue.list.clone(),
            ListRef::Likes => self.store.likes.clone(),
            ListRef::History => self.store.history.iter().map(|h| h.track.clone()).collect(),
            ListRef::Local => self.local.clone(),
            ListRef::Playlist(id) => self
                .store
                .playlists
                .iter()
                .find(|p| &p.id == id)
                .map(|p| p.tracks.clone())
                .unwrap_or_default(),
        }
    }

    pub(crate) fn resolve(&self, r: &TrackRef) -> Option<(Track, Vec<Track>)> {
        let list = self.resolve_list(&r.list);
        list.get(r.index).cloned().map(|t| (t, list))
    }

    pub(crate) fn recent_tracks(&self) -> Vec<Track> {
        let mut seen = std::collections::HashSet::new();
        self.store
            .history
            .iter()
            .map(|h| h.track.clone())
            .filter(|t| seen.insert(t.id.clone()))
            .take(12)
            .collect()
    }

    pub(crate) fn device_hits(&self) -> Vec<Track> {
        let q = self.ui.discover.query.trim().to_lowercase();
        if q.chars().count() <= 1 {
            return Vec::new();
        }
        self.local
            .iter()
            .filter(|t| {
                format!("{} {} {}", t.title, t.artist, t.album)
                    .to_lowercase()
                    .contains(&q)
            })
            .take(5)
            .cloned()
            .collect()
    }

    fn is_active(&self, t: &Track) -> bool {
        self.player.track.as_ref().is_some_and(|c| c.id == t.id)
    }

    pub(crate) fn tile<'a>(
        &'a self,
        t: &Track,
        r: TrackRef,
        width: f32,
        p: Palette,
    ) -> Element<'a, Message> {
        let active = self.is_active(t);
        let playing = active && self.player.playing;
        let liked = self.store.is_liked(&t.id);
        let unavailable = self.player.unavailable(t).is_some();
        let art = stack![
            self.art(artwork_of(t), width, 16.0),
            container(
                container(text(fmt_ms(t.duration)).size(11).color(iced::Color::WHITE))
                    .padding([0, 8])
                    .style(styles::fill(iced::Color::from_rgba8(0, 0, 0, 0.67), 8.0)),
            )
            .width(width)
            .height(width)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(8),
        ];
        let overlay = |show: bool| -> Element<'a, Message> {
            let glyph = if playing {
                Icon::Pause
            } else {
                Icon::PlayArrow
            };
            container(if show {
                Element::from(icon(glyph, 30.0).color(iced::Color::WHITE))
            } else {
                Space::new().into()
            })
            .center(width)
            .style(styles::fill(
                if show {
                    iced::Color::from_rgba8(0, 0, 0, 0.4)
                } else {
                    iced::Color::TRANSPARENT
                },
                16.0,
            ))
            .into()
        };
        let main = button(art)
            .padding(0)
            .style(styles::bare(p))
            .on_press(Message::Track(Msg::Click(r.clone())));
        let top: Element<'a, Message> = if unavailable {
            Space::new().into()
        } else {
            stack![
                overlay(true),
                container(
                    button(
                        container(icon(Icon::MoreVert, 20.0).color(iced::Color::WHITE))
                            .center(Length::Fill)
                    )
                    .width(32)
                    .height(32)
                    .padding(0)
                    .style(move |_, _| button::Style {
                        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.67).into()),
                        border: iced::border::rounded(16),
                        ..button::Style::default()
                    })
                    .on_press(Message::Track(Msg::OpenMenu(r.clone()))),
                )
                .width(width)
                .align_x(iced::alignment::Horizontal::Right)
                .padding(6),
            ]
            .into()
        };
        let art_block: Element<'a, Message> = if active {
            stack![main, overlay(true)].into()
        } else {
            hover(main, top)
        };
        let art_block: Element<'a, Message> = if liked {
            stack![
                art_block,
                container(icon(Icon::Favorite, 16.0).color(p.primary)).padding(10),
            ]
            .into()
        } else {
            art_block
        };
        let title_color = if active { p.primary } else { p.on_surface };
        let alpha = if unavailable { 0.45 } else { 1.0 };
        let body = column![
            art_block,
            container(
                text(t.title.clone())
                    .size(14)
                    .color(crate::theme::alpha(title_color, alpha))
            )
            .height(52)
            .padding(iced::Padding {
                top: 12.0,
                ..iced::Padding::ZERO
            })
            .clip(true),
            container(
                text(t.artist.clone())
                    .size(12)
                    .wrapping(text::Wrapping::None)
                    .color(crate::theme::alpha(p.on_surface_variant, alpha))
            )
            .clip(true),
        ]
        .spacing(2)
        .width(width);
        let body: Element<'a, Message> = mouse_area(body)
            .on_right_press(Message::Track(Msg::OpenMenu(r)))
            .into();
        if unavailable {
            tooltip_unavailable(body, self.player.unavailable(t).unwrap_or_default(), p)
        } else {
            body
        }
    }

    pub(crate) fn track_row<'a>(
        &'a self,
        t: &Track,
        index: usize,
        r: TrackRef,
        show_index: bool,
        p: Palette,
    ) -> Element<'a, Message> {
        let active = self.is_active(t);
        let playing = active && self.player.playing;
        let liked = self.store.is_liked(&t.id);
        let unavailable = self.player.unavailable(t).is_some();
        let alpha = if unavailable { 0.45 } else { 1.0 };
        let idx: Element<'a, Message> = if active {
            self.bars(playing, p)
        } else if show_index {
            text((index + 1).to_string())
                .size(14)
                .color(p.on_surface_variant)
                .into()
        } else {
            Space::new().into()
        };
        let sub = format!(
            "{}{}",
            t.artist,
            if t.album.is_empty() {
                String::new()
            } else {
                format!(" · {}", t.album)
            }
        );
        let mut sub_row = row![].spacing(4).align_y(Alignment::Center);
        if t.source == Source::Local {
            sub_row = sub_row.push(icon(Icon::Folder, 12.0).color(p.on_surface_variant));
        }
        sub_row = sub_row.push(
            text(sub)
                .size(12)
                .wrapping(text::Wrapping::None)
                .color(p.on_surface_variant),
        );
        let main = button(
            row![
                container(idx).center(32).height(40),
                self.art(artwork_of(t), 40.0, 10.0),
                column![
                    container(
                        text(t.title.clone())
                            .size(14)
                            .wrapping(text::Wrapping::None)
                            .color(crate::theme::alpha(
                                if active { p.primary } else { p.on_surface },
                                alpha
                            ))
                    )
                    .clip(true),
                    container(sub_row).clip(true),
                ]
                .width(Length::Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Length::Fill)
        .height(48)
        .padding([0, 4])
        .style(move |_, status| button::Style {
            background: match (active, status) {
                (true, _) => Some(mix(p.primary, iced::Color::TRANSPARENT, 0.16).into()),
                (_, button::Status::Hovered | button::Status::Pressed) => {
                    Some(mix(p.on_surface, iced::Color::TRANSPARENT, 0.08).into())
                }
                _ => None,
            },
            text_color: p.on_surface,
            border: iced::border::rounded(14),
            ..button::Style::default()
        })
        .on_press(Message::Track(Msg::Click(r.clone())));
        let like = button(
            container(if liked {
                icon(Icon::Favorite, 20.0)
            } else {
                outlined(Icon::Favorite, 20.0)
            })
            .center(Length::Fill),
        )
        .width(40)
        .height(40)
        .padding(0)
        .style(if liked {
            Box::new(styles::icon_button_accent(p)) as Box<dyn Fn(&_, _) -> _>
        } else {
            Box::new(styles::icon_button(p, false))
        })
        .on_press(Message::Track(Msg::Like(r.clone())));
        let more = button(container(icon(Icon::MoreVert, 20.0)).center(Length::Fill))
            .width(40)
            .height(40)
            .padding(0)
            .style(styles::icon_button(p, false))
            .on_press(Message::Track(Msg::OpenMenu(r.clone())));
        let line = row![
            main,
            like,
            container(
                text(fmt_ms(t.duration))
                    .size(12)
                    .color(p.on_surface_variant)
            )
            .width(48)
            .align_x(iced::alignment::Horizontal::Right),
            more,
        ]
        .spacing(4)
        .align_y(Alignment::Center);
        let line: Element<'a, Message> = mouse_area(line)
            .on_right_press(Message::Track(Msg::OpenMenu(r)))
            .into();
        match self.player.unavailable(t) {
            Some(reason) => tooltip_unavailable(line, reason, p),
            None => line,
        }
    }

    /// The three equaliser bars (`Bars.js`); they move only while playing.
    fn bars(&self, playing: bool, p: Palette) -> Element<'_, Message> {
        let t = self.now.saturating_duration_since(self.epoch).as_secs_f32();
        let base = [7.0, 12.0, 5.0];
        let bars = (0..3).map(|i| {
            let h = if playing {
                let phase = (t / 0.9 + i as f32 * 0.333).fract();
                3.0 + 11.0 * (1.0 - (phase * 2.0 - 1.0).abs())
            } else {
                base[i]
            };
            container(Space::new().width(3).height(h))
                .style(styles::fill(p.primary, 1.0))
                .into()
        });
        iced::widget::Row::with_children(bars)
            .spacing(2)
            .align_y(Alignment::End)
            .height(14)
            .into()
    }

    pub(crate) fn track_menu(&self, menu: &MenuState, p: Palette) -> Element<'_, Message> {
        let t = &menu.track;
        let liked = self.store.is_liked(&t.id);
        let item =
            |glyph: iced::widget::Text<'static>, label: String, action: MenuAction, end: bool| {
                let mut r = row![glyph.color(p.on_surface_variant), text(label).size(14)]
                    .spacing(14)
                    .align_y(Alignment::Center);
                if end {
                    r = r
                        .push(Space::new().width(Length::Fill))
                        .push(icon(Icon::ChevronRight, 20.0).color(p.on_surface_variant));
                }
                button(r)
                    .width(Length::Fill)
                    .height(40)
                    .padding([0, 12])
                    .style(styles::plain(p, 10.0))
                    .on_press(Message::Track(Msg::Menu(action)))
            };
        let divider = || {
            container(
                container(Space::new().height(1))
                    .width(Length::Fill)
                    .style(styles::fill(p.outline_variant, 0.0)),
            )
            .padding([4, 6])
        };
        let head = |title: String, sub: Option<String>| {
            let mut c = column![text(title).size(14).wrapping(text::Wrapping::None)];
            if let Some(s) = sub {
                c = c.push(
                    text(s)
                        .size(12)
                        .color(p.on_surface_variant)
                        .wrapping(text::Wrapping::None),
                );
            }
            container(c).padding([8, 12]).width(Length::Fill).clip(true)
        };
        let mut items = column![].spacing(0);
        let mut height = 12.0;
        if !menu.sub {
            items = items
                .push(head(t.title.clone(), Some(t.artist.clone())))
                .push(divider())
                .push(item(
                    icon(Icon::PlayArrow, 20.0),
                    "Play".into(),
                    MenuAction::Play,
                    false,
                ))
                .push(item(
                    icon(Icon::PlaylistPlay, 20.0),
                    "Play next".into(),
                    MenuAction::PlayNext,
                    false,
                ))
                .push(item(
                    icon(Icon::QueueMusic, 20.0),
                    "Add to queue".into(),
                    MenuAction::Queue,
                    false,
                ))
                .push(divider())
                .push(item(
                    if liked {
                        icon(Icon::Favorite, 20.0)
                    } else {
                        outlined(Icon::Favorite, 20.0)
                    },
                    if liked {
                        "Unlike".into()
                    } else {
                        "Like".into()
                    },
                    MenuAction::Like,
                    false,
                ))
                .push(item(
                    icon(Icon::PlaylistAdd, 20.0),
                    "Add to playlist".into(),
                    MenuAction::Submenu,
                    true,
                ));
            height += 56.0 + 9.0 + 40.0 * 5.0 + 9.0;
            if t.source == Source::SoundCloud {
                items = items.push(item(
                    icon(Icon::Sensors, 20.0),
                    "Start radio".into(),
                    MenuAction::Radio,
                    false,
                ));
                height += 40.0;
            }
            if t.permalink.is_some() {
                items = items
                    .push(divider())
                    .push(item(
                        icon(Icon::OpenInNew, 20.0),
                        format!("Open on {}", source_name(t)),
                        MenuAction::OpenExternal,
                        false,
                    ))
                    .push(item(
                        icon(Icon::Link, 20.0),
                        "Copy link".into(),
                        MenuAction::CopyLink,
                        false,
                    ));
                height += 9.0 + 80.0;
            }
        } else {
            items = items
                .push(head("Add to playlist".into(), None))
                .push(divider());
            for pl in &self.store.playlists {
                items = items.push(item(
                    icon(Icon::PlaylistPlay, 20.0),
                    pl.name.clone(),
                    MenuAction::AddTo(pl.id.clone()),
                    false,
                ));
            }
            items = items.push(item(
                icon(Icon::Add, 20.0),
                "New playlist".into(),
                MenuAction::NewPlaylist,
                false,
            ));
            height += 40.0 + 9.0 + 40.0 * (self.store.playlists.len() as f32 + 1.0);
        }
        let width = 248.0;
        let x = menu.at.x.min(self.size.width - width - 8.0).max(8.0);
        let y = menu.at.y.min(self.size.height - height - 8.0).max(8.0);
        let popup = container(items)
            .width(width)
            .padding(6)
            .style(styles::popup(p, 16.0));
        stack![
            mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
                .on_press(Message::Track(Msg::CloseMenu))
                .on_right_press(Message::Track(Msg::CloseMenu)),
            container(mouse_area(popup).on_press(Message::Nop)).padding(iced::Padding {
                left: x,
                top: y,
                ..iced::Padding::ZERO
            })
        ]
        .into()
    }

    pub(crate) fn on_track(&mut self, msg: Msg) -> iced::Task<Message> {
        use iced::Task;
        match msg {
            Msg::Click(r) => {
                let Some((track, list)) = self.resolve(&r) else {
                    return Task::none();
                };
                if self.is_active(&track) {
                    self.player_input(PlayerInput::Toggle)
                } else {
                    self.player_input(PlayerInput::Play(track, Some(list)))
                }
            }
            Msg::Like(r) => {
                let Some((track, _)) = self.resolve(&r) else {
                    return Task::none();
                };
                self.like(&track);
                Task::none()
            }
            Msg::OpenMenu(r) => {
                if let Some((track, _)) = self.resolve(&r) {
                    if is_playable_source(&track) {
                        self.ui.menu = Some(MenuState {
                            track,
                            at: self.cursor,
                            sub: false,
                        });
                    }
                }
                Task::none()
            }
            Msg::CloseMenu => {
                self.ui.menu = None;
                Task::none()
            }
            Msg::Menu(action) => self.on_menu(action),
        }
    }

    pub(crate) fn like(&mut self, track: &Track) {
        let liked = self.store.toggle_like(track);
        self.touch();
        self.toast(if liked {
            "Added to Liked songs"
        } else {
            "Removed from Liked songs"
        });
    }

    fn on_menu(&mut self, action: MenuAction) -> iced::Task<Message> {
        use iced::Task;
        if let MenuAction::Submenu = action {
            if let Some(m) = &mut self.ui.menu {
                m.sub = true;
            }
            return Task::none();
        }
        let Some(menu) = self.ui.menu.take() else {
            return Task::none();
        };
        let t = menu.track;
        match action {
            MenuAction::Play => self.player_input(PlayerInput::Play(t, None)),
            MenuAction::PlayNext => self.player_input(PlayerInput::PlayNext(t)),
            MenuAction::Queue => self.player_input(PlayerInput::AddToQueue(t)),
            MenuAction::Like => {
                self.like(&t);
                Task::none()
            }
            MenuAction::AddTo(id) => {
                let name = self
                    .store
                    .playlists
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                let added = self.store.add_to_playlist(&id, &t);
                self.touch();
                self.toast(if added {
                    format!("Added to {name}")
                } else {
                    format!("Already in {name}")
                });
                Task::none()
            }
            MenuAction::NewPlaylist => {
                self.store.create_playlist("New playlist", &[t]);
                self.touch();
                self.toast("Created “New playlist”");
                Task::none()
            }
            MenuAction::Radio => self.player_input(PlayerInput::StartRadio(t)),
            MenuAction::OpenExternal => {
                if let Some(url) = &t.permalink {
                    mosaic_platform::open::open_https(url);
                }
                Task::none()
            }
            MenuAction::CopyLink => {
                self.toast("Link copied");
                iced::clipboard::write(t.permalink.unwrap_or_default())
            }
            MenuAction::Submenu => Task::none(),
        }
    }
}

fn tooltip_unavailable<'a>(
    content: Element<'a, Message>,
    reason: &'static str,
    p: Palette,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        content,
        container(text(reason).size(12))
            .padding([4, 8])
            .style(styles::popup(p, 8.0)),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}
