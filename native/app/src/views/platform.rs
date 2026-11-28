//! One rail tab per music service: its home or library, and its account and settings, with the
//! service's search bar always at the top. Typing shows that service's results in place of
//! the page; clearing the search brings the page back.

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, Task};

use super::discover::{Mode, SearchService};
use crate::app::{App, Message};
use crate::theme::{styles, Palette};
use crate::widgets::rail::View;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Home,
    Account,
}

/// The sub-tabs of a platform view, with their labels (none for a service with one page).
pub fn tabs(view: View) -> &'static [(Tab, &'static str)] {
    match view {
        View::Spotify | View::YouTube => &[(Tab::Home, "Library"), (Tab::Account, "Account")],
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
    matches!(
        view,
        View::SoundCloud | View::Spotify | View::YouTube | View::Monochrome
    )
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
        self.catalog_close(view);
        self.ui.scroll.0 = 0.0;
        self.page_transition();
        self.platform_entered(view)
    }

    /// Called when a platform view (or one of its tabs) comes up. Its search bar searches this
    /// service; what was typed elsewhere carries over and is searched here.
    pub(crate) fn platform_entered(&mut self, view: View) -> Task<Message> {
        let mut search = Task::none();
        if let Some(service) = service(view) {
            if self.ui.discover.service != service {
                self.ui.discover.service = service;
                if !self.ui.discover.query.trim().is_empty() {
                    search = self.on_discover(super::discover::Msg::Submit);
                }
            }
        }
        let load = match (self.platform_tab(view), view) {
            (Tab::Home, View::Spotify) => self.spotify_ensure_loaded(),
            (Tab::Home, View::YouTube) => self.youtube_ensure_loaded(),
            _ => Task::none(),
        };
        Task::batch([search, load])
    }

    /// Whether the platform view shows search results (or why it can't search) instead of
    /// its page.
    fn searching(&self, view: View) -> bool {
        !self.ui.discover.query.trim().is_empty() && service(view).is_some()
    }

    /// The strip above a platform page: its tabs (highlighted unless results are showing) and
    /// the service's search bar.
    fn tab_bar(&self, view: View, p: Palette) -> Element<'_, Message> {
        let current = (!self.searching(view)).then(|| self.platform_tab(view));
        let chips = row(tabs(view).iter().map(|(tab, label)| {
            button(container(text(*label).size(14)).center_y(Length::Fill))
                .height(34)
                .padding([0, 16])
                .style(styles::chip(p, Some(*tab) == current))
                .on_press(Message::PlatformTab(view, *tab))
                .into()
        }))
        .spacing(6)
        .align_y(Alignment::Center);
        // A service without tabs shows its name there instead.
        let left: Element<'_, Message> = if tabs(view).is_empty() {
            text(view.label()).size(26).into()
        } else {
            chips.into()
        };
        // The bar takes the room the left side leaves, up to its own width, at the right, with
        // its suggestions right under it.
        let strip = row![
            container(left).height(56).center_y(56),
            container(
                column![self.search_bar(p)]
                    .width(Length::Fill)
                    .max_width(520)
                    .align_x(Alignment::End),
            )
            .width(Length::Fill)
            .align_x(Alignment::End),
        ]
        .spacing(16)
        .align_y(Alignment::Start);
        container(strip)
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
            _ if self.catalog_page(view).is_some() => {
                self.catalog_view(self.catalog_page(view).expect("checked"), p)
            }
            _ if self.searching(view) => {
                self.discover_page(Mode::Search(service(view).unwrap_or_default()), p)
            }
            (View::SoundCloud, _) => self.discover_page(Mode::Charts, p),
            // Monochrome is all search: with nothing typed, its page says what it is.
            (View::Monochrome, _) => self.discover_page(Mode::Search(SearchService::Monochrome), p),
            (View::Spotify, Tab::Home) => self.spotify_view(p),
            (View::YouTube, Tab::Home) => self.youtube_view(p),
            (View::Spotify, Tab::Account) => {
                self.account_page("Spotify", vec![self.spotify_settings(p)], p)
            }
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
        // Suggestions float over the page under the bar rather than pushing it down.
        let overlay = container(self.suggestion_list(p, false))
            .padding(iced::Padding {
                top: 20.0 + 56.0 + 6.0,
                right: 40.0,
                bottom: 0.0,
                left: 40.0,
            })
            .width(Length::Fill)
            .align_x(Alignment::End);
        iced::widget::stack![
            column![self.tab_bar(view, p), body].height(Length::Fill),
            overlay
        ]
        .into()
    }

    fn account_page<'a>(
        &'a self,
        title: &str,
        rows: Vec<Element<'a, Message>>,
        p: Palette,
    ) -> Element<'a, Message> {
        let body = column![
            self.page_header(
                title.to_string(),
                Some("Account and settings".into()),
                Vec::new(),
                p
            ),
            self.settings_card(rows, p),
        ]
        .max_width(760);
        super::common::page(body, p)
    }
}
