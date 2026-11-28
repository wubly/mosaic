//! Artists, albums and playlists from any service's catalog: the cards search shows for them,
//! and one page layout for opening any of them. Spotify cards lead to the Spotify section's
//! own pages (they can be followed and saved there); every other service opens here.

use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_core::model::{Source, Track};
use mosaic_services::catalog::{Card, CardKind, Collection};
use mosaic_services::spotify::library::PlaylistCard;
use mosaic_services::spotify::web_api::{AlbumCard, ArtistCard};
use rand::seq::SliceRandom;

use super::common::{ghost_button, primary_button, tonal_button, virtual_rows, visible_range};
use super::spotify::{songs, total_length, Route};
use super::track::{ListRef, TrackRef};
use crate::app::{App, Message};
use crate::player::PlayerInput;
use crate::theme::{styles, Palette};
use crate::widgets::icon::Icon;
use crate::widgets::rail::View;

#[derive(Clone, Debug)]
pub enum Msg {
    Open(Card),
    Loaded(String, Result<Box<Collection>, String>),
    Back,
    PlayAll,
    ShuffleAll,
    EnqueueAll,
}

/// An opened card, on the view it was opened from.
pub struct Page {
    pub view: View,
    pub card: Card,
    pub data: Option<Result<Collection, String>>,
}

#[derive(Default)]
pub struct State {
    pub stack: Vec<Page>,
}

fn key(card: &Card) -> String {
    format!("{}:{:?}:{}", card.source.as_str(), card.kind, card.id)
}

pub fn from_spotify_artist(a: &ArtistCard) -> Card {
    Card {
        source: Source::Spotify,
        kind: CardKind::Artist,
        id: a.id.clone(),
        title: a.name.clone(),
        subtitle: "Artist".into(),
        image: a.image.clone(),
    }
}

pub fn from_spotify_album(a: &AlbumCard) -> Card {
    let who = if a.kind == "single" {
        "Single".to_string()
    } else {
        a.artist.clone()
    };
    Card {
        source: Source::Spotify,
        kind: CardKind::Album,
        id: a.id.clone(),
        title: a.name.clone(),
        subtitle: [a.year.clone(), who]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        image: a.image.clone(),
    }
}

pub fn from_spotify_playlist(l: &PlaylistCard) -> Card {
    Card {
        source: Source::Spotify,
        kind: CardKind::Playlist,
        id: l.id.clone(),
        title: l.name.clone(),
        subtitle: l.owner.clone(),
        image: l.image.clone(),
    }
}

fn service_name(source: &Source) -> &'static str {
    match source {
        Source::YouTube => "YouTube Music",
        Source::SoundCloud => "SoundCloud",
        Source::Monochrome => "Monochrome",
        Source::Spotify => "Spotify",
        _ => "Mosaic",
    }
}

impl App {
    /// The page open on `view`, if one is.
    pub(crate) fn catalog_page(&self, view: View) -> Option<&Page> {
        self.ui.catalog.stack.last().filter(|p| p.view == view)
    }

    /// Forgets the pages opened on `view` (its tab was picked again).
    pub(crate) fn catalog_close(&mut self, view: View) {
        self.ui.catalog.stack.retain(|p| p.view != view);
    }

    /// Tracks of the page on screen.
    pub(crate) fn catalog_tracks(&self) -> Vec<Track> {
        match self.catalog_page(self.view_kind()) {
            Some(Page {
                data: Some(Ok(c)), ..
            }) => c.tracks.clone(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn on_catalog(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::Open(card) => {
                if card.source == Source::Spotify {
                    let route = match card.kind {
                        CardKind::Artist => Route::Artist(card.id),
                        CardKind::Album => Route::Album(card.id),
                        CardKind::Playlist => Route::Playlist(card.id),
                    };
                    return self.spotify_go(route);
                }
                let view = self.view_kind();
                self.ui.scroll.0 = 0.0;
                self.page_transition();
                self.ui.catalog.stack.push(Page {
                    view,
                    card: card.clone(),
                    data: None,
                });
                let services = self.services.clone();
                let k = key(&card);
                Task::perform(
                    async move {
                        services
                            .open_card(&card)
                            .await
                            .map(Box::new)
                            .map_err(|e| e.to_string())
                    },
                    move |r| Message::Catalog(Msg::Loaded(k.clone(), r)),
                )
            }
            Msg::Loaded(k, result) => {
                // Only the page that asked; an older one the user backed out of keeps nothing.
                if let Some(page) = self
                    .ui
                    .catalog
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|p| key(&p.card) == k && p.data.is_none())
                {
                    page.data = Some(result.map(|c| *c));
                }
                Task::none()
            }
            Msg::Back => {
                let view = self.view_kind();
                if let Some(i) = self.ui.catalog.stack.iter().rposition(|p| p.view == view) {
                    self.ui.catalog.stack.remove(i);
                }
                self.ui.scroll.0 = 0.0;
                self.page_transition();
                Task::none()
            }
            Msg::PlayAll | Msg::ShuffleAll => {
                let mut list = self.catalog_tracks();
                if matches!(msg, Msg::ShuffleAll) {
                    list.shuffle(&mut rand::rng());
                }
                match list.first().cloned() {
                    Some(t) => self.player_input(PlayerInput::Play(t, Some(list))),
                    None => Task::none(),
                }
            }
            Msg::EnqueueAll => {
                let list = self.catalog_tracks();
                self.toast(format!("Added {} to the queue", songs(list.len() as u64)));
                self.player_input(PlayerInput::EnqueueMany(list))
            }
        }
    }

    /// Whether tracks from `source` have artist and album pages to go to.
    pub(crate) fn has_pages(&self, source: &Source) -> bool {
        match source {
            Source::Spotify => self.spotify_enabled(),
            Source::YouTube | Source::SoundCloud | Source::Monochrome => true,
            _ => false,
        }
    }

    /// Opens the artist or album page of `track` (from its menu), on its service's page.
    pub(crate) fn open_track_page(
        &mut self,
        track: &Track,
        kind: CardKind,
        id: String,
    ) -> Task<Message> {
        self.ui.menu = None;
        let view = match track.source {
            Source::YouTube => View::YouTube,
            Source::SoundCloud => View::SoundCloud,
            Source::Monochrome => View::Monochrome,
            _ => View::Spotify,
        };
        if track.source != Source::Spotify {
            self.ui.view = Some(view);
            self.ui.now_playing = false;
            self.retarget_rail();
        }
        let (title, subtitle) = match kind {
            CardKind::Artist => (track.artist.clone(), "Artist".to_string()),
            _ => (track.album.clone(), track.artist.clone()),
        };
        self.on_catalog(Msg::Open(Card {
            source: track.source.clone(),
            kind,
            id,
            title,
            subtitle,
            image: if kind == CardKind::Album {
                track.artwork.clone()
            } else {
                String::new()
            },
        }))
    }

    /// A carousel of cards; clicking one opens its page.
    pub(crate) fn catalog_cards<'a>(&'a self, cards: &[Card], p: Palette) -> Element<'a, Message> {
        Self::shelf_row(
            crate::widgets::appear::list_key(cards.iter().map(|c| &c.id)),
            cards
                .iter()
                .map(|c| {
                    self.card(
                        &c.image,
                        c.title.clone(),
                        c.subtitle.clone(),
                        c.kind == CardKind::Artist,
                        Message::Catalog(Msg::Open(c.clone())),
                        p,
                    )
                })
                .collect::<Vec<_>>()
                .into_iter(),
            p,
        )
    }

    fn catalog_rows<'a>(
        &'a self,
        tracks: &'a [Track],
        top: f32,
        p: Palette,
    ) -> Element<'a, Message> {
        let range = visible_range(tracks.len(), self.ui.scroll.0, self.ui.scroll.1, top);
        virtual_rows(tracks.len(), range, |i| {
            self.track_row(
                &tracks[i],
                i,
                TrackRef {
                    list: ListRef::Catalog,
                    index: i,
                },
                true,
                p,
            )
        })
    }

    /// The page of an opened artist, album or playlist.
    pub(crate) fn catalog_view<'a>(&'a self, page: &'a Page, p: Palette) -> Element<'a, Message> {
        let h3 = |t: &str| {
            container(text(t.to_string()).size(16).color(p.on_surface_variant)).padding(
                iced::Padding {
                    top: 32.0,
                    bottom: 16.0,
                    ..iced::Padding::ZERO
                },
            )
        };
        let card = match &page.data {
            Some(Ok(c)) => &c.card,
            _ => &page.card,
        };
        let round = card.kind == CardKind::Artist;
        let eyebrow = format!(
            "{} · {}",
            match card.kind {
                CardKind::Artist => "Artist",
                CardKind::Album => "Album",
                CardKind::Playlist => "Playlist",
            },
            service_name(&card.source)
        );
        let mut info = column![
            text(eyebrow).size(14).color(p.on_surface_variant),
            text(card.title.clone()).size(32),
        ]
        .spacing(4)
        .width(Length::Fill);
        if !card.subtitle.is_empty() && card.kind != CardKind::Artist {
            info = info.push(
                text(card.subtitle.clone())
                    .size(14)
                    .color(p.on_surface_variant),
            );
        }
        if let Some(Ok(c)) = &page.data {
            if !c.tracks.is_empty() {
                if card.kind != CardKind::Artist {
                    info = info.push(
                        text(format!(
                            "{} · {}",
                            songs(c.tracks.len() as u64),
                            total_length(&c.tracks)
                        ))
                        .size(14)
                        .color(p.on_surface_variant),
                    );
                }
                info = info.push(
                    container(
                        row![
                            primary_button(
                                "Play",
                                Icon::PlayArrow,
                                p,
                                Message::Catalog(Msg::PlayAll)
                            ),
                            tonal_button(
                                "Shuffle",
                                Icon::Shuffle,
                                p,
                                Message::Catalog(Msg::ShuffleAll)
                            ),
                            tonal_button(
                                "Add to queue",
                                Icon::QueueMusic,
                                p,
                                Message::Catalog(Msg::EnqueueAll)
                            ),
                        ]
                        .spacing(8)
                        .wrap(),
                    )
                    .padding(iced::Padding {
                        top: 16.0,
                        ..iced::Padding::ZERO
                    }),
                );
            }
        }
        let head = row![
            self.art(&card.image, 168.0, if round { 84.0 } else { 20.0 }),
            info
        ]
        .spacing(28)
        .align_y(Alignment::End)
        .padding([8, 0]);

        let mut body = column![
            ghost_button("Back", Icon::ArrowBack, p, Message::Catalog(Msg::Back)),
            head
        ];
        match &page.data {
            None => {
                body = body.push(Space::new().height(24)).push(
                    column(
                        (0..8).map(|_| crate::widgets::skeleton::block(Length::Fill, 46, 12.0, p)),
                    )
                    .spacing(8),
                );
            }
            Some(Err(e)) => {
                body = body.push(Space::new().height(24)).push(
                    container(text(e.clone()).size(14))
                        .padding([12, 16])
                        .width(Length::Fill)
                        .style(styles::error_box(p)),
                );
            }
            Some(Ok(c)) => {
                if c.tracks.is_empty() {
                    body = body.push(h3(if c.card.source == Source::SoundCloud {
                        "SoundCloud only has previews of these songs."
                    } else {
                        "Nothing playable here."
                    }));
                } else if c.card.kind == CardKind::Artist {
                    let top = &c.tracks[..c.tracks.len().min(10)];
                    body = body
                        .push(h3("Popular"))
                        .push(self.catalog_rows(top, 420.0, p));
                } else {
                    body = body
                        .push(Space::new().height(16))
                        .push(self.catalog_rows(&c.tracks, 360.0, p));
                }
                if !c.albums.is_empty() {
                    body = body
                        .push(h3("Discography"))
                        .push(self.catalog_cards(&c.albums, p));
                }
                if let Some(artist) = &c.artist {
                    body = body.push(Space::new().height(16)).push(ghost_button(
                        &format!("More by {}", artist.title),
                        Icon::Person,
                        p,
                        Message::Catalog(Msg::Open(artist.clone())),
                    ));
                }
            }
        }
        super::common::page(body, p)
    }
}
