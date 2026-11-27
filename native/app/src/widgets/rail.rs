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
    SoundCloud,
    Spotify,
    YouTube,
    Monochrome,
    Settings,
}

impl View {
    pub fn label(self) -> &'static str {
        match self {
            View::Discover => "Home",
            View::Library => "Library",
            View::Playlists => "Playlists",
            View::Local => "Device",
            View::Queue => "Queue",
            View::SoundCloud => "SoundCloud",
            View::Spotify => "Spotify",
            View::Monochrome => "Monochrome",
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
            View::SoundCloud => (Icon::GraphicEq, Icon::GraphicEq),
            View::Spotify => (Icon::Album, Icon::Album),
            View::Monochrome => (Icon::MusicNote, Icon::MusicNote),
            View::YouTube => (Icon::PlayArrow, Icon::PlayArrow),
            View::Settings => (Icon::Settings, Icon::Settings),
        }
    }
}

pub const WIDTH: f32 = 80.0;
const ITEM_H: f32 = 60.0;
const GAP: f32 = 2.0;
/// Space between the general items and the platforms.
const SPLIT: f32 = 14.0;
const PAD_TOP: f32 = 12.0;
const PAD_BOTTOM: f32 = 4.0;
const PILL_INSET: f32 = 6.0;

/// Everything that's yours, across services.
pub const GENERAL: [View; 3] = [View::Discover, View::Library, View::Queue];
/// One tab per place music comes from, each with its own search, library and account.
pub const PLATFORMS: [View; 5] = [
    View::SoundCloud,
    View::Spotify,
    View::YouTube,
    View::Monochrome,
    View::Local,
];

impl View {
    /// The rail item that shows as selected for this view.
    fn rail_item(self) -> View {
        match self {
            View::Playlists => View::Library,
            v => v,
        }
    }
}

/// Where the active pill's top edge sits inside the rail, for the spring target.
pub fn pill_y(view: View, rail_height: f32) -> f32 {
    let view = view.rail_item();
    let step = ITEM_H + GAP;
    if let Some(i) = GENERAL.iter().position(|v| *v == view) {
        return PAD_TOP + PILL_INSET + i as f32 * step;
    }
    if let Some(i) = PLATFORMS.iter().position(|v| *v == view) {
        return PAD_TOP + PILL_INSET + (GENERAL.len() + i) as f32 * step + SPLIT;
    }
    rail_height - PAD_BOTTOM - ITEM_H + PILL_INSET
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
    pill_y: f32,
    p: Palette,
    on_select: impl Fn(View) -> Message,
) -> Element<'a, Message> {
    let current = current.rail_item();
    let mut items = column![].spacing(GAP).align_x(Alignment::Center);
    for v in GENERAL {
        let badge = if v == View::Queue { queue_count } else { 0 };
        items = items.push(item(v, v == current, badge, p, on_select(v)));
    }
    items = items.push(
        container(
            container(Space::new().width(32).height(1))
                .style(styles::fill(p.outline_variant, 0.0)),
        )
        .padding([(SPLIT - GAP * 2.0) / 2.0, 0.0]),
    );
    for v in PLATFORMS {
        items = items.push(item(v, v == current, 0, p, on_select(v)));
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
