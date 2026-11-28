//! Port of `SettingsView.js` (no YouTube or Performance sections).

use iced::widget::{button, column, container, row, text, text_input, Space};
use iced::{Alignment, Element, Length, Task};
use mosaic_services::update::{self, UpdateState};

use super::common::{page, primary_button, tonal_button};
use crate::app::{App, Message};

use crate::theme::{styles, Palette};
use crate::widgets::icon::Icon;
use crate::widgets::switch::switch;
use crate::widgets::wavy_slider::flat_slider;

pub const SHORTCUTS: [(&str, &str); 13] = [
    ("Space", "Play / pause"),
    ("← →", "Seek 5 seconds"),
    ("Shift + ← →", "Previous / next track"),
    ("↑ ↓", "Volume"),
    ("M", "Mute"),
    ("L", "Like current track"),
    ("S / R", "Shuffle / repeat"),
    ("Y", "Toggle lyrics panel"),
    ("F", "Immersive Now Playing"),
    ("I", "Mini player (always on top)"),
    ("/", "Focus search"),
    ("Ctrl + K", "Command palette"),
    ("Media keys", "Work system-wide, even when minimised"),
];

pub const NO_TRAY: &str = "No system tray found, so closing the window quits Mosaic";

#[derive(Clone, Debug)]
pub enum Msg {
    CheckUpdate,
    Updated(Box<UpdateState>),
    OpenUpdate,
    AutoUpdate(bool),
    /// Download the update if needed, then install it and restart.
    InstallUpdate,
    /// The download finished; `true` installs right away.
    Downloaded(Result<std::path::PathBuf, String>, bool),
    Autoplay(bool),
    Crossfade(f32),
    FadePause(bool),
    SkipSilence(bool),
    Leveling(bool),
    Normalize(bool),
    MixedSources(bool),
    Lyrics(bool),
    CloseToTray(bool),
    Presence(bool),
    PlaybackLogin,
    PlaybackReady(Result<String, String>),
    SoloistPath(String),
    SoloistKey(String),
    SoloistSave,
    SoloistDownload,
    Rescan,
    ClearHistory,
    Onboarding,
}

pub fn crossfade_from_fraction(f: f32) -> f64 {
    f64::from((f.clamp(0.0, 1.0) * 12.0).round())
}

impl App {
    pub(crate) fn check_update(&mut self) -> Task<Message> {
        self.ui.checking_update = true;
        let http = self.services.http().clone();
        let previous = self.ui.update.clone();
        Task::perform(
            async move { update::check(&http, &previous, env!("CARGO_PKG_VERSION")).await },
            |u| Message::Settings(Msg::Updated(Box::new(u))),
        )
    }

    fn download_update(&mut self, install: bool) -> Task<Message> {
        let Some(asset) = self.ui.update.asset.clone() else {
            return self.update(Message::Settings(Msg::OpenUpdate));
        };
        if self.ui.update_busy {
            return Task::none();
        }
        self.ui.update_busy = true;
        let http = self.services.http().clone();
        let dir = std::env::temp_dir().join("mosaic-update");
        Task::perform(
            async move {
                update::download(&http, &asset, &dir, |_, _| {})
                    .await
                    .map_err(|e| e.to_string())
            },
            move |r| Message::Settings(Msg::Downloaded(r, install)),
        )
    }

    /// Hands over to the installer, which waits for Mosaic to quit and starts it again.
    fn install_update(&mut self, path: std::path::PathBuf) -> Task<Message> {
        match update::install_and_restart(&path) {
            Ok(()) => iced::exit(),
            Err(update::InstallError::OpenReleasePage) => {
                self.update(Message::Settings(Msg::OpenUpdate))
            }
            Err(update::InstallError::Failed(e)) => {
                self.toast(format!("Couldn’t install the update: {e}"));
                Task::none()
            }
        }
    }

    pub(crate) fn on_settings(&mut self, msg: Msg) -> Task<Message> {
        match msg {
            Msg::CheckUpdate => self.check_update(),
            Msg::Updated(u) => {
                self.ui.checking_update = false;
                let announce = u.available && !self.ui.update.available;
                if u.latest != self.ui.update.latest {
                    self.ui.update_file = None;
                }
                self.ui.update = *u;
                let auto =
                    self.services.config().auto_update == Some(true) && update::can_self_update();
                if announce && auto {
                    return self.download_update(false);
                }
                if announce {
                    let latest = self.ui.update.latest.clone().unwrap_or_default();
                    self.toast(format!(
                        "Mosaic {latest} is ready. Open Settings to update."
                    ));
                }
                Task::none()
            }
            Msg::AutoUpdate(v) => {
                self.services.update_config(|c| c.auto_update = Some(v));
                if v && self.ui.update.available && self.ui.update_file.is_none() {
                    return self.download_update(false);
                }
                Task::none()
            }
            Msg::InstallUpdate => match self.ui.update_file.clone() {
                Some(path) => self.install_update(path),
                None => self.download_update(true),
            },
            Msg::Downloaded(result, install) => {
                self.ui.update_busy = false;
                match result {
                    Ok(path) => {
                        self.ui.update_file = Some(path.clone());
                        if install {
                            return self.install_update(path);
                        }
                        let latest = self.ui.update.latest.clone().unwrap_or_default();
                        self.toast(format!(
                            "Mosaic {latest} is downloaded. Restart to update from Settings."
                        ));
                    }
                    Err(e) => self.toast(format!("Couldn’t download the update: {e}")),
                }
                Task::none()
            }
            Msg::OpenUpdate => {
                if let Some(url) = &self.ui.update.url {
                    mosaic_platform::open::open_https(url);
                }
                Task::none()
            }
            Msg::Autoplay(v) => self.set_setting(|s| s.autoplay = v),
            Msg::Crossfade(f) => {
                let v = crossfade_from_fraction(f);
                self.set_setting(|s| s.crossfade = v)
            }
            Msg::FadePause(v) => self.set_setting(|s| s.fade_pause = v),
            Msg::SkipSilence(v) => self.set_setting(|s| s.skip_silence = v),
            Msg::Leveling(v) => self.set_setting(|s| s.leveling = v),
            Msg::Normalize(v) => self.set_setting(|s| s.normalize = v),
            Msg::MixedSources(v) => self.set_setting(|s| s.mixed_sources = v),
            Msg::Lyrics(v) => self.set_setting(|s| s.lyrics_open = v),
            Msg::CloseToTray(v) => {
                self.services.update_config(|c| c.close_to_tray = Some(v));
                Task::none()
            }
            Msg::Presence(v) => self.set_setting(|s| s.presence = v),
            Msg::PlaybackLogin => self.playback_login(),
            Msg::PlaybackReady(result) => {
                self.toast(match result {
                    Ok(status) => status,
                    Err(e) => e,
                });
                Task::none()
            }
            Msg::SoloistPath(s) => {
                self.ui.spotify.soloist_path = s;
                Task::none()
            }
            Msg::SoloistKey(s) => {
                self.ui.spotify.soloist_key = s;
                Task::none()
            }
            Msg::SoloistSave => self.soloist_save(),
            Msg::SoloistDownload => {
                #[cfg(feature = "spotify")]
                mosaic_platform::open::open_https(mosaic_services::spotify::soloist::DOWNLOAD_URL);
                Task::none()
            }
            Msg::Rescan => {
                self.toast("Rescanning your music…");
                self.ui.local.scanning = true;
                self.scan()
            }
            Msg::ClearHistory => {
                self.store.clear_history();
                self.touch();
                self.refresh_recent();
                self.toast("History and stats cleared");
                Task::none()
            }
            Msg::Onboarding => {
                self.services.update_config(|c| *c = c.reset());
                self.begin_onboarding();
                Task::none()
            }
        }
    }

    #[cfg(feature = "spotify")]
    fn playback_login(&mut self) -> Task<Message> {
        let services = self.services.clone();
        self.toast("Opening Spotify sign-in for playback…");
        Task::perform(
            async move {
                services
                    .spotify_playback()
                    .await
                    .map(|pb| pb.describe())
                    .map_err(|e| e.to_string())
            },
            |r| Message::Settings(Msg::PlaybackReady(r)),
        )
    }

    #[cfg(not(feature = "spotify"))]
    fn playback_login(&mut self) -> Task<Message> {
        Task::none()
    }

    /// Saves the Soloist settings, restarts the backend with them, and starts Soloist when a key
    /// is set (so it is up for pairing).
    #[cfg(feature = "spotify")]
    fn soloist_save(&mut self) -> Task<Message> {
        use mosaic_services::spotify::soloist::SoloistSettings;
        let settings = SoloistSettings {
            path: self.ui.spotify.soloist_path.clone(),
            api_key: self.ui.spotify.soloist_key.clone(),
        };
        if let Err(e) = self.services.soloist_save(&settings) {
            self.toast(format!("Could not save Soloist settings: {e}"));
            return Task::none();
        }
        if settings.configured() {
            self.toast("Starting Spotify Soloist…");
        }
        let services = self.services.clone();
        Task::perform(
            async move {
                services.spotify_playback_reset().await;
                if !settings.configured() {
                    return Ok("Soloist turned off; Spotify plays through librespot".to_string());
                }
                services
                    .spotify_playback()
                    .await
                    .map(|pb| pb.describe())
                    .map_err(|e| e.to_string())
            },
            |r| Message::Settings(Msg::PlaybackReady(r)),
        )
    }

    #[cfg(not(feature = "spotify"))]
    fn soloist_save(&mut self) -> Task<Message> {
        Task::none()
    }

    fn switch_row<'a>(
        &self,
        title: &str,
        hint: String,
        on: bool,
        msg: impl Fn(bool) -> Msg,
        p: Palette,
    ) -> Element<'a, Message> {
        container(
            row![
                column![
                    text(title.to_string()).size(16),
                    text(hint).size(13).color(p.on_surface_variant)
                ]
                .spacing(2)
                .width(Length::Fill),
                switch(on, p, Message::Settings(msg(!on))),
            ]
            .spacing(24)
            .align_y(Alignment::Center),
        )
        .padding([14, 0])
        .into()
    }

    pub(crate) fn settings_card<'a>(
        &self,
        rows: Vec<Element<'a, Message>>,
        p: Palette,
    ) -> Element<'a, Message> {
        let mut col = column![].spacing(0);
        for (i, r) in rows.into_iter().enumerate() {
            if i > 0 {
                col = col.push(container(Space::new().height(1)).width(Length::Fill).style(
                    styles::fill(crate::theme::alpha(p.outline_variant, 0.55), 0.0),
                ));
            }
            col = col.push(r);
        }
        container(col)
            .padding([6, 20])
            .width(Length::Fill)
            .style(styles::fill(p.surface_container, 24.0))
            .into()
    }

    fn group<'a>(
        &self,
        title: &str,
        body: Element<'a, Message>,
        p: Palette,
    ) -> Element<'a, Message> {
        column![
            container(text(title.to_uppercase()).size(13).color(p.primary)).padding(
                iced::Padding {
                    left: 8.0,
                    ..iced::Padding::ZERO
                }
            ),
            body
        ]
        .spacing(10)
        .into()
    }

    pub(crate) fn settings_view(&self, p: Palette) -> Element<'_, Message> {
        let s = self.settings();
        let config = self.services.config();
        let u = &self.ui.update;

        let update_row: Element<'_, Message> = {
            let (title, hint) = if u.available {
                (
                    format!("Mosaic {} is ready", u.latest.clone().unwrap_or_default()),
                    if self.ui.update_file.is_some() {
                        format!(
                            "You’re on {}. Downloaded and ready: Mosaic restarts to finish.",
                            u.current
                        )
                    } else {
                        format!("You’re on {}.", u.current)
                    },
                )
            } else {
                let current = if u.current.is_empty() {
                    env!("CARGO_PKG_VERSION").to_string()
                } else {
                    u.current.clone()
                };
                (
                    format!("Mosaic {current}"),
                    match (&u.error, u.checked_at) {
                        (Some(e), _) => format!("Couldn’t check: {e}"),
                        (None, 0) => "Checking for updates…".into(),
                        _ => "You’re up to date.".into(),
                    },
                )
            };
            let action: Element<'_, Message> = if u.available
                && update::can_self_update()
                && u.asset.is_some()
            {
                primary_button(
                    if self.ui.update_busy {
                        "Downloading…"
                    } else if self.ui.update_file.is_some() {
                        "Restart to update"
                    } else {
                        "Update now"
                    },
                    Icon::CloudDownload,
                    p,
                    Message::Settings(Msg::InstallUpdate),
                )
            } else if u.available {
                primary_button(
                    "Get update",
                    Icon::OpenInNew,
                    p,
                    Message::Settings(Msg::OpenUpdate),
                )
            } else {
                let label = if self.ui.checking_update {
                    "Checking…"
                } else {
                    "Check now"
                };
                let b = button(container(text(label.to_string()).size(14)).center_y(Length::Fill))
                    .height(40)
                    .padding([0, 20])
                    .style(styles::tonal(p))
                    .on_press_maybe(
                        (!self.ui.checking_update).then_some(Message::Settings(Msg::CheckUpdate)),
                    );
                b.into()
            };
            container(
                row![
                    column![
                        text(title).size(16),
                        text(hint).size(13).color(p.on_surface_variant)
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    action
                ]
                .spacing(24)
                .align_y(Alignment::Center),
            )
            .padding([14, 0])
            .into()
        };

        let crossfade = {
            let hint = if s.crossfade > 0.0 {
                format!(
                    "Overlap the end of one track with the start of the next for {} seconds.",
                    s.crossfade
                )
            } else {
                "Off. Tracks play back to back.".into()
            };
            container(
                row![
                    column![
                        text("Crossfade").size(16),
                        text(hint).size(13).color(p.on_surface_variant)
                    ]
                    .spacing(2)
                    .width(Length::Fill),
                    container(flat_slider((s.crossfade / 12.0) as f32, p, |f| {
                        Message::Settings(Msg::Crossfade(f))
                    }))
                    .width(180),
                ]
                .spacing(24)
                .align_y(Alignment::Center),
            )
            .padding([14, 0])
        };

        let tray_hint = if self.tray_available {
            "Closing the window hides Mosaic in the system tray so your music keeps going. Quit from the tray icon.".to_string()
        } else {
            NO_TRAY.to_string()
        };
        let playback = self.settings_card(
            vec![
                self.switch_row("Autoplay similar tracks", "Keep the queue going with songs in the style of what’s playing. They show up in the queue right away, so you can change them first. A Spotify song played from an album or playlist follows Spotify’s own queue.".into(), s.autoplay, Msg::Autoplay, p),
                self.switch_row("Mix services in autoplay (experimental)", "When the service you’re listening on doesn’t have a song autoplay picked, play it from YouTube Music instead of skipping it. The same song never shows up twice in the queue.".into(), s.mixed_sources, Msg::MixedSources, p),
                crossfade.into(),
                self.switch_row("Fade on pause and resume", "Ease the volume instead of cutting it off.".into(), s.fade_pause, Msg::FadePause, p),
                self.switch_row("Skip silence at start and end", "Jump past long silent intros and outros, like the blank first minute some SoundCloud uploads use to dodge copyright. Never touches the middle of a song.".into(), s.skip_silence, Msg::SkipSilence, p),
                self.switch_row("Same volume everywhere", "Play every song at the same loudness, so SoundCloud, YouTube and Monochrome match each other and Spotify.".into(), s.normalize, Msg::Normalize, p),
                self.switch_row("Level loudness", "Gently compress loud peaks so quiet and loud tracks sit closer together.".into(), s.leveling, Msg::Leveling, p),
                self.switch_row("Show lyrics panel", "Synced lyrics from LRCLIB beside your music, matched to the song and artist.".into(), s.lyrics_open, Msg::Lyrics, p),
                self.switch_row("Keep playing in the tray", tray_hint, config.close_to_tray() && self.tray_available, Msg::CloseToTray, p),
            ],
            p,
        );

        let discord = {
            let st = self.discord.status();
            let note = if !s.presence {
                String::new()
            } else if st.connected {
                format!(
                    " Connected.{}",
                    st.error
                        .map(|e| format!(" Last update failed: {e}"))
                        .unwrap_or_default()
                )
            } else {
                " Discord isn’t running — Mosaic keeps trying and connects automatically.".into()
            };
            self.settings_card(
                vec![self.switch_row(
                    "Rich Presence",
                    format!("Show what you are listening to on your Discord profile.{note}"),
                    s.presence,
                    Msg::Presence,
                    p,
                )],
                p,
            )
        };

        let library = self.settings_card(
            vec![container(
                iced::widget::Row::with_children(vec![
                    tonal_button(
                        "Rescan local music",
                        Icon::Refresh,
                        p,
                        Message::Settings(Msg::Rescan),
                    ),
                    tonal_button(
                        "Clear history & stats",
                        Icon::Delete,
                        p,
                        Message::Settings(Msg::ClearHistory),
                    ),
                    tonal_button(
                        "Restart onboarding",
                        Icon::RestartAlt,
                        p,
                        Message::Settings(Msg::Onboarding),
                    ),
                ])
                .spacing(8)
                .wrap()
                .vertical_spacing(8),
            )
            .padding([14, 0])
            .into()],
            p,
        );

        let mut keys = column![].spacing(0);
        for (k, d) in SHORTCUTS {
            keys = keys.push(
                container(
                    row![
                        row(k.split(" + ").enumerate().flat_map(|(i, part)| {
                            let mut v: Vec<Element<'_, Message>> = Vec::new();
                            if i > 0 {
                                v.push(text(" + ").size(13).color(p.on_surface_variant).into());
                            }
                            v.push(
                                container(text(part.to_string()).size(12))
                                    .padding([1, 7])
                                    .style(styles::kbd(p))
                                    .into(),
                            );
                            v
                        })),
                        Space::new().width(Length::Fill),
                        text(d.to_string()).size(14).color(p.on_surface_variant),
                    ]
                    .align_y(Alignment::Center),
                )
                .padding([8, 0]),
            );
        }
        let about = self.settings_card(
            vec![container(
                text(format!(
                    "Mosaic {} · lyrics by LRCLIB · colour from your artwork via Material You.",
                    env!("CARGO_PKG_VERSION")
                ))
                .size(14)
                .color(p.on_surface_variant),
            )
            .padding([14, 0])
            .into()],
            p,
        );

        let groups = [
            self.group(
                "Updates",
                self.settings_card(
                    if update::can_self_update() {
                        vec![
                            update_row,
                            self.switch_row(
                                "Download updates automatically",
                                "New versions download in the background. Installing waits for you to say so.".into(),
                                config.auto_update == Some(true),
                                Msg::AutoUpdate,
                                p,
                            ),
                        ]
                    } else {
                        vec![update_row]
                    },
                    p,
                ),
                p,
            ),
            self.group("Playback", playback, p),
            self.group("Discord", discord, p),
            self.group("Library", library, p),
            self.group(
                "Keyboard shortcuts",
                self.settings_card(vec![keys.into()], p),
                p,
            ),
            self.group("About", about, p),
        ];
        let header = self.page_header("Settings".into(), None, Vec::new(), p);
        // Two columns once the page is wide enough for two readable cards side by side.
        let body: Element<'_, Message> = if self.main_width() >= 1120.0 {
            let mut left = column![].spacing(28).width(Length::FillPortion(1));
            let mut right = column![].spacing(28).width(Length::FillPortion(1));
            for (i, g) in groups.into_iter().enumerate() {
                if i < 4 {
                    left = left.push(g);
                } else {
                    right = right.push(g);
                }
            }
            column![header, row![left, right].spacing(24)]
                .spacing(28)
                .into()
        } else {
            column(std::iter::once(header).chain(groups))
                .spacing(28)
                .max_width(720)
                .into()
        };
        page(body, p)
    }

    pub(crate) fn spotify_settings(&self, p: Palette) -> Element<'_, Message> {
        use super::spotify::Msg as Sp;
        let st = &self.ui.spotify;
        let status = &st.status;
        let hint = if status.connected {
            format!(
                "Connected as {}.{}",
                status.user.clone().unwrap_or_default(),
                if status.premium {
                    ""
                } else {
                    " Playing needs Premium."
                }
            )
        } else {
            "Search, browse and play Spotify here. Playing needs Premium.".into()
        };
        let action: Element<'_, Message> = if status.connected {
            row![
                button(
                    container(
                        text(if st.importing {
                            "Importing…"
                        } else {
                            "Import library"
                        })
                        .size(14)
                    )
                    .center_y(Length::Fill)
                )
                .height(40)
                .padding([0, 20])
                .style(styles::primary(p))
                .on_press_maybe((!st.importing).then_some(Message::Spotify(Sp::ImportAll))),
                tonal_button(
                    "Disconnect",
                    Icon::Close,
                    p,
                    Message::Spotify(Sp::Disconnect)
                ),
            ]
            .spacing(8)
            .into()
        } else {
            let label = if st.connecting {
                "Waiting for Spotify…"
            } else {
                "Connect"
            };
            button(container(text(label.to_string()).size(14)).center_y(Length::Fill))
                .height(40)
                .padding([0, 24])
                .style(styles::primary(p))
                .on_press_maybe(
                    (!st.connecting && !st.client_id.trim().is_empty())
                        .then_some(Message::Spotify(Sp::Connect)),
                )
                .into()
        };
        let mut col = column![container(
            row![
                column![
                    text("Spotify").size(16),
                    text(hint).size(13).color(p.on_surface_variant)
                ]
                .spacing(2)
                .width(Length::Fill),
                action
            ]
            .spacing(24)
            .align_y(Alignment::Center)
        )
        .padding([14, 0])];
        if !st.error.is_empty() {
            col = col.push(
                container(text(st.error.clone()).size(13))
                    .padding([10, 14])
                    .width(Length::Fill)
                    .style(styles::error_box(p)),
            );
        }
        if !status.connected {
            col = col.push(
                column![
                    container(
                        text_input("Spotify client ID", &st.client_id)
                            .on_input(|s| Message::Spotify(Sp::ClientId(s)))
                            .size(14)
                            .padding([0, 16])
                            .style(move |_, _| iced::widget::text_input::Style {
                                background: p.surface_container_highest.into(),
                                border: iced::border::rounded(12),
                                icon: p.on_surface_variant,
                                placeholder: p.on_surface_variant,
                                value: p.on_surface,
                                selection: crate::theme::alpha(p.primary, 0.35),
                            }),
                    )
                    .height(44)
                    .center_y(44),
                    text("Spotify needs an app of your own: create one at developer.spotify.com/dashboard, add http://127.0.0.1:8898/callback as its redirect URI, and paste its client ID here.")
                        .size(12)
                        .color(p.on_surface_variant),
                ]
                .spacing(8)
                .padding(iced::Padding { bottom: 14.0, ..iced::Padding::ZERO }),
            );
        } else if cfg!(feature = "spotify") {
            // Windows and macOS play through the system web view; Soloist is Linux-only.
            let web_player = cfg!(any(windows, target_os = "macos"));
            let soloist = !web_player && !st.soloist_key.trim().is_empty();
            if status.premium || soloist {
                col = col.push(
                    container(tonal_button(
                        if soloist {
                            "Start Spotify Soloist"
                        } else {
                            "Test playback"
                        },
                        Icon::PlayArrow,
                        p,
                        Message::Settings(Msg::PlaybackLogin),
                    ))
                    .padding(iced::Padding {
                        bottom: 14.0,
                        ..iced::Padding::ZERO
                    }),
                );
            }
            if web_player {
                col = col.push(web_player_note(p));
            } else {
                col = col.push(self.soloist_settings(p));
            }
        }
        col.into()
    }

    pub(crate) fn youtube_settings(&self, p: Palette) -> Element<'_, Message> {
        use super::youtube::Msg as Yt;
        let st = &self.ui.youtube;
        let status = &st.status;
        let hint = if status.connected {
            format!(
                "Connected as {} with your own Google client.",
                status.user.clone().unwrap_or_default()
            )
        } else {
            "For developers: browse liked videos with your own Google Cloud OAuth client.".into()
        };
        let action: Element<'_, Message> = if status.connected {
            tonal_button(
                "Disconnect",
                Icon::Close,
                p,
                Message::Youtube(Yt::Disconnect),
            )
        } else {
            let label = if st.connecting {
                "Waiting for Google…"
            } else {
                "Connect"
            };
            button(container(text(label.to_string()).size(14)).center_y(Length::Fill))
                .height(40)
                .padding([0, 24])
                .style(styles::primary(p))
                .on_press_maybe(
                    (!st.connecting && !st.client_id.trim().is_empty())
                        .then_some(Message::Youtube(Yt::Connect)),
                )
                .into()
        };
        let mut col = column![container(
            row![
                column![
                    text("Own Google client (advanced)").size(16),
                    text(hint).size(13).color(p.on_surface_variant)
                ]
                .spacing(2)
                .width(Length::Fill),
                action
            ]
            .spacing(24)
            .align_y(Alignment::Center)
        )
        .padding([14, 0])];
        if !st.error.is_empty() {
            col = col.push(
                container(text(st.error.clone()).size(13))
                    .padding([10, 14])
                    .width(Length::Fill)
                    .style(styles::error_box(p)),
            );
        }
        if !status.connected {
            let field =
                move |placeholder: &str, value: &str, secure: bool, on: fn(String) -> Yt| {
                    container(
                        text_input(placeholder, value)
                            .on_input(move |s| Message::Youtube(on(s)))
                            .secure(secure)
                            .size(14)
                            .padding([0, 16])
                            .style(move |_, _| iced::widget::text_input::Style {
                                background: p.surface_container_highest.into(),
                                border: iced::border::rounded(12),
                                icon: p.on_surface_variant,
                                placeholder: p.on_surface_variant,
                                value: p.on_surface,
                                selection: crate::theme::alpha(p.primary, 0.35),
                            }),
                    )
                    .height(44)
                    .center_y(44)
                };
            col = col.push(
                column![
                    field("Google client ID", &st.client_id, false, Yt::ClientId),
                    field(
                        if status.has_secret {
                            "Client secret (saved, leave blank to keep)"
                        } else {
                            "Client secret"
                        },
                        &st.client_secret,
                        true,
                        Yt::ClientSecret
                    ),
                    text("Make an OAuth client in the Google Cloud console with the YouTube Data API v3 enabled, add http://127.0.0.1:8899/callback as a redirect URI, then paste its client ID and secret here.")
                        .size(12)
                        .color(p.on_surface_variant),
                ]
                .spacing(8)
                .padding(iced::Padding { bottom: 14.0, ..iced::Padding::ZERO }),
            );
        }
        col.into()
    }

    fn soloist_settings(&self, p: Palette) -> Element<'_, Message> {
        let st = &self.ui.spotify;
        let field = move |placeholder: &str, value: &str, secure: bool, on: fn(String) -> Msg| {
            container(
                text_input(placeholder, value)
                    .on_input(move |s| Message::Settings(on(s)))
                    .secure(secure)
                    .size(14)
                    .padding([0, 16])
                    .style(move |_, _| iced::widget::text_input::Style {
                        background: p.surface_container_highest.into(),
                        border: iced::border::rounded(12),
                        icon: p.on_surface_variant,
                        placeholder: p.on_surface_variant,
                        value: p.on_surface,
                        selection: crate::theme::alpha(p.primary, 0.35),
                    }),
            )
            .height(44)
            .center_y(44)
        };
        column![
            column![
                text("Spotify Soloist").size(16),
                text("Spotify no longer gives newer accounts playback keys through librespot. Soloist is Spotify's official headless player and works for those accounts. Download it from Spotify (Mosaic can't bundle it, and builds expire after 90 days), make an API key at developer.spotify.com/dashboard/soloist, then save. To pair, pick \u{201c}Mosaic\u{201d} from the devices menu in the Spotify app once. Soloist plays straight to your speakers, so EQ, leveling, crossfade and the visualizer don't apply to Spotify tracks.")
                    .size(13)
                    .color(p.on_surface_variant),
            ]
            .spacing(2),
            field("Soloist API key", &st.soloist_key, true, Msg::SoloistKey),
            field(
                "Path to soloist (blank: search PATH and ~/.local/bin)",
                &st.soloist_path,
                false,
                Msg::SoloistPath,
            ),
            row![
                primary_button("Save", Icon::Check, p, Message::Settings(Msg::SoloistSave)),
                tonal_button(
                    "Get Soloist",
                    Icon::OpenInNew,
                    p,
                    Message::Settings(Msg::SoloistDownload)
                ),
            ]
            .spacing(8),
        ]
        .spacing(10)
        .padding(iced::Padding {
            bottom: 14.0,
            ..iced::Padding::ZERO
        })
        .into()
    }
}

/// What Settings says about Spotify playback on Windows and macOS.
fn web_player_note<'a>(p: Palette) -> Element<'a, Message> {
    let engine = if cfg!(target_os = "macos") {
        "the system's web view (WKWebView; experimental, not yet tested on macOS)"
    } else {
        "the Microsoft Edge WebView2 runtime that comes with Windows"
    };
    column![
        text("Spotify player").size(16),
        text(format!(
            "Spotify tracks play through Spotify's own web player, running hidden in {engine}. It needs Premium. The player starts with the first Spotify track and stops after 10 minutes without one. It plays straight to your speakers, so EQ, leveling, crossfade and the visualizer don't apply to Spotify tracks. If Mosaic asks you to reconnect, disconnect and connect Spotify again above to grant the playback permission."
        ))
        .size(13)
        .color(p.on_surface_variant),
    ]
    .spacing(2)
    .padding(iced::Padding {
        bottom: 14.0,
        ..iced::Padding::ZERO
    })
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossfade_slider_snaps_to_whole_seconds() {
        assert_eq!(crossfade_from_fraction(0.0), 0.0);
        assert_eq!(crossfade_from_fraction(0.5), 6.0);
        assert_eq!(crossfade_from_fraction(0.51), 6.0);
        assert_eq!(crossfade_from_fraction(1.4), 12.0);
        assert_eq!(crossfade_from_fraction(-1.0), 0.0);
    }

    #[test]
    fn shortcut_table_matches_the_keyboard_handler() {
        let keys: Vec<_> = SHORTCUTS.iter().map(|(k, _)| *k).collect();
        for expected in ["Space", "M", "L", "Y", "F", "I", "/", "Ctrl + K"] {
            assert!(keys.contains(&expected), "{expected}");
        }
    }
}
