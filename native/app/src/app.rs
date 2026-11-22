use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::widget::{column, container, image, mouse_area, row, stack, text, Space};
use iced::window::{self, Direction};
use iced::{event, keyboard, mouse, Element, Length, Point, Size, Subscription, Task, Theme};
use mosaic_audio::Engine;
use mosaic_core::model::{Config, Repeat, Settings, Store};
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
use crate::widgets::icon::{icon, Icon};
use crate::widgets::resizer::{resizer, Axis};
use crate::widgets::toast;
use crate::widgets::{logo, rail};

pub use rail::View;

const BAR: (f64, f64, f64) = (80.0, 200.0, 100.0);
const SIDE: (f64, f64, f64) = (280.0, 560.0, 360.0);
const EDGE: f32 = 6.0;
const TITLEBAR: f32 = 40.0;
const MAC_INSET: f32 = 92.0;

pub struct Flags {
    pub dirs: Dirs,
    pub config: Arc<Mutex<Config>>,
    pub store: Store,
    pub services: Services,
    pub engine: Option<Engine>,
    pub engine_error: Option<String>,
    pub media: Option<mosaic_platform::media_controls::Media>,
    pub debug_theme: Option<String>,
    pub debug_screenshot: Option<std::path::PathBuf>,
}

pub static FLAGS: Mutex<Option<Flags>> = Mutex::new(None);

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Message {
    Player(PlayerInput),
    Nop,
    Track(crate::views::track::Msg),
    Discover(crate::views::discover::Msg),
    Palette(crate::views::palette::Msg),
    Cursor(Point),
    Scanned(Result<Vec<mosaic_core::model::Track>, String>),
    SeekFraction(f32),
    VolumeFraction(f32),
    ToggleLike,
    OpenNowPlaying,
    ToggleMini,
    ToggleLyrics,
    Tune(crate::views::tune::Msg),
    External(External),
    Nav(View),
    Frame(Instant),
    Tick(Instant),
    PollImages,
    ToastTick,
    WindowResized(Size),
    CloseRequested(window::Id),
    DragWindow,
    Minimize,
    ToggleMaximize,
    Maximized(bool),
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
    id: u64,
    message: String,
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
    pub(crate) discover: crate::views::discover::State,
    pub(crate) menu: Option<crate::views::track::MenuState>,
    pub(crate) palette: crate::views::palette::State,
    pub(crate) importing: bool,
}

pub struct App {
    pub(crate) main: window::Id,
    pub(crate) size: Size,
    pub(crate) maximized: bool,
    pub(crate) decorations: bool,
    pub(crate) dirs: Dirs,
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
    pub(crate) media_track: Option<mosaic_core::model::TrackId>,
    pub(crate) discord: mosaic_services::discord::Discord,
    pub(crate) local: Vec<mosaic_core::model::Track>,
    pub(crate) recent: Vec<mosaic_core::model::Track>,
    pub(crate) cursor: Point,
    pub(crate) now: Instant,
    pub(crate) pos_at: Instant,
    pub(crate) epoch: Instant,
    pub(crate) debug_screenshot: Option<std::path::PathBuf>,
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
        let (main, open) = window::open(window_settings());
        let decorations = decorations_enabled();
        let now = Instant::now();
        let saver = Saver::new(&flags.dirs);
        let mut app = App {
            main,
            size: Size::new(1280.0, 820.0),
            maximized: false,
            decorations,
            dirs: flags.dirs,
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
            media: flags.media,
            media_track: None,
            discord: mosaic_services::discord::Discord::spawn(),
            local: Vec::new(),
            recent: Vec::new(),
            cursor: Point::ORIGIN,
            now,
            pos_at: now,
            epoch: now,
            debug_screenshot: flags.debug_screenshot,
        };
        app.retarget_rail();
        app.refresh_recent();
        let mut tasks = vec![open.discard(), app.discover_boot(), app.scan()];
        let fx = app.player.sync_engine();
        tasks.push(app.run_effects(fx));
        if let Some(session) = app.store.session.clone() {
            tasks.push(app.player_input(PlayerInput::Restore(session)));
        }
        if app.debug_screenshot.is_some() {
            tasks.push(
                Task::future(tokio::time::sleep(Duration::from_secs(8)))
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
        let y = rail::pill_y(self.view_kind(), false, self.rail_height());
        self.anim.spring("rail-pill", y, 500.0, 36.0);
    }

    pub(crate) fn touch(&mut self) {
        self.saver.schedule(&self.store);
    }

    pub(crate) fn toast(&mut self, message: impl Into<String>) {
        self.toast_seq += 1;
        self.toasts.push(Toast {
            id: self.toast_seq,
            message: message.into(),
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
        let fx = self.player.handle(input, Instant::now());
        let task = self.run_effects(fx);
        let after = self.player.track.as_ref().map(|t| t.id.clone());
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
        if before != after {
            return Task::batch([task, self.track_changed()]);
        }
        task
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
                Effect::ResolveStream { track, gen } => {
                    let services = self.services.clone();
                    tasks.push(Task::perform(
                        async move { resolve_stream(&services, &track).await },
                        move |result| Message::Player(PlayerInput::StreamResolved { gen, result }),
                    ));
                }
                Effect::FetchRelated { seed, purpose } => {
                    let services = self.services.clone();
                    let limit = match purpose {
                        crate::player::RelatedPurpose::Autoplay => 40,
                        crate::player::RelatedPurpose::Radio => 20,
                    };
                    tasks.push(Task::perform(
                        async move {
                            let result = services
                                .sc_related(&seed.id, limit)
                                .await
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
                Effect::Toast(message) => self.toast(message),
                Effect::RecordPlay(track) => {
                    self.store
                        .record_play(&track, chrono::Utc::now().timestamp_millis());
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
            Message::Nop => Task::none(),
            Message::Track(msg) => self.on_track(msg),
            Message::Discover(msg) => self.on_discover(msg),
            Message::Palette(msg) => self.on_palette(msg),
            Message::Cursor(p) => {
                self.cursor = p;
                Task::none()
            }
            Message::Scanned(result) => {
                if let Ok(tracks) = result {
                    self.local = tracks;
                }
                Task::none()
            }
            Message::SeekFraction(f) => {
                let t = self.player.time;
                let (_, shown) =
                    crate::views::player_bar::shown_times(t.current, t.duration, t.offset, t.tail);
                self.pos_at = self.now;
                self.player_input(PlayerInput::Seek(f64::from(f) * shown + t.offset))
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
            Message::ToggleMini => {
                self.ui.mini = !self.ui.mini;
                Task::none()
            }
            Message::ToggleLyrics => {
                let open = self.settings().lyrics_open;
                self.set_setting(|s| s.lyrics_open = !open)
            }
            Message::Tune(msg) => self.on_tune(msg),
            Message::Nav(view) => {
                self.ui.view = Some(view);
                self.retarget_rail();
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
                self.theme.tick(now);
                if self.ambient_prev.is_some() && self.anim.value("ambient", 1.0) >= 1.0 {
                    self.ambient_prev = None;
                }
                Task::none()
            }
            Message::Tick(_) => self.player_input(PlayerInput::Tick),
            Message::PollImages => Task::none(),
            Message::ToastTick => {
                let now = self.now;
                self.toasts.retain(|t| t.expires > now);
                Task::none()
            }
            Message::WindowResized(size) => {
                self.size = size;
                self.retarget_rail();
                Task::none()
            }
            Message::CloseRequested(_) => {
                let _ = self.saver.flush(&self.store);
                self.discord.shutdown();
                iced::exit()
            }
            Message::DragWindow => window::drag(self.main),
            Message::Minimize => window::minimize(self.main, true),
            Message::ToggleMaximize => {
                self.maximized = !self.maximized;
                window::toggle_maximize(self.main)
            }
            Message::Maximized(on) => {
                self.maximized = on;
                Task::none()
            }
            Message::ResizeEdge(direction) => window::drag_resize(self.main, direction),
            Message::ResizerPressed(axis) => {
                self.drag = Some(axis);
                Task::none()
            }
            Message::ResizerMoved(point) => {
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
                Task::none()
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
                    let handle =
                        image::Handle::from_rgba(img.width(), img.height(), img.as_raw().clone());
                    self.images.insert(key, handle, self.now);
                }
                Task::none()
            }
            Message::ThemeArt { src, seed, ambient } => {
                if src != self.art_src && !self.is_debug_theme(&src) {
                    return Task::none();
                }
                let palette = palette_from_seed(seed.unwrap_or(DEFAULT_SEED));
                self.theme.remember(src, palette);
                self.theme.transition_to(palette, self.now);
                if let Some(img) = ambient {
                    let handle =
                        image::Handle::from_rgba(img.width(), img.height(), img.as_raw().clone());
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
        use crate::views::tune::{apply_eq, Msg, Tab};
        match msg {
            Msg::Toggle => {
                self.ui.tune = match self.ui.tune {
                    Some(_) => None,
                    None => Some(Tab::Equalizer),
                };
                Task::none()
            }
            Msg::Close => {
                self.ui.tune = None;
                Task::none()
            }
            Msg::Tab(tab) => {
                self.ui.tune = Some(tab);
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
            self.ui.palette_open = !self.ui.palette_open;
            return Task::none();
        }
        if matches!(key.as_ref(), keyboard::Key::Named(Named::Escape)) {
            self.ui.tune = None;
            self.ui.menu = None;
            self.ui.now_playing = false;
            self.ui.palette_open = false;
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
                "i" => {
                    self.ui.mini = !self.ui.mini;
                    return Task::none();
                }
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

    fn needs_frames(&self) -> bool {
        self.theme.is_animating()
            || self.anim.is_active(self.now)
            || self.images.is_fading(self.now)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            window::close_requests().map(Message::CloseRequested),
            window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            Subscription::run(external_events).map(Message::External),
            event::listen_with(key_events),
            iced::time::every(Duration::from_secs(1)).map(Message::Tick),
        ];
        if self.needs_frames() {
            subs.push(window::frames().map(Message::Frame));
        }
        if self.drag.is_some() {
            subs.push(event::listen_with(drag_events));
        }
        if self.images.has_wanted() {
            subs.push(iced::time::every(Duration::from_millis(30)).map(|_| Message::PollImages));
        }
        if !self.toasts.is_empty() {
            subs.push(iced::time::every(Duration::from_millis(200)).map(|_| Message::ToastTick));
        }
        Subscription::batch(subs)
    }

    pub fn view(&self, _: window::Id) -> Element<'_, Message> {
        let p = self.palette();
        let mut layers = stack![self.ambient_view(p), self.shell(p), self.titlebar(p)];
        if !self.decorations && !self.maximized {
            layers = layers.push(self.edges());
        }
        let messages: Vec<(u64, String)> = self
            .toasts
            .iter()
            .map(|t| (t.id, t.message.clone()))
            .collect();
        if let Some(menu) = &self.ui.menu {
            layers = layers.push(self.track_menu(menu, p));
        }
        if self.ui.palette_open {
            layers = layers.push(self.palette_view(p));
        }
        if let Some(tab) = self.ui.tune {
            layers = layers.push(self.tune_overlay(tab, p));
        }
        layers = layers.push(toast::view(&messages, p));
        container(layers)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(styles::surface(p))
            .into()
    }

    fn ambient_view(&self, p: Palette) -> Element<'_, Message> {
        let fade = self.anim.value("ambient", 1.0);
        let layer = |l: &AmbientLayer, opacity: f32| {
            image(l.handle.clone())
                .content_fit(iced::ContentFit::Cover)
                .width(Length::Fill)
                .height(Length::Fill)
                .opacity(opacity)
                .scale(1.3_f32)
        };
        let mut layers = stack![container(Space::new()).style(styles::surface(p))]
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

        let mut bar = row![drag].height(TITLEBAR);
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
                .push(button(
                    Icon::Close,
                    Message::CloseRequested(self.main),
                    true,
                ));
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
            current,
            self.player.queue.list.len(),
            false,
            self.anim.value("rail-pill", 0.0),
            p,
            Message::Nav,
        );
        let content: Element<'_, Message> = match current {
            View::Discover => self.discover_view(p),
            other => container(text(other.label()).size(28).color(p.on_surface))
                .padding([32, 40])
                .into(),
        };
        let main = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(styles::glass(p));

        let mut body = row![nav, Space::new().width(16), main].height(Length::Fill);
        if s.lyrics_open {
            body = body
                .push(resizer(
                    Axis::Vertical,
                    self.drag == Some(Axis::Vertical),
                    p,
                    Message::ResizerPressed(Axis::Vertical),
                    Message::ResizerReset(Axis::Vertical),
                ))
                .push(
                    container(Space::new())
                        .width(side_w)
                        .height(Length::Fill)
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
        column![body, gap, bar]
            .padding(iced::Padding {
                top: TITLEBAR,
                right: 16.0,
                bottom: 16.0,
                left: 16.0,
            })
            .into()
    }
}

fn key_events(event: iced::Event, status: event::Status, _: window::Id) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => Some(
            Message::Key(key, modifiers, status == event::Status::Captured),
        ),
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

fn decorations_enabled() -> bool {
    std::env::var_os("MOSAIC_DECORATIONS").is_some_and(|v| v == "1")
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
