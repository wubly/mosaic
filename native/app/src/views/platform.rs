//! One rail tab per music service. Each has the same few sub-tabs (its home or library, its
//! own search, its account and settings), so everything about a service is in one place.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, Task};

use super::discover::{Mode, SearchService};
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::rail::View;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Home,
    Search,
    Account,
}

/// The sub-tabs of a platform view, with their labels.
pub fn tabs(view: View) -> &'static [(Tab, &'static str)] {
    match view {
        View::SoundCloud => &[(Tab::Home, "Charts"), (Tab::Search, "Search")],
        View::Spotify | View::YouTube => &[
            (Tab::Home, "Library"),
            (Tab::Search, "Search"),
            (Tab::Account, "Account"),
        ],
        View::Monochrome => &[(Tab::Search, "Search")],
        _ => &[],
    }
}

/// The service a platform view searches.
pub fn service(view: View) -> Option<SearchService> {
    Some(match view {
        View::SoundCloud => SearchService::SoundCloud,
        View::Spotify => SearchService::Spotify,
        View::YouTube => SearchService::YouTube,
        View::Monochrome => SearchService::Monochrome,
        View::Local => SearchService::Device,
        _ => return None,
    })
}

pub fn is_platform(view: View) -> bool {
    !tabs(view).is_empty()
}

impl App {
    /// The sub-tab shown for `view`: the last one picked, else Account for a service that
    /// isn't connected yet, else the first.
    pub(crate) fn platform_tab(&self, view: View) -> Tab {
        if let Some(t) = self.ui.platform_tabs.get(&view) {
            return *t;
        }
        let connected = match view {
            View::Spotify => self.spotify_enabled(),
            View::YouTube => self.youtube_enabled(),
            _ => true,
        };
        if !connected {
            return Tab::Account;
        }
        tabs(view).first().map_or(Tab::Home, |(t, _)| *t)
    }

    /// Picking a sub-tab. A search tab searches its own service, carrying over what was typed.
    pub(crate) fn on_platform_tab(&mut self, view: View, tab: Tab) -> Task<Message> {
        self.ui.platform_tabs.insert(view, tab);
        self.ui.scroll.0 = 0.0;
        self.page_transition();
        self.platform_entered(view)
    }

    /// Called when a platform view (or one of its tabs) comes up.
    pub(crate) fn platform_entered(&mut self, view: View) -> Task<Message> {
        match self.platform_tab(view) {
            Tab::Search => {
                let Some(service) = service(view) else {
                    return Task::none();
                };
                if self.ui.discover.service == service {
                    return Task::none();
                }
                self.ui.discover.service = service;
                let q = self.ui.discover.query.clone();
                if q.trim().is_empty() {
                    Task::none()
                } else {
                    self.start_search(q)
                }
            }
            Tab::Home => match view {
                View::Spotify => self.spotify_ensure_loaded(),
                View::YouTube => self.youtube_ensure_loaded(),
                _ => Task::none(),
            },
            Tab::Account => Task::none(),
        }
    }

    fn tab_bar(&self, view: View, p: Palette) -> Element<'_, Message> {
        let current = self.platform_tab(view);
        let chips = row(tabs(view).iter().map(|(tab, label)| {
            button(container(text(*label).size(14)).center_y(Length::Fill))
                .height(34)
                .padding([0, 16])
                .style(styles::chip(p, *tab == current))
                .on_press(Message::PlatformTab(view, *tab))
                .into()
        }))
        .spacing(6)
        .align_y(Alignment::Center);
        container(chips)
            .padding(iced::Padding {
                top: 20.0,
                right: 40.0,
                bottom: 0.0,
                left: 40.0,
            })
            .width(Length::Fill)
            .into()
    }

    pub(crate) fn platform_view(&self, view: View, p: Palette) -> Element<'_, Message> {
        let tab = self.platform_tab(view);
        let body: Element<'_, Message> = match (view, tab) {
            (_, Tab::Search) => self.discover_page(
                Mode::Search(service(view).unwrap_or_default()),
                p,
            ),
            (View::SoundCloud, _) => self.discover_page(Mode::Charts, p),
            (View::Spotify, Tab::Home) => self.spotify_view(p),
            (View::YouTube, Tab::Home) => self.youtube_view(p),
            (View::Spotify, Tab::Account) => self.account_page(
                "Spotify",
                vec![self.spotify_settings(p)],
                p,
            ),
            (View::YouTube, Tab::Account) => {
                let yt = &self.ui.youtube;
                let mut rows = vec![self.youtube_account(p)];
                // The old bring-your-own-client flow, only for whoever already set it up.
                if yt.status.connected || !yt.client_id.trim().is_empty() {
                    rows.push(self.youtube_settings(p));
                }
                self.account_page("YouTube Music", rows, p)
            }
            _ => self.discover_page(Mode::Charts, p),
        };
        if tabs(view).len() < 2 {
            return body;
        }
        column![self.tab_bar(view, p), body]
            .height(Length::Fill)
            .into()
    }

    fn account_page<'a>(
        &'a self,
        title: &str,
        rows: Vec<Element<'a, Message>>,
        p: Palette,
    ) -> Element<'a, Message> {
        let body = column![
            self.page_header(title.to_string(), Some("Account and settings".into()), Vec::new(), p),
            self.settings_card(rows, p),
        ]
        .max_width(760);
        super::common::page(body, p)
    }
}
