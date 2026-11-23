use iced::widget::{button, column, container, row, stack, text, Space};
use iced::{Alignment, Element, Length};

use super::icon::{icon, outlined, Icon};
use crate::theme::{styles, Palette};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Discover,
    Library,
    Playlists,
    Local,
    Queue,
    Spotify,
    YouTube,
    Settings,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Discover => "Discover",
            View::Library => "Library",
            View::Playlists => "Playlists",
            View::Local => "Device",
            View::Queue => "Queue",
            View::Spotify => "Spotify",
            View::YouTube => "YouTube",
            View::Settings => "Settings",
        }
    }

    /// (outlined, filled) glyphs, as in `Rail.js`.
    fn glyphs(self) -> (Icon, Icon) {
        match self {
            View::Discover => (Icon::Explore, Icon::Explore),
            View::Library => (Icon::LibraryMusic, Icon::LibraryMusic),
            View::Playlists => (Icon::PlaylistPlay, Icon::PlaylistPlay),
            View::Local => (Icon::Folder, Icon::FolderOpen),
            View::Queue => (Icon::QueueMusic, Icon::QueueMusic),
            View::Spotify => (Icon::Album, Icon::Album),
            View::YouTube => (Icon::PlayArrow, Icon::PlayArrow),
            View::Settings => (Icon::Settings, Icon::Settings),
        }
    }
}

pub const WIDTH: f32 = 80.0;
const ITEM_H: f32 = 64.0;
const GAP: f32 = 12.0;
const PAD_TOP: f32 = 12.0;
const PAD_BOTTOM: f32 = 4.0;
const PILL_INSET: f32 = 6.0;

pub fn top_items(spotify: bool, youtube: bool) -> Vec<View> {
    let mut items = vec![
        View::Discover,
        View::Library,
        View::Playlists,
        View::Local,
        View::Queue,
    ];
    if spotify {
        items.push(View::Spotify);
    }
    if youtube {
        items.push(View::YouTube);
    }
    items
}

/// Where the active pill's top edge sits inside the rail, for the spring target.
pub fn pill_y(view: View, spotify: bool, youtube: bool, rail_height: f32) -> f32 {
    match top_items(spotify, youtube).iter().position(|v| *v == view) {
        Some(i) => PAD_TOP + PILL_INSET + i as f32 * (ITEM_H + GAP),
        None => rail_height - PAD_BOTTOM - ITEM_H + PILL_INSET,
    }
}

fn item<'a, Message: Clone + 'a>(
    view: View,
    active: bool,
    badge: usize,
    p: Palette,
    on_press: Message,
) -> Element<'a, Message> {
    let (plain, filled) = view.glyphs();
    let glyph = if active {
        icon(filled, 24.0).color(p.on_primary_container)
    } else {
        outlined(plain, 24.0).color(p.on_surface_variant)
    };
    let mut pill = stack![container(glyph).center(Length::Fill)]
        .width(56)
        .height(32);
    if badge > 0 {
        let label = if badge > 99 {
            "99+".to_string()
        } else {
            badge.to_string()
        };
        pill = pill.push(
            container(
                container(text(label).size(11).line_height(1.0).color(p.on_primary))
                    .padding([2, 4])
                    .style(styles::fill(p.primary, 8.0)),
            )
            .align_right(Length::Fill)
            .padding(iced::Padding {
                top: -2.0,
                right: 2.0,
                ..iced::Padding::ZERO
            }),
        );
    }
    let label_color = if active {
        p.on_surface
    } else {
        p.on_surface_variant
    };
    button(
        column![pill, text(view.label()).size(12).color(label_color)]
            .spacing(6)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .padding([6, 0])
    .width(72)
    .height(ITEM_H)
    .style(styles::bare(p))
    .on_press(on_press)
    .into()
}

pub fn view<'a, Message: Clone + 'a>(
    current: View,
    queue_count: usize,
    spotify: bool,
    youtube: bool,
    pill_y: f32,
    p: Palette,
    on_select: impl Fn(View) -> Message,
) -> Element<'a, Message> {
    let mut items = column![].spacing(GAP).align_x(Alignment::Center);
    for v in top_items(spotify, youtube) {
        let badge = if v == View::Queue { queue_count } else { 0 };
        items = items.push(item(v, v == current, badge, p, on_select(v)));
    }
    let nav = column![
        items,
        Space::new().height(Length::Fill),
        item(
            View::Settings,
            current == View::Settings,
            0,
            p,
            on_select(View::Settings)
        ),
    ]
    .padding(iced::Padding {
        top: PAD_TOP,
        bottom: PAD_BOTTOM,
        ..iced::Padding::ZERO
    })
    .align_x(Alignment::Center)
    .width(WIDTH)
    .height(Length::Fill);

    let pill = column![
        Space::new().height(pill_y),
        row![
            Space::new().width((WIDTH - 56.0) / 2.0),
            container(Space::new().width(56).height(32))
                .style(styles::fill(p.primary_container, 16.0)),
        ],
    ];
    stack![pill, nav].width(WIDTH).height(Length::Fill).into()
}
