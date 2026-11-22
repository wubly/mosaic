pub mod discover;
pub mod palette;
pub mod player_bar;
pub mod track;
pub mod tune;

use iced::widget::{container, image, Space};
use iced::{ContentFit, Element, Length};

use crate::app::{App, Message};
use crate::widgets::icon::{icon, Icon};

impl App {
    /// Artwork at `size` logical pixels: the cached image, or a gradient placeholder.
    pub(crate) fn art(&self, src: &str, size: f32, radius: f32) -> Element<'_, Message> {
        let p = self.palette();
        match self.images.art(src, (size * 2.0).ceil() as u32, self.now) {
            Some((handle, opacity)) => image(handle)
                .width(size)
                .height(size)
                .content_fit(ContentFit::Cover)
                .border_radius(radius)
                .opacity(opacity)
                .into(),
            None => container(if size >= 40.0 {
                Element::from(
                    icon(Icon::MusicNote, (size * 0.4).min(32.0)).color(p.on_surface_variant),
                )
            } else {
                Space::new().into()
            })
            .center(Length::Fixed(size))
            .style(move |_| container::Style {
                background: Some(iced::Background::Gradient(
                    iced::gradient::Linear::new(std::f32::consts::FRAC_PI_4 * 3.0)
                        .add_stop(0.0, p.primary_container)
                        .add_stop(1.0, p.surface_container_high)
                        .into(),
                )),
                border: iced::border::rounded(radius),
                ..container::Style::default()
            })
            .into(),
        }
    }
}

pub fn artwork_of(track: &mosaic_core::model::Track) -> &str {
    if track.thumb.is_empty() {
        &track.artwork
    } else {
        &track.thumb
    }
}
