use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::widget::{column, container, image, mouse_area, row, stack, text, Space};
use iced::window::{self, Direction};
use iced::{event, keyboard, mouse, Element, Length, Point, Size, Subscription, Task, Theme};
use mosaic_audio::Engine;
use mosaic_core::model::{Repeat, Settings, Store};
use mosaic_core::storage::{Dirs, Saver};
use mosaic_services::Services;

use crate::ambient;
use crate::anim::{AnimationClock, Ease};
use crate::external::{self, External};
use crate::images::{ImageCache, Key as ImageKey};
use crate::player::{Effect, PlayerController, PlayerInput, Setting};
use crate::resolve::resolve_stream;
use crate::theme::{
    palette_from_seed, seed_from_rgba, styles, DynamicTheme, Palette, DEFAULT_SEED,
};
use crate::views::common;
use crate::widgets::icon::{icon, Icon};
use crate::widgets::resizer::{resizer, Axis};
use crate::widgets::toast;
use crate::widgets::{logo, rail};

pub use rail::View;

const BAR: (f64, f64, f64) = (88.0, 200.0, 100.0);
const SIDE: (f64, f64, f64) = (280.0, 560.0, 360.0);
const EDGE: f32 = 6.0;
const TITLEBAR: f32 = 40.0;
const MAC_INSET: f32 = 92.0;
/// Fade-through timings: the old page fades out quickly, the new one eases in.
const PAGE_OUT_MS: u64 = 90;
const PAGE_IN_MS: u64 = 320;
/// Animation frame interval: 120 fps at most (the present mode holds it to the display's rate
/// below that). Not every vsync: a frame that rebuilds the view costs a lot on a 360 Hz monitor.
/// Playback alone doesn't need these frames: the seek bar animates itself on redraws.
pub const FRAME: Duration = Duration::from_micros(8_333);

pub struct Flags {
    pub dirs: Dirs,
    pub store: Store,
    pub services: Services,
    pub engine: Option<Engine>,
    pub engine_error: Option<String>,
    /// Created before the event loop on Linux only. Elsewhere the handles aren't `Send` (this
    /// struct sits in a static) and are made on the main thread once the window is up.
    #[cfg(target_os = "linux")]
    pub media: Option<mosaic_platform::media_controls::Media>,
    #[cfg(target_os = "linux")]
    pub tray: Option<mosaic_platform::tray::Tray>,
    pub files: Vec<std::path::PathBuf>,
    pub migrated: bool,
    pub reset_window: bool,
    pub debug_theme: Option<String>,
    pub debug_screenshot: Option<std::path::PathBuf>,
    pub debug_view: Option<String>,
    pub debug_import: Option<String>,
}

pub static FLAGS: Mutex<Option<Flags>> = Mutex::new(None);

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Player(PlayerInput),
    Nop,
    Settings(crate::views::settings::Msg),
    Onboarding(crate::views::onboarding::Msg),
    UpdateTimer,
    Spotify(crate::views::spotify::Msg),
    Youtube(crate::views::youtube::Msg),
    Catalog(crate::views::catalog::Msg),
    /// Open the playing track's artist page (the artist name in the player bar).
    GoPlayingArtist,
    Import(crate::views::import::Msg),
    Lyrics(crate::views::lyrics::Msg),
    CloseNowPlaying,
    Library(crate::views::library::Msg),
    Local(crate::views::local::Msg),
    Queue(crate::views::queue::Msg),
    Reorder(crate::views::reorder::Msg),
    Scrolled(f32, f32),
    OpenImport,
    FileHovered(bool),
    FileDropped(std::path::PathBuf),
    DropFlush(u64),
    Track(crate::views::track::Msg),
    Discover(crate::views::discover::Msg),
    Palette(crate::views::palette::Msg),
    Scanned(Result<Vec<mosaic_core::model::Track>, String>),
    Opened(Vec<mosaic_core::model::Track>, Vec<String>),
    SeekFraction(f32),
    VolumeFraction(f32),
    ToggleLike,
    OpenNowPlaying,
    ToggleMini,
    ToggleLyrics,
    Tune(crate::views::tune::Msg),
    External(External),
    /// The main window is up: start the tray and media controls (macOS/Windows only). Carries
    /// the window's HWND on Windows.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    PlatformReady(Option<isize>),
    Nav(View),
    PlatformTab(View, crate::views::platform::Tab),
    Frame(Instant),
    Tick,
    PollImages,
    /// The next track's stream came in early (see `App::prefetched`).
    Prefetched(
        mosaic_core::model::TrackId,
        Result<crate::player::Stream, String>,
    ),
    /// The window's display scale (at start, and when it moves to another screen).
    Rescaled(f32),
    /// Lets go of artwork nothing has drawn for a while.
    TrimImages,
    ToastTick,
    WindowResized(Size),
    CloseRequested,
    DragWindow,
    Minimize,
    ToggleMaximize,
    ResizeEdge(Direction),
    ResizerPressed(Axis),
    ResizerMoved(Point),
    ResizerReleased,
    ResizerReset(Axis),
    Key(keyboard::Key, keyboard::Modifiers, bool),
    ArtLoaded(ImageKey, Result<Arc<::image::RgbaImage>, String>),
    Screenshot(window::Screenshot),
    ThemeArt {
        src: String,
        seed: Option<material_colors::color::Rgb>,
        ambient: Option<Arc<::image::RgbaImage>>,
    },
}

pub(crate) struct Toast {
    message: String,
    born: Instant,
    expires: Instant,
}

pub(crate) struct AmbientLayer {
    handle: image::Handle,
}

#[derive(Default)]
pub(crate) struct Ui {
    pub(crate) view: Option<View>,
    pub(crate) palette_open: bool,
    pub(crate) now_playing: bool,
    pub(crate) mini: bool,
    pub(crate) tune: Option<crate::views::tune::Tab>,
    /// The tab shown while the popover animates closed.
    pub(crate) tune_last: crate::views::tune::Tab,
    pub(crate) discover: crate::views::discover::State,
    pub(crate) menu: Option<crate::views::track::MenuState>,
    pub(crate) palette: crate::views::palette::State,
    pub(crate) importing: bool,
    pub(crate) update: mosaic_services::update::UpdateState,
    pub(crate) checking_update: bool,
    /// The downloaded installer for `update.latest`, or the download in flight.
    pub(crate) update_file: Option<std::path::PathBuf>,
    pub(crate) update_busy: bool,
    pub(crate) onboarding: Option<crate::views::onboarding::State>,
    pub(crate) hidden: bool,
    pub(crate) spotify: crate::views::spotify::State,
    pub(crate) youtube: crate::views::youtube::State,
    /// Artist, album and playlist pages opened from search or a library.
    pub(crate) catalog: crate::views::catalog::State,
    pub(crate) import: crate::views::import::State,
    pub(crate) lyrics: crate::views::lyrics::State,
    pub(crate) library: crate::views::library::State,
    pub(crate) local: crate::views::local::State,
    pub(crate) reorder: Option<crate::views::reorder::State>,
    pub(crate) scroll: (f32, f32),
    pub(crate) dropped: Vec<std::path::PathBuf>,
    pub(crate) drop_hover: bool,
    pub(crate) drop_seq: u64,
    /// A navigation waiting for the current page to fade out.
    pub(crate) nav: Option<View>,
    /// The sub-tab last picked on each platform view.
    pub(crate) platform_tabs: std::collections::HashMap<View, crate::views::platform::Tab>,
}

pub struct App {
    pub(crate) main: window::Id,
    pub(crate) size: Size,
    pub(crate) maximized: bool,
    pub(crate) decorations: bool,
    pub(crate) services: Services,
    pub(crate) store: Store,
    pub(crate) saver: Saver,
    pub(crate) engine: Option<Engine>,
    pub engine_error: Option<String>,
    pub(crate) player: PlayerController,
    pub(crate) ui: Ui,
    pub(crate) drag: Option<Axis>,
    pub(crate) theme: DynamicTheme,
    pub(crate) anim: AnimationClock,
    pub(crate) toasts: Vec<Toast>,
    pub(crate) toast_seq: u64,
    pub(crate) images: ImageCache,
    pub(crate) ambient: Option<AmbientLayer>,
    pub(crate) ambient_prev: Option<AmbientLayer>,
    pub(crate) art_src: String,
    pub(crate) media: Option<mosaic_platform::media_controls::Media>,
    pub(crate) tray: Option<mosaic_platform::tray::Tray>,
    pub(crate) tray_available: bool,
    pub(crate) media_track: Option<mosaic_core::model::TrackId>,
    pub(crate) discord: mosaic_services::discord::Discord,
    pub(crate) local: Vec<mosaic_core::model::Track>,
    pub(crate) recent: Vec<mosaic_core::model::Track>,
    pub(crate) cursor: Point,
    pub(crate) analyser: Option<mosaic_audio::Analyser>,
    pub(crate) viz: [f32; crate::widgets::visualizer::BARS],
    pub(crate) viz_last: Instant,
    pub(crate) normal_size: Size,
    pub(crate) now: Instant,
    pub(crate) pos_at: Instant,
    pub(crate) epoch: Instant,
    pub(crate) debug_screenshot: Option<std::path::PathBuf>,
    /// The window's display scale; artwork is decoded at this many pixels per logical pixel.
    pub(crate) scale: f32,
    /// The next track's stream, resolved before the playing one ends so the change of track
    /// doesn't wait on the network: (track, when it was asked for, the stream once it's in).
    pub(crate) prefetched: Option<Prefetch>,
}

/// The Win32 HWND behind an iced window, for SMTC. `None` everywhere else.
#[cfg(not(target_os = "linux"))]
fn native_hwnd(w: &dyn window::Window) -> Option<isize> {
    #[cfg(windows)]
    {
        use window::raw_window_handle::RawWindowHandle;
        match w.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
            _ => None,
        }
    }
    #[cfg(not(windows))]
    {
        let _ = w;
        None
    }
}

fn clamp(v: f64, (lo, hi, _): (f64, f64, f64)) -> f64 {
    v.clamp(lo, hi)
}

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let flags = FLAGS
            .lock()
            .ok()
            .and_then(|mut f| f.take())
            .expect("flags are set before the daemon starts");
        #[cfg(target_os = "linux")]
        let (media, tray) = (flags.media, flags.tray);
        #[cfg(not(target_os = "linux"))]
        let (media, tray): (
            Option<mosaic_platform::media_controls::Media>,
            Option<mosaic_platform::tray::Tray>,
        ) = (None, None);
        let (main, open) = window::open(window_settings());
        let decorations = decorations_enabled();
        let now = Instant::now();
        let saver = Saver::new(&flags.dirs);
        let analyser = flags.engine.as_ref().map(|e| e.analyser());
        let mut app = App {
            main,
            size: Size::new(1280.0, 820.0),
            maximized: false,
            decorations,
            services: flags.services,
            player: PlayerController::new(flags.store.settings.clone()),
            store: flags.store,
            saver,
            engine: flags.engine,
            engine_error: flags.engine_error,
            ui: Ui {
                view: Some(View::Discover),
                ..Ui::default()
            },
            drag: None,
            theme: DynamicTheme::default(),
            anim: AnimationClock::default(),
            toasts: Vec::new(),
            toast_seq: 0,
            images: ImageCache::default(),
            ambient: None,
            ambient_prev: None,
            art_src: String::new(),
            media,
            tray_available: tray.is_some(),
            tray,
            media_track: None,
            discord: mosaic_services::discord::Discord::spawn(),
            local: Vec::new(),
            recent: Vec::new(),
            cursor: Point::ORIGIN,
            analyser,
            viz: [0.0; crate::widgets::visualizer::BARS],
            viz_last: now,
            normal_size: Size::new(1280.0, 820.0),
            now,
            pos_at: now,
            epoch: now,
            debug_screenshot: flags.debug_screenshot,
            // Until the window says otherwise; 2× is sharp everywhere, just larger than needed.
            scale: 2.0,
            prefetched: None,
        };
        if let Some(v) = &flags.debug_view {
            app.ui.view = Some(match v.as_str() {
                "library" => View::Library,
                "playlists" => View::Playlists,
                "local" => View::Local,
                "queue" => View::Queue,
                "settings" => View::Settings,
                "soundcloud" => View::SoundCloud,
                "spotify" => View::Spotify,
                "youtube" => View::YouTube,
                "monochrome" => View::Monochrome,
                _ => View::Discover,
            });
            if v == "palette" {
                app.ui.palette_open = true;
            }
            if v == "nowplaying" {
                app.ui.now_playing = true;
            }
            if v == "stats" {
                app.ui.view = Some(View::Library);
                app.ui.library.tab = crate::views::library::Tab::Stats;
            }
        }
        if !app.services.config().onboarded.unwrap_or(false) {
            app.begin_onboarding();
        }
        // Older builds allowed a shorter bar than the controls fit in.
        app.store.settings.bar_height = clamp(app.store.settings.bar_height, BAR);
        if flags.reset_window {
            app.store.settings.bar_height = BAR.2;
            app.store.settings.side_width = SIDE.2;
            app.touch();
        }
        if flags.migrated {
            app.toast("Imported your library from the previous version");
        }
        app.retarget_rail();
        app.refresh_recent();
        #[cfg(target_os = "linux")]
        let open = open.discard();
        #[cfg(not(target_os = "linux"))]
        let open = open
            .then(move |_| window::run(main, native_hwnd))
            .map(Message::PlatformReady);
        let mut tasks = vec![
            open,
            window::scale_factor(main).map(Message::Rescaled),
            app.discover_boot(),
            app.scan(),
            app.spotify_boot(),
            app.youtube_boot(),
            Task::perform(tokio::time::sleep(Duration::from_secs(5)), |()| {
                Message::UpdateTimer
            }),
        ];
        if !flags.files.is_empty() {
            let files: Vec<String> = flags
                .files
                .iter()
                .map(|f| f.to_string_lossy().into_owned())
                .collect();
            tasks.push(app.open_files(files));
        }
        let fx = app.player.sync_engine();
        tasks.push(app.run_effects(fx));
        if let Some(session) = app.store.session.clone() {
            tasks.push(app.player_input(PlayerInput::Restore(session)));
        }
        if let Some(url) = flags.debug_import.clone() {
            app.ui.importing = true;
            app.ui.import.url = url;
            tasks.push(app.on_import(crate::views::import::Msg::Start));
        }
        // `search:<view>:<query>` and `open:<view>:<artist|album|playlist>:<id>` for screenshots
        // of a service's search or of a catalog page.
        if let Some(v) = flags.debug_view.as_deref() {
            let parts: Vec<&str> = v.splitn(4, ':').collect();
            let view = |name: &str| match name {
                "youtube" => View::YouTube,
                "soundcloud" => View::SoundCloud,
                "monochrome" => View::Monochrome,
                _ => View::Spotify,
            };
            match parts.as_slice() {
                ["search", name, query] => {
                    let view = view(name);
                    app.ui.view = Some(view);
                    if let Some(service) = crate::views::platform::service(view) {
                        app.ui.discover.service = service;
                    }
                    tasks
                        .push(app.on_discover(crate::views::discover::Msg::Query((*query).into())));
                    tasks.push(app.on_discover(crate::views::discover::Msg::Submit));
                }
                ["open", name, kind, id] => {
                    use mosaic_services::catalog::{Card, CardKind};
                    let view = view(name);
                    app.ui.view = Some(view);
                    let source = match view {
                        View::YouTube => mosaic_core::model::Source::YouTube,
                        View::SoundCloud => mosaic_core::model::Source::SoundCloud,
                        _ => mosaic_core::model::Source::Monochrome,
                    };
                    let kind = match *kind {
                        "artist" => CardKind::Artist,
                        "playlist" => CardKind::Playlist,
                        _ => CardKind::Album,
                    };
                    tasks.push(app.on_catalog(crate::views::catalog::Msg::Open(Card {
                        source,
                        kind,
                        id: (*id).into(),
                        title: String::new(),
                        subtitle: String::new(),
                        image: String::new(),
                    })));
                }
                _ => {}
            }
        }
        if flags.debug_view.as_deref() == Some("mini") {
            tasks.push(app.toggle_mini());
        }
        if app.debug_screenshot.is_some() {
            tasks.push(
                Task::future(tokio::time::sleep(
                    // `MOSAIC_SHOT_AFTER_MS` catches a moment mid-animation.
                    std::env::var("MOSAIC_SHOT_AFTER_MS")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .map(Duration::from_millis)
                        .unwrap_or(Duration::from_secs(if flags.debug_import.is_some() {
                            25
                        } else {
                            8
                        })),
                ))
                .then(move |_| window::screenshot(main))
                .map(Message::Screenshot),
            );
        }
        if let Some(url) = flags.debug_theme {
            tasks.push(app.load_theme_art(url));
        }
        (app, Task::batch(tasks))
    }

    pub(crate) fn view_kind(&self) -> View {
        self.ui.view.unwrap_or(View::Discover)
    }

    /// The view the rail shows as selected (a pending navigation counts).
    fn rail_view(&self) -> View {
        self.ui.nav.unwrap_or_else(|| self.view_kind())
    }

    pub(crate) fn palette(&self) -> Palette {
        self.theme.current
    }

    pub(crate) fn settings(&self) -> &Settings {
        &self.store.settings
    }

    fn rail_height(&self) -> f32 {
        let bar = self.settings().bar_height as f32;
        (self.size.height - TITLEBAR - 16.0 - bar - 16.0).max(0.0)
    }

    pub(crate) fn retarget_rail(&mut self) {
        let y = rail::pill_y(self.rail_view(), self.rail_height());
        self.anim.spring("rail-pill", y, 500.0, 36.0);
    }

    /// Eases the page area in after a navigation (the second half of a fade-through).
    pub(crate) fn page_transition(&mut self) {
        let now = Instant::now();
        self.anim
            .tween("page", 0.0, now, Duration::ZERO, Ease::Linear);
        self.anim.tween(
            "page",
            1.0,
            now,
            Duration::from_millis(PAGE_IN_MS),
            Ease::Decelerate,
        );
    }

    /// Switches to `view` once the current page has faded out.
    fn apply_nav(&mut self, view: View) -> Task<Message> {
        self.ui.view = Some(view);
        self.ui.scroll.0 = 0.0;
        if view == View::Library {
            self.ui.library.open = None;
        }
        self.retarget_rail();
        self.page_transition();
        match view {
            v if crate::views::platform::is_platform(v) => self.platform_entered(v),
            View::Queue => self.on_spotify(crate::views::spotify::Msg::PollQueue),
            _ => Task::none(),
        }
    }

    pub(crate) fn touch(&mut self) {
        self.saver.schedule(&self.store);
    }

    pub(crate) fn toast(&mut self, message: impl Into<String>) {
        self.toast_seq += 1;
        self.toasts.push(Toast {
            message: message.into(),
            born: self.now,
            expires: self.now + Duration::from_millis(toast::LIFETIME_MS),
        });
        let extra = self.toasts.len().saturating_sub(toast::MAX);
        self.toasts.drain(..extra);
    }

    pub(crate) fn set_setting(&mut self, f: impl FnOnce(&mut Settings)) -> Task<Message> {
        f(&mut self.store.settings);
        self.touch();
        let settings = self.store.settings.clone();
        self.player_input(PlayerInput::SettingsChanged(settings))
    }

    pub(crate) fn player_input(&mut self, input: PlayerInput) -> Task<Message> {
        let before = self.player.track.as_ref().map(|t| t.id.clone());
        let was_playing = self.player.playing;
        let was_spotify = self.playing_spotify();
        let volume = self.settings().volume;
        let fx = self.player.handle(input, Instant::now());
        self.services.set_spotify_leads(self.player.spotify_led());
        let after = self.player.track.as_ref().map(|t| t.id.clone());
        let task = Task::batch([
            self.run_effects(fx),
            self.sync_spotify(before == after, was_playing, was_spotify, volume),
        ]);
        let busy = self.player.playing && !self.player.loading;
        let now = Instant::now();
        self.anim.tween(
            "seek-amp",
            if busy { 1.0 } else { 0.02 },
            now,
            Duration::from_millis(450),
            Ease::Out,
        );
        self.anim.spring(
            "play-radius",
            if self.player.playing { 28.0 } else { 18.0 },
            420.0,
            28.0,
        );
        let prefetch = self.prefetch_next();
        if before != after {
            return Task::batch([task, prefetch, self.track_changed(), self.fetch_lyrics()]);
        }
        Task::batch([task, prefetch])
    }

    /// Resolves the next track's stream in the last `PREFETCH_SECS` of the playing one, so the
    /// change of track starts decoding straight away. Only for services whose stream is a fresh
    /// signed link (SoundCloud, YouTube); Monochrome's needs no lookup and Spotify loads in its
    /// own player.
    fn prefetch_next(&mut self) -> Task<Message> {
        let t = self.player.time;
        let near_end =
            self.player.playing && t.duration > 0.0 && t.duration - t.current < PREFETCH_SECS;
        let Some(next) = self.player.up_next().filter(|_| near_end).cloned() else {
            return Task::none();
        };
        if !matches!(
            next.source,
            mosaic_core::model::Source::SoundCloud | mosaic_core::model::Source::YouTube
        ) || self
            .prefetched
            .as_ref()
            .is_some_and(|(id, _, _)| *id == next.id)
        {
            return Task::none();
        }
        self.prefetched = Some((next.id.clone(), Instant::now(), None));
        let services = self.services.clone();
        let id = next.id.clone();
        Task::perform(
            async move { resolve_stream(&services, &next, 0.0).await },
            move |result| Message::Prefetched(id.clone(), result),
        )
    }

    /// Keeps the Spotify backend's play state and volume in step with the deck. Play/pause
    /// only follows a change on the same track: a track change loads (or adopts) on its own,
    /// and pausing in between would cut off a track Spotify already moved on to.
    fn sync_spotify(
        &mut self,
        same_track: bool,
        was_playing: bool,
        was_spotify: bool,
        volume_before: f64,
    ) -> Task<Message> {
        #[cfg(feature = "spotify")]
        {
            let is_spotify = self.playing_spotify();
            let playing = self.player.playing;
            let volume = self.settings().volume;
            let action = if was_spotify && !is_spotify {
                Some(0)
            } else if is_spotify && same_track && playing != was_playing && !self.player.loading {
                Some(if playing { 1 } else { 2 })
            } else {
                None
            };
            let set_volume = is_spotify && (!same_track || volume != volume_before);
            if action.is_some() || set_volume {
                let services = self.services.clone();
                return Task::future(async move {
                    // Never start a backend here: loading a track does that.
                    let Some(pb) = services.spotify_playback_current().await else {
                        return;
                    };
                    if set_volume {
                        pb.set_volume(volume);
                    }
                    match action {
                        Some(0) => pb.stop(),
                        Some(1) => pb.resume(),
                        Some(_) => pb.pause(),
                        None => {}
                    }
                })
                .discard();
            }
        }
        #[cfg(not(feature = "spotify"))]
        let _ = (same_track, was_playing, was_spotify, volume_before);
        Task::none()
    }

    fn track_changed(&mut self) -> Task<Message> {
        let src = self
            .player
            .track
            .as_ref()
            .map(|t| {
                if t.thumb.is_empty() {
                    t.artwork.clone()
                } else {
                    t.thumb.clone()
                }
            })
            .unwrap_or_default();
        if src == self.art_src {
            return Task::none();
        }
        self.art_src = src.clone();
        if src.is_empty() {
            self.theme
                .transition_to(palette_from_seed(DEFAULT_SEED), self.now);
            self.ambient_prev = self.ambient.take();
            self.anim.tween(
                "ambient",
                1.0,
                self.now,
                Duration::from_millis(1200),
                Ease::Linear,
            );
            return Task::none();
        }
        if let Some(cached) = self.theme.cached(&src) {
            self.theme.transition_to(cached, self.now);
        }
        self.load_theme_art(src)
    }

    fn load_theme_art(&mut self, src: String) -> Task<Message> {
        let services = self.services.clone();
        Task::perform(
            async move {
                let art = services.load_image(&src, 128).await.ok();
                let Some(art) = art else {
                    return Message::ThemeArt {
                        src,
                        seed: None,
                        ambient: None,
                    };
                };
                let (seed, ambient) = tokio::task::spawn_blocking(move || {
                    (seed_from_rgba(art.as_raw()), ambient::render(&art))
                })
                .await
                .unwrap_or((None, ::image::RgbaImage::new(1, 1)));
                Message::ThemeArt {
                    src,
                    seed,
                    ambient: Some(Arc::new(ambient)),
                }
            },
            |m| m,
        )
    }

    fn run_effects(&mut self, effects: Vec<Effect>) -> Task<Message> {
        let mut tasks = Vec::new();
        for effect in effects {
            match effect {
                Effect::Engine(cmd) => {
                    if let Some(engine) = &self.engine {
                        engine.send(cmd);
                    }
                }
                Effect::ResolveStream {
                    track,
                    gen,
                    seek_to,
                } => {
                    // Resolved ahead of time and still fresh: start with it right away.
                    let ready = match self.prefetched.take() {
                        Some((id, at, Some(Ok(stream))))
                            if id == track.id
                                && seek_to == 0.0
                                && at.elapsed() < PREFETCH_FRESH =>
                        {
                            Some(stream)
                        }
                        other => {
                            // Still on its way for another track: keep waiting for it.
                            if other
                                .as_ref()
                                .is_some_and(|(id, _, r)| *id != track.id && r.is_none())
                            {
                                self.prefetched = other;
                            }
                            None
                        }
                    };
                    if let Some(stream) = ready {
                        tasks.push(Task::done(Message::Player(PlayerInput::StreamResolved {
                            gen,
                            result: Ok(stream),
                        })));
                        continue;
                    }
                    let services = self.services.clone();
                    tasks.push(Task::perform(
                        async move { resolve_stream(&services, &track, seek_to).await },
                        move |result| Message::Player(PlayerInput::StreamResolved { gen, result }),
                    ));
                }
                Effect::FetchRelated { seed, purpose } => {
                    let services = self.services.clone();
                    let limit = match purpose {
                        crate::player::RelatedPurpose::Autoplay => 50,
                        crate::player::RelatedPurpose::Radio => 25,
                    };
                    // A few tracks before the seed steer the radio too (see `autoplay`).
                    let seeds = self.player.recent_seeds(&seed, 3);
                    let mixed = self.settings().mixed_sources;
                    // Songs heard lately are left out, so a session doesn't circle back to them.
                    let heard: std::collections::HashSet<String> = self
                        .store
                        .history
                        .iter()
                        .take(RECENTLY_HEARD)
                        .map(|h| mosaic_services::autoplay::song_key(&h.track))
                        .collect();
                    tasks.push(Task::perform(
                        async move {
                            let result = services
                                .autoplay_mixed(&seeds, limit as usize, mixed)
                                .await
                                .map(|list| without_heard(list, &heard))
                                .map_err(|e| e.to_string());
                            (seed, result)
                        },
                        move |(seed, result)| {
                            Message::Player(PlayerInput::Related {
                                purpose,
                                seed,
                                result,
                            })
                        },
                    ));
                }
                Effect::SpotifySkip => {
                    let services = self.services.clone();
                    tasks.push(
                        Task::future(async move {
                            if let Some(pb) = services.spotify_playback_current().await {
                                pb.skip_next();
                            }
                        })
                        .discard(),
                    );
                }
                Effect::Toast(message) => self.toast(message),
                Effect::RecordPlay(track) => {
                    self.store
                        .record_play(&track, chrono::Utc::now().timestamp_millis());
                    self.refresh_recent();
                    self.touch();
                }
                Effect::AddSeconds(track, secs) => {
                    self.store.add_seconds(
                        &track,
                        f64::from(secs),
                        chrono::Local::now().date_naive(),
                    );
                    self.touch();
                }
                Effect::SaveSession(patch) => {
                    self.store.save_session(patch);
                    self.touch();
                }
                Effect::Setting(setting) => {
                    match setting {
                        Setting::Volume(v) => self.store.settings.volume = v,
                        Setting::Speed(v) => self.store.settings.speed = v,
                        Setting::Shuffle(v) => self.store.settings.shuffle = v,
                        Setting::Repeat(v) => self.store.settings.repeat = v,
                    }
                    self.touch();
                }
                Effect::Presence(payload) => {
                    self.discord
                        .update(payload.map(|p| mosaic_services::discord::Payload {
                            title: p.title,
                            artist: p.artist,
                            album: p.album,
                            artwork: p.artwork,
                            url: p.url,
                            duration_ms: p.duration_ms,
                            position_ms: p.position_ms,
                        }))
                }
                Effect::PresenceEnabled(on) => self.discord.set_enabled(on),
                Effect::MediaControls(state) => self.update_media(&state),
            }
        }
        Task::batch(tasks)
    }

    pub(crate) fn refresh_recent(&mut self) {
        self.recent = self.recent_tracks();
    }

    pub(crate) fn scan(&mut self) -> Task<Message> {
        let sources = self.store.sources.clone();
        if sources.is_empty() {
            self.local.clear();
            return Task::none();
        }
        let services = self.services.clone();
        Task::perform(
            async move {
                services
                    .scan_paths(&sources)
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::Scanned,
        )
    }

    pub(crate) fn quit(&mut self) -> Task<Message> {
        let _ = self.saver.flush(&self.store);
        self.discord.shutdown();
        if let Some(engine) = &self.engine {
            engine.send(mosaic_audio::Command::Shutdown);
        }
        iced::exit()
    }

    pub(crate) fn show_window(&mut self) -> Task<Message> {
        self.ui.hidden = false;
        Task::batch([
            window::set_mode(self.main, window::Mode::Windowed),
            window::minimize(self.main, false),
            window::gain_focus(self.main),
            window::request_user_attention(self.main, Some(window::UserAttention::Informational)),
        ])
    }

    fn on_tray(&mut self, action: mosaic_platform::tray::TrayAction) -> Task<Message> {
        use mosaic_platform::tray::TrayAction as T;
        match action {
            T::Open => self.show_window(),
            T::Toggle => self.player_input(PlayerInput::Toggle),
            T::Next => self.player_input(PlayerInput::Next),
            T::Previous => self.player_input(PlayerInput::Prev),
            T::Quit => self.quit(),
        }
    }

    fn on_instance(&mut self, line: &str) -> Task<Message> {
        match mosaic_platform::single_instance::parse_open(line) {
            Some(path) => {
                Task::batch([self.show_window(), self.open_files(vec![path.to_string()])])
            }
            None => self.show_window(),
        }
    }

    /// Adds files or folders as sources and plays the first track found.
    pub(crate) fn open_files(&mut self, paths: Vec<String>) -> Task<Message> {
        let services = self.services.clone();
        self.store.add_sources(&paths);
        self.touch();
        let sources = self.store.sources.clone();
        self.ui.local.scanning = true;
        Task::perform(
            async move {
                services
                    .scan_paths(&sources)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |r| match r {
                Ok(tracks) => Message::Opened(tracks, paths.clone()),
                Err(e) => Message::Scanned(Err(e)),
            },
        )
    }

    fn update_media(&mut self, state: &crate::player::MediaState) {
        let Some(media) = &mut self.media else { return };
        let id = state.track.as_ref().map(|t| t.id.clone());
        if id != self.media_track {
            self.media_track = id;
            if let Some(t) = &state.track {
                media.set_metadata(&mosaic_platform::media_controls::Metadata {
                    title: t.title.clone(),
                    artist: t.artist.clone(),
                    album: t.album.clone(),
                    cover: mosaic_platform::media_controls::cover_url(&t.artwork),
                    duration: (t.duration > 0).then(|| Duration::from_millis(t.duration)),
                });
            }
        }
        media.set_playback(state.track.is_some(), state.playing, state.position);
    }

    fn on_media(&mut self, cmd: mosaic_platform::media_controls::MediaCommand) -> Task<Message> {
        use mosaic_platform::media_controls::MediaCommand as C;
        let playing = self.player.playing;
        let position = self.player.time.current;
        let input = match cmd {
            C::Toggle => PlayerInput::Toggle,
            C::Play if !playing => PlayerInput::Toggle,
            C::Pause if playing => PlayerInput::Toggle,
            C::Play | C::Pause => return Task::none(),
            C::Next => PlayerInput::Next,
            C::Previous => PlayerInput::Prev,
            C::Stop => PlayerInput::Stop,
            C::SeekTo(secs) => PlayerInput::Seek(secs),
            C::SeekBy(delta) => PlayerInput::Seek((position + delta).max(0.0)),
            C::Raise => return window::gain_focus(self.main),
        };
        self.player_input(input)
    }

    fn image_requests(&mut self) -> Task<Message> {
        let services = self.services.clone();
        Task::batch(self.images.take_wanted().into_iter().map(|key| {
            let services = services.clone();
            Task::perform(
                {
                    let key = key.clone();
                    async move {
                        services
                            .load_image(&key.0, key.1)
                            .await
                            .map_err(|e| e.to_string())
                    }
                },
                move |result| Message::ArtLoaded(key.clone(), result),
            )
        }))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        let task = self.handle(message);
        self.sync_presence();
        Task::batch([task, self.image_requests()])
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Player(input) => self.player_input(input),
            Message::External(External::Engine(event)) => {
                if matches!(
                    event,
                    mosaic_audio::Event::Position { .. } | mosaic_audio::Event::Seeked { .. }
                ) {
                    self.pos_at = Instant::now();
                }
                self.player_input(PlayerInput::Engine(event))
            }
            Message::External(External::Media(cmd)) => self.on_media(cmd),
            Message::External(External::SpotifyMoved { from, to }) => {
                self.on_spotify(crate::views::spotify::Msg::Moved { from, to })
            }
            Message::Nop => Task::none(),
            Message::Settings(msg) => self.on_settings(msg),
            Message::Onboarding(msg) => self.on_onboarding(msg),
            Message::PlatformReady(hwnd) => {
                if self.media.is_none() {
                    self.media = crate::start_media(hwnd);
                    self.media_track = None;
                }
                if self.tray.is_none() {
                    self.tray = crate::start_tray();
                    self.tray_available = self.tray.is_some();
                }
                Task::none()
            }
            Message::UpdateTimer => Task::batch([
                self.check_update(),
                Task::perform(tokio::time::sleep(mosaic_services::update::RECHECK), |()| {
                    Message::UpdateTimer
                }),
            ]),
            Message::External(External::Tray(action)) => self.on_tray(action),
            Message::External(External::Instance(line)) => self.on_instance(&line),
            Message::Spotify(msg) => self.on_spotify(msg),
            Message::Youtube(msg) => self.on_youtube(msg),
            Message::Catalog(msg) => self.on_catalog(msg),
            Message::GoPlayingArtist => match self.player.track.clone() {
                Some(t) if t.artist_id.is_some() && self.has_pages(&t.source) => {
                    let id = t.artist_id.clone().unwrap_or_default();
                    self.open_track_page(&t, mosaic_services::catalog::CardKind::Artist, id)
                }
                _ => Task::none(),
            },
            Message::Import(msg) => self.on_import(msg),
            Message::Lyrics(msg) => self.on_lyrics(msg),
            Message::CloseNowPlaying => {
                self.ui.now_playing = false;
                Task::none()
            }
            Message::Library(msg) => self.on_library(msg),
            Message::Local(msg) => self.on_local(msg),
            Message::Queue(msg) => self.on_queue(msg),
            Message::Reorder(msg) => {
                self.on_reorder(msg);
                Task::none()
            }
            Message::Scrolled(y, h) => {
                self.ui.scroll = (y, h);
                Task::none()
            }
            Message::OpenImport => {
                self.ui.importing = true;
                Task::none()
            }
            Message::FileHovered(on) => {
                self.ui.drop_hover = on;
                Task::none()
            }
            Message::FileDropped(path) => {
                self.ui.drop_hover = false;
                self.ui.dropped.push(path);
                self.ui.drop_seq += 1;
                let seq = self.ui.drop_seq;
                Task::perform(tokio::time::sleep(Duration::from_millis(80)), move |()| {
                    Message::DropFlush(seq)
                })
            }
            Message::DropFlush(seq) => {
                if seq != self.ui.drop_seq || self.ui.dropped.is_empty() {
                    return Task::none();
                }
                let paths: Vec<String> = std::mem::take(&mut self.ui.dropped)
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                let n = paths.len();
                self.ui.view = Some(View::Local);
                self.retarget_rail();
                self.toast(format!(
                    "Adding {n} item{} to your library",
                    if n > 1 { "s" } else { "" }
                ));
                self.add_sources(paths)
            }
            Message::Track(msg) => self.on_track(msg),
            Message::Discover(msg) => self.on_discover(msg),
            Message::Palette(msg) => self.on_palette(msg),
            Message::Opened(tracks, paths) => {
                let first = tracks
                    .iter()
                    .find(|t| {
                        self.services
                            .local_path(&t.id)
                            .is_some_and(|p| paths.iter().any(|q| p.starts_with(q)))
                    })
                    .cloned();
                let task = self.handle(Message::Scanned(Ok(tracks)));
                match first {
                    Some(t) => Task::batch([task, self.player_input(PlayerInput::Play(t, None))]),
                    None => task,
                }
            }
            Message::Scanned(result) => {
                self.ui.local.scanning = false;
                if let Ok(tracks) = result {
                    self.local = tracks;
                    self.rebuild_local();
                }
                Task::none()
            }
            Message::SeekFraction(f) => {
                let t = self.player.time;
                let (_, shown) =
                    crate::views::player_bar::shown_times(t.current, t.duration, t.offset, t.tail);
                self.pos_at = self.now;
                let seek = self.player_input(PlayerInput::Seek(f64::from(f) * shown + t.offset));
                Task::batch([seek, self.lyrics_tick()])
            }
            Message::VolumeFraction(f) => {
                let v = (f64::from(f) * 100.0).round() / 100.0;
                self.player_input(PlayerInput::SetVolume(v))
            }
            Message::ToggleLike => self.toggle_like(),
            Message::OpenNowPlaying => {
                self.ui.now_playing = true;
                Task::none()
            }
            Message::ToggleMini => self.toggle_mini(),
            Message::ToggleLyrics => {
                let open = self.settings().lyrics_open;
                self.set_setting(|s| s.lyrics_open = !open)
            }
            Message::Tune(msg) => self.on_tune(msg),
            Message::PlatformTab(view, tab) => self.on_platform_tab(view, tab),
            Message::Nav(view) => {
                if crate::anim::reduced_motion()
                    || (view == self.view_kind() && self.ui.nav.is_none())
                {
                    return self.apply_nav(view);
                }
                // Fade-through: the current page fades out, then the new one eases in. The rail
                // pill heads for the new item straight away.
                self.ui.nav = Some(view);
                let y = rail::pill_y(view, self.rail_height());
                self.anim.spring("rail-pill", y, 500.0, 36.0);
                let now = Instant::now();
                self.anim.tween(
                    "page",
                    0.0,
                    now,
                    Duration::from_millis(PAGE_OUT_MS),
                    Ease::Linear,
                );
                Task::none()
            }
            Message::Screenshot(shot) => {
                if let Some(path) = self.debug_screenshot.take() {
                    if let Some(img) = ::image::RgbaImage::from_raw(
                        shot.size.width,
                        shot.size.height,
                        shot.rgba.to_vec(),
                    ) {
                        let _ = img.save(path);
                    }
                }
                iced::exit()
            }
            Message::Frame(now) => {
                self.now = now;
                self.anim.tick(now);
                if let Some(view) = self.ui.nav {
                    if !self.anim.is_active(now) || self.anim.value("page", 0.0) <= 0.001 {
                        self.ui.nav = None;
                        return self.apply_nav(view);
                    }
                }
                self.theme.tick(now);
                self.viz_tick();
                self.video_tick();
                if self.ambient_prev.is_some() && self.anim.value("ambient", 1.0) >= 1.0 {
                    self.ambient_prev = None;
                }
                if self.settings().lyrics_open || self.ui.now_playing || self.ui.mini {
                    return self.lyrics_tick();
                }
                Task::none()
            }
            Message::Tick => {
                let tick = self.player_input(PlayerInput::Tick);
                #[cfg(feature = "spotify")]
                if self.player.spotify_due_soon() {
                    let services = self.services.clone();
                    let warm = Task::future(async move {
                        if services.spotify_playback_current().await.is_none() {
                            let _ = services.spotify_playback().await;
                        }
                    })
                    .discard();
                    return Task::batch([tick, warm]);
                }
                tick
            }
            Message::PollImages => Task::none(),
            Message::Prefetched(id, result) => {
                if let Some((want, _, slot)) = &mut self.prefetched {
                    if *want == id {
                        *slot = Some(result);
                    }
                }
                Task::none()
            }
            Message::Rescaled(scale) => {
                self.scale = scale.clamp(1.0, 4.0);
                Task::none()
            }
            Message::TrimImages => {
                if self.images.expire(self.now) > 0 {
                    self.services.trim_image_memory();
                    crate::trim_heap();
                }
                Task::none()
            }
            Message::ToastTick => {
                let now = self.now;
                self.toasts.retain(|t| toast::visible(t.expires, now));
                Task::none()
            }
            Message::WindowResized(size) => {
                self.size = size;
                self.retarget_rail();
                Task::none()
            }
            Message::CloseRequested => {
                if self.tray.is_some() && self.services.config().close_to_tray() {
                    self.ui.hidden = true;
                    return window::set_mode(self.main, window::Mode::Hidden);
                }
                self.quit()
            }
            Message::DragWindow => window::drag(self.main),
            Message::Minimize => window::minimize(self.main, true),
            Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                window::toggle_maximize(self.main)
            }
            Message::ResizeEdge(direction) => window::drag_resize(self.main, direction),
            Message::ResizerPressed(axis) => {
                self.drag = Some(axis);
                Task::none()
            }
            Message::ResizerMoved(point) => {
                if self.ui.reorder.is_some() {
                    self.cursor = point;
                    self.reorder_moved();
                }
                match self.drag {
                    Some(Axis::Horizontal) => {
                        let bar = clamp(
                            (f64::from(self.size.height) - 24.0 - f64::from(point.y)).round(),
                            BAR,
                        );
                        self.store.settings.bar_height = bar;
                    }
                    Some(Axis::Vertical) => {
                        let side = clamp(
                            (f64::from(self.size.width) - 24.0 - f64::from(point.x)).round(),
                            SIDE,
                        );
                        self.store.settings.side_width = side;
                    }
                    None => {}
                }
                self.retarget_rail();
                Task::none()
            }
            Message::ResizerReleased => {
                if self.drag.take().is_some() {
                    self.touch();
                }
                self.reorder_finish()
            }
            Message::ResizerReset(axis) => {
                match axis {
                    Axis::Horizontal => self.store.settings.bar_height = BAR.2,
                    Axis::Vertical => self.store.settings.side_width = SIDE.2,
                }
                self.touch();
                self.retarget_rail();
                Task::none()
            }
            Message::Key(key, modifiers, captured) => self.on_key(&key, modifiers, captured),
            Message::ArtLoaded(key, result) => {
                if let Ok(img) = result {
                    let handle = ImageCache::handle_for(img);
                    self.images.insert(key, handle, self.now);
                }
                Task::none()
            }
            Message::ThemeArt { src, seed, ambient } => {
                if src != self.art_src && !self.is_debug_theme(&src) {
                    return Task::none();
                }
                let palette = crate::theme::palette_from_seed(seed.unwrap_or(DEFAULT_SEED));
                self.theme.remember(src, palette);
                self.theme.transition_to(palette, self.now);
                if let Some(img) = ambient {
                    let handle = ImageCache::handle_for(img);
                    self.ambient_prev = self.ambient.take();
                    self.ambient = Some(AmbientLayer { handle });
                    self.anim
                        .tween("ambient", 0.0, self.now, Duration::ZERO, Ease::Linear);
                    self.anim.tween(
                        "ambient",
                        1.0,
                        self.now,
                        Duration::from_millis(1200),
                        Ease::Linear,
                    );
                }
                Task::none()
            }
        }
    }

    fn on_tune(&mut self, msg: crate::views::tune::Msg) -> Task<Message> {
        use crate::views::tune::{apply_eq, Msg};
        match msg {
            Msg::Toggle => {
                self.ui.tune = match self.ui.tune {
                    Some(_) => None,
                    None => Some(self.ui.tune_last),
                };
                Task::none()
            }
            Msg::Close => {
                self.ui.tune = None;
                Task::none()
            }
            Msg::Tab(tab) => {
                if let Some(old) = self.ui.tune.filter(|old| *old != tab) {
                    let dir = (crate::views::tune::tab_index(tab)
                        - crate::views::tune::tab_index(old))
                    .signum();
                    self.anim.reset("tune-dir", dir, 1.0, 1.0);
                    self.anim.reset("tune-pane", 0.0, 260.0, 30.0);
                    self.anim.spring("tune-pane", 1.0, 260.0, 30.0);
                }
                self.ui.tune = Some(tab);
                self.ui.tune_last = tab;
                Task::none()
            }
            Msg::Speed(v) => self.player_input(PlayerInput::SetSpeed(v)),
            Msg::Sleep(req) => self.player_input(PlayerInput::SetSleep(req)),
            eq => self.set_setting(|s| apply_eq(s, &eq)),
        }
    }

    fn is_debug_theme(&self, _src: &str) -> bool {
        self.art_src.is_empty()
    }

    fn on_key(
        &mut self,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        captured: bool,
    ) -> Task<Message> {
        use keyboard::key::Named;
        let ctrl = modifiers.command() || modifiers.control();
        if ctrl
            && matches!(key.as_ref(), keyboard::Key::Character(c) if c.eq_ignore_ascii_case("k"))
        {
            return self.open_palette();
        }
        if self.ui.palette_open {
            use crate::views::palette::Msg as P;
            match key.as_ref() {
                keyboard::Key::Named(Named::ArrowDown) => return self.on_palette(P::Move(1)),
                keyboard::Key::Named(Named::ArrowUp) => return self.on_palette(P::Move(-1)),
                _ => {}
            }
        }
        if matches!(key.as_ref(), keyboard::Key::Named(Named::Escape)) {
            if self.ui.lyrics.picker.take().is_some() {
                return Task::none();
            }
            if self.ui.importing {
                return self.on_import(crate::views::import::Msg::Close);
            }
            let overlay = self.ui.tune.is_some()
                || self.ui.menu.is_some()
                || self.ui.now_playing
                || self.ui.palette_open;
            self.ui.tune = None;
            self.ui.menu = None;
            self.ui.now_playing = false;
            self.ui.palette_open = false;
            if overlay {
                return Task::none();
            }
            // With nothing open, Esc steps back: out of an artist/album page, then out of a
            // search, back to the page it covered.
            if self.catalog_page(self.view_kind()).is_some() {
                return self.on_catalog(crate::views::catalog::Msg::Back);
            }
            if !self.ui.discover.query.is_empty() {
                return self.on_discover(crate::views::discover::Msg::Clear);
            }
            return Task::none();
        }
        if captured || self.ui.palette_open || ctrl || modifiers.alt() {
            return Task::none();
        }
        let volume = self.settings().volume;
        let step_volume = |delta: f64| ((volume + delta) * 100.0).round().clamp(0.0, 100.0) / 100.0;
        let position = self.player.time.current;
        let input = match key.as_ref() {
            keyboard::Key::Named(Named::Space) => Some(PlayerInput::Toggle),
            keyboard::Key::Named(Named::ArrowRight) if modifiers.shift() => Some(PlayerInput::Next),
            keyboard::Key::Named(Named::ArrowLeft) if modifiers.shift() => Some(PlayerInput::Prev),
            keyboard::Key::Named(Named::ArrowRight) => Some(PlayerInput::Seek(position + 5.0)),
            keyboard::Key::Named(Named::ArrowLeft) => {
                Some(PlayerInput::Seek((position - 5.0).max(0.0)))
            }
            keyboard::Key::Named(Named::ArrowUp) => Some(PlayerInput::SetVolume(step_volume(0.05))),
            keyboard::Key::Named(Named::ArrowDown) => {
                Some(PlayerInput::SetVolume(step_volume(-0.05)))
            }
            keyboard::Key::Character(c) => match c.to_lowercase().as_str() {
                "m" => Some(PlayerInput::SetVolume(if volume == 0.0 {
                    0.8
                } else {
                    0.0
                })),
                "s" => Some(PlayerInput::ToggleShuffle),
                "r" => Some(PlayerInput::CycleRepeat),
                "l" => return self.toggle_like(),
                "y" => {
                    let open = self.settings().lyrics_open;
                    return self.set_setting(|s| s.lyrics_open = !open);
                }
                "i" => return self.toggle_mini(),
                "f" => {
                    if self.player.track.is_some() {
                        self.ui.now_playing = !self.ui.now_playing;
                    }
                    return Task::none();
                }
                "/" => return self.start_search(String::new()),
                _ => None,
            },
            _ => None,
        };
        match input {
            Some(input) => self.player_input(input),
            None => Task::none(),
        }
    }

    pub fn toggle_like(&mut self) -> Task<Message> {
        let Some(track) = self.player.track.clone() else {
            return Task::none();
        };
        let liked = self.store.toggle_like(&track);
        self.touch();
        self.toast(if liked {
            "Added to Liked songs"
        } else {
            "Removed from Liked songs"
        });
        Task::none()
    }

    pub fn title(&self, _: window::Id) -> String {
        match &self.player.track {
            Some(t) if self.player.playing => format!("{} — {}", t.title, t.artist),
            _ => "Mosaic".into(),
        }
    }

    pub fn theme(&self, _: window::Id) -> Theme {
        self.palette().iced_theme()
    }

    pub fn style(&self, _: &Theme) -> iced::theme::Style {
        iced::theme::Style {
            background_color: self.palette().surface,
            text_color: self.palette().on_surface,
        }
    }

    /// Something is moving: springs, page and theme transitions, fades, or motion that lasts as
    /// long as the music (wave scroll, smoothed seek position, lyric fill, visualizer).
    fn needs_frames(&self) -> bool {
        self.theme.is_animating()
            || self.ui.nav.is_some()
            || self.anim.is_active(self.now)
            || !self.toasts.is_empty()
            || self.images.is_fading(self.now)
            || self.viz_active()
            || self.video_active()
            || (self.player.track.is_some()
                && (self.player.playing || self.player.loading)
                && (self.settings().lyrics_open || self.ui.now_playing || self.ui.mini))
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            window::close_requests().map(|_| Message::CloseRequested),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            Subscription::run(external_events).map(Message::External),
            event::listen_with(key_events),
        ];
        if self.player.wants_tick() {
            subs.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick));
        }
        if self.needs_frames() {
            subs.push(iced::time::every(FRAME).map(Message::Frame));
        }
        if self.drag.is_some() || self.ui.reorder.is_some() {
            subs.push(event::listen_with(drag_events));
        }
        if !self.images.is_empty() {
            subs.push(iced::time::every(crate::images::TRIM_EVERY).map(|_| Message::TrimImages));
        }
        if self.images.has_wanted() {
            subs.push(iced::time::every(Duration::from_millis(50)).map(|_| Message::PollImages));
        }
        if !self.toasts.is_empty() {
            subs.push(iced::time::every(Duration::from_millis(200)).map(|_| Message::ToastTick));
        }
        if self.playing_spotify() {
            // Cheap: it reads what Soloist last announced, so the queue keeps up with Spotify.
            subs.push(
                iced::time::every(Duration::from_secs(1))
                    .map(|_| Message::Spotify(crate::views::spotify::Msg::PollQueue)),
            );
        }
        Subscription::batch(subs)
    }

    pub fn view(&self, _: window::Id) -> Element<'_, Message> {
        let p = self.palette();
        let mut layers = if let Some(o) = &self.ui.onboarding {
            stack![self.onboarding_view(o, p), self.titlebar(p)]
        } else if self.ui.mini {
            stack![self.mini_view(p)]
        } else {
            let mut base = stack![self.ambient_view(p), self.shell(p)];
            let np = self.ui.now_playing && self.player.track.is_some();
            if let Some(t) = self.overlay("pres-now", np) {
                base = base.push(common::fade_up(
                    self.now_playing_view(p),
                    t,
                    16.0,
                    p.surface,
                ));
            }
            if let Some(layer) = self.video_layer(p) {
                base = base.push(layer);
            }
            base.push(self.titlebar(p))
        };
        if !self.decorations && !self.maximized {
            layers = layers.push(self.edges());
        }
        let messages: Vec<toast::Shown> = self
            .toasts
            .iter()
            .map(|t| toast::Shown {
                message: t.message.clone(),
                t: toast::progress(t.born, t.expires, self.now),
            })
            .collect();
        if let Some(t) = self.overlay("pres-import", self.ui.importing) {
            layers = layers.push(self.import_view(p, t));
        }
        if let Some(t) = self.overlay("pres-drop", self.ui.drop_hover) {
            layers = layers.push(self.drop_veil(p, t));
        }
        if let Some(menu) = &self.ui.menu {
            layers = layers.push(self.track_menu(menu, p));
        }
        if let Some(t) = self.overlay("pres-palette", self.ui.palette_open) {
            layers = layers.push(self.palette_view(p, t));
        }
        if let Some(t) = self.overlay("pres-tune", self.ui.tune.is_some()) {
            let tab = self.ui.tune.unwrap_or(self.ui.tune_last);
            layers = layers.push(self.tune_overlay(tab, t, p));
        }
        layers = layers.push(toast::view(&messages, p));
        container(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(styles::surface(p))
            .into()
    }

    fn drop_veil(&self, p: Palette, t: f32) -> Element<'_, Message> {
        let ink = crate::theme::alpha(p.on_primary_container, t);
        container(
            column![
                icon(Icon::UploadFile, 56.0).color(ink),
                text("Drop to add to Mosaic").size(26).color(ink),
                text("Music files and folders are added to your library")
                    .size(14)
                    .color(ink),
            ]
            .spacing(8)
            .align_x(iced::Alignment::Center),
        )
        .center(Length::Fill)
        .style(styles::fill(
            crate::theme::alpha(p.primary_container, 0.88 * t),
            0.0,
        ))
        .into()
    }

    pub(crate) fn ambient_view(&self, p: Palette) -> Element<'_, Message> {
        let fade = self.anim.value("ambient", 1.0);
        let layer = |l: &AmbientLayer, opacity: f32| {
            image(l.handle.clone())
                .content_fit(iced::ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .opacity(opacity)
                .scale(1.3_f32)
        };
        let mut layers = stack![container(Space::new())
            .width(Length::Fill)
            .height(Length::Fill)
            .style(styles::surface(p))]
        .width(Length::Fill)
        .height(Length::Fill);
        if let Some(prev) = &self.ambient_prev {
            layers = layers.push(layer(prev, 1.0));
        }
        if let Some(current) = &self.ambient {
            layers = layers.push(layer(current, fade));
        }
        layers = layers.push(
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_| container::Style {
                    background: Some(iced::Background::Gradient(
                        iced::gradient::Linear::new(std::f32::consts::PI)
                            .add_stop(0.0, iced::Color::TRANSPARENT)
                            .add_stop(
                                1.0,
                                crate::theme::mix(p.surface, iced::Color::TRANSPARENT, 0.7),
                            )
                            .into(),
                    )),
                    ..container::Style::default()
                }),
        );
        layers.into()
    }

    fn titlebar(&self, p: Palette) -> Element<'_, Message> {
        let inset = if cfg!(target_os = "macos") {
            MAC_INSET
        } else {
            20.0
        };
        let brand = row![
            logo::mark(16.0),
            text("Mosaic").size(13).color(p.on_surface_variant),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center);
        let drag = mouse_area(
            container(brand)
                .padding(iced::Padding {
                    left: inset,
                    ..iced::Padding::ZERO
                })
                .center_y(Length::Fill)
                .width(Length::Fill),
        )
        .on_press(Message::DragWindow)
        .on_double_click(Message::ToggleMaximize);

        let mut bar = row![drag].height(TITLEBAR).align_y(iced::Alignment::Center);
        if self.ui.mini {
            // The title bar sits over the mini player and swallows its clicks, so the way back
            // lives here.
            bar = bar.push(iced::widget::tooltip(
                iced::widget::button(container(icon(Icon::OpenInFull, 18.0)).center(Length::Fill))
                    .width(36)
                    .height(32)
                    .padding(0)
                    .style(styles::icon_button(p, false))
                    .on_press(Message::ToggleMini),
                container(text("Exit mini player (I)").size(12))
                    .padding([4, 8])
                    .style(styles::popup(p, 8.0)),
                iced::widget::tooltip::Position::Bottom,
            ));
            bar = bar.push(Space::new().width(6));
        }
        if !self.decorations && !cfg!(target_os = "macos") {
            let button = |glyph: Icon, msg: Message, close: bool| {
                iced::widget::button(container(icon(glyph, 16.0)).center(Length::Fill))
                    .width(46)
                    .height(TITLEBAR)
                    .padding(0)
                    .style(styles::window_button(p, close))
                    .on_press(msg)
            };
            bar = bar
                .push(button(Icon::Minimize, Message::Minimize, false))
                .push(button(
                    if self.maximized {
                        Icon::Restore
                    } else {
                        Icon::Maximize
                    },
                    Message::ToggleMaximize,
                    false,
                ))
                .push(button(Icon::Close, Message::CloseRequested, true));
        }
        container(bar).width(Length::Fill).height(TITLEBAR).into()
    }

    fn edges(&self) -> Element<'_, Message> {
        let edge = |dir: Direction, interaction: mouse::Interaction, w: Length, h: Length| {
            mouse_area(Space::new().width(w).height(h))
                .on_press(Message::ResizeEdge(dir))
                .interaction(interaction)
        };
        use mouse::Interaction as I;
        let c = Length::Fixed(EDGE * 2.0);
        let e = Length::Fixed(EDGE);
        let top = row![
            edge(Direction::NorthWest, I::ResizingDiagonallyDown, c, e),
            edge(Direction::North, I::ResizingVertically, Length::Fill, e),
            edge(Direction::NorthEast, I::ResizingDiagonallyUp, c, e),
        ];
        let middle = row![
            edge(Direction::West, I::ResizingHorizontally, e, Length::Fill),
            Space::new().width(Length::Fill),
            edge(Direction::East, I::ResizingHorizontally, e, Length::Fill),
        ]
        .height(Length::Fill);
        let bottom = row![
            edge(Direction::SouthWest, I::ResizingDiagonallyUp, c, e),
            edge(Direction::South, I::ResizingVertically, Length::Fill, e),
            edge(Direction::SouthEast, I::ResizingDiagonallyDown, c, e),
        ];
        column![top, middle, bottom].into()
    }

    fn shell(&self, p: Palette) -> Element<'_, Message> {
        let s = self.settings();
        let side_w = s.side_width as f32;
        let current = self.view_kind();

        let nav = rail::view(
            self.rail_view(),
            self.player.queue.list.len(),
            self.anim.value("rail-pill", 0.0),
            p,
            Message::Nav,
        );
        let content: Element<'_, Message> = match current {
            View::Discover => self.discover_view(p),
            View::Library | View::Playlists => self.library_view(p),
            View::Local => self.local_view(p),
            View::Queue => self.queue_view(p),
            View::SoundCloud | View::Spotify | View::YouTube | View::Monochrome => {
                self.platform_view(current, p)
            }
            View::Settings => self.settings_view(p),
        };
        let t = self.anim.value("page", 1.0).clamp(0.0, 1.0);
        let rise = if self.ui.nav.is_some() {
            0.0
        } else {
            20.0 * (1.0 - t)
        };
        let content = stack![
            container(content)
                .padding(iced::Padding {
                    top: rise,
                    ..iced::Padding::ZERO
                })
                .width(Length::Fill)
                .height(Length::Fill),
            container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(styles::fill(
                    crate::theme::alpha(p.surface, (1.0 - t) * 0.9),
                    0.0
                )),
        ];
        let main = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .clip(true)
            .style(styles::glass(p));

        let mut body = row![nav, Space::new().width(16), main].height(Length::Fill);
        if let Some(t) = self.overlay("pres-lyrics", s.lyrics_open) {
            body = body
                .push(resizer(
                    Axis::Vertical,
                    self.drag == Some(Axis::Vertical),
                    p,
                    Message::ResizerPressed(Axis::Vertical),
                    Message::ResizerReset(Axis::Vertical),
                ))
                .push(
                    container(self.lyrics_panel(p))
                        .width(side_w * t)
                        .height(Length::Fill)
                        .clip(true)
                        .style(styles::glass(p)),
                );
        }
        let bar = self.player_bar(p);
        let gap = row![
            Space::new().width(rail::WIDTH),
            resizer(
                Axis::Horizontal,
                self.drag == Some(Axis::Horizontal),
                p,
                Message::ResizerPressed(Axis::Horizontal),
                Message::ResizerReset(Axis::Horizontal),
            ),
        ];
        let mut layout = column![];
        if let Some(message) = &self.engine_error {
            layout = layout.push(
                container(text(message.clone()).size(14))
                    .padding([10, 16])
                    .width(Length::Fill)
                    .style(styles::error_box(p)),
            );
        }
        layout
            .push(body)
            .push(gap)
            .push(bar)
            .padding(iced::Padding {
                top: TITLEBAR,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            })
            .into()
    }
}

/// A stream resolved ahead: the track, when it was asked for, the stream once it's in.
pub(crate) type Prefetch = (
    mosaic_core::model::TrackId,
    Instant,
    Option<Result<crate::player::Stream, String>>,
);

/// How long before a track ends the next one's stream is resolved.
const PREFETCH_SECS: f64 = 30.0;
/// A resolved stream older than this isn't used: signed links expire.
const PREFETCH_FRESH: Duration = Duration::from_secs(5 * 60);

/// How much of the history autoplay avoids repeating.
const RECENTLY_HEARD: usize = 150;
/// Autoplay keeps recently heard songs rather than come back with almost nothing.
const MIN_FRESH: usize = 10;

/// `list` without songs in `heard`, unless that leaves too few to be worth it.
fn without_heard(
    list: Vec<mosaic_core::model::Track>,
    heard: &std::collections::HashSet<String>,
) -> Vec<mosaic_core::model::Track> {
    let fresh: Vec<_> = list
        .iter()
        .filter(|t| !heard.contains(&mosaic_services::autoplay::song_key(t)))
        .cloned()
        .collect();
    if fresh.len() >= MIN_FRESH.min(list.len()) {
        fresh
    } else {
        list
    }
}

static CURSOR: AtomicU64 = AtomicU64::new(0);

fn pack(p: Point) -> u64 {
    (u64::from(p.x.to_bits()) << 32) | u64::from(p.y.to_bits())
}

/// Last pointer position, tracked without waking the update loop.
pub(crate) fn cursor_now() -> Point {
    let v = CURSOR.load(Ordering::Relaxed);
    Point::new(f32::from_bits((v >> 32) as u32), f32::from_bits(v as u32))
}

fn key_events(event: iced::Event, status: event::Status, _: window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => Some(
            Message::Key(key, modifiers, status == event::Status::Captured),
        ),
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            // Mouse moves must not reach `update`: each message rebuilds the whole view.
            CURSOR.store(pack(position), Ordering::Relaxed);
            None
        }
        iced::Event::Window(window::Event::Rescaled(scale)) => Some(Message::Rescaled(scale)),
        iced::Event::Window(window::Event::FileHovered(_)) => Some(Message::FileHovered(true)),
        iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FileHovered(false)),
        iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
        _ => None,
    }
}

fn drag_events(event: iced::Event, _: event::Status, _: window::Id) -> Option<Message> {
    match event {
        iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
            Some(Message::ResizerMoved(position))
        }
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::ResizerReleased)
        }
        _ => None,
    }
}

fn external_events() -> impl iced::futures::Stream<Item = External> {
    use iced::futures::SinkExt;
    iced::stream::channel(64, async |mut output| {
        let Some(mut rx) = external::take_receiver() else {
            return std::future::pending::<()>().await;
        };
        while let Some(event) = rx.recv().await {
            if output.send(event).await.is_err() {
                break;
            }
        }
    })
}

/// Off by default (we draw our own title bar) except on macOS, where winit cannot drag-resize
/// an undecorated window and the traffic lights are expected; there the transparent,
/// full-size title bar keeps the look. `MOSAIC_DECORATIONS=1`/`=0` overrides.
fn decorations_enabled() -> bool {
    match std::env::var_os("MOSAIC_DECORATIONS") {
        Some(v) => v == "1",
        None => cfg!(target_os = "macos"),
    }
}

pub fn window_settings() -> window::Settings {
    const ICON: &[u8] = include_bytes!("../../assets/icon.png");
    window::Settings {
        size: Size::new(1280.0, 820.0),
        min_size: Some(Size::new(980.0, 640.0)),
        decorations: decorations_enabled(),
        icon: window::icon::from_file_data(ICON, None).ok(),
        #[cfg(target_os = "linux")]
        platform_specific: window::settings::PlatformSpecific {
            application_id: "mosaic".into(),
            ..Default::default()
        },
        #[cfg(target_os = "macos")]
        platform_specific: window::settings::PlatformSpecific {
            titlebar_transparent: true,
            fullsize_content_view: true,
            title_hidden: true,
        },
        ..Default::default()
    }
}

#[allow(dead_code)]
fn repeat_label(r: Repeat) -> &'static str {
    match r {
        Repeat::Off => "off",
        Repeat::All => "all",
        Repeat::One => "one",
    }
}
