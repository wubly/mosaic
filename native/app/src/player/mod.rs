//! UI-agnostic port of `PlayerProvider.js`, `useSleepTimer.js`, `useListeningStats.js`,
//! `usePresenceSync.js` and the session half of `useSessionPersistence.js`.
//!
//! The controller never touches the engine or the network: it consumes [`PlayerInput`]s and returns
//! [`Effect`]s for the app to execute.

mod crossfade;
mod load;
mod queue_ops;
mod sleep;
#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use mosaic_audio::{AfterFade, Command, Curve, DeckId, Event, SourceSpec};
use mosaic_core::model::{Repeat, Session, Settings, Source, Track, TrackId};
use mosaic_core::queue::{step_index, Queue};
use mosaic_core::relevance::filter_along;
use mosaic_services::soundcloud::silence::Silence;

pub use sleep::{Sleep, SleepRequest};

const STAT_AFTER: Duration = Duration::from_secs(20);
const STAT_TICK: Duration = Duration::from_secs(15);
const SEEK_PRESENCE_DEBOUNCE: Duration = Duration::from_millis(800);
const POSITION_SAVE_EVERY: Duration = Duration::from_secs(4);
const MEDIA_REFRESH_EVERY: Duration = Duration::from_secs(5);
const SESSION_BEFORE: usize = 20;
const SESSION_SIZE: usize = 300;
const PAUSE_FADE_MS: u32 = 250;
const RESUME_FADE_MS: u32 = 300;
const PREV_RESTARTS_AFTER: f64 = 3.0;

/// Autoplay tops the queue up once fewer than this many tracks follow the current one.
const AUTOPLAY_LOW_WATER: isize = 2;

pub const UNAVAILABLE_YOUTUBE: &str = "YouTube isn’t available right now";
pub const UNAVAILABLE_SOURCE: &str = "Not available in this version";
pub const UNAVAILABLE_SPOTIFY: &str = "Connect Spotify in the Spotify tab";

/// What the service layer resolved for a track.
#[derive(Clone, Debug)]
pub struct Stream {
    pub source: SourceSpec,
    pub silence: Option<Silence>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Time {
    /// Source-time position, like `audio.currentTime`.
    pub current: f64,
    pub duration: f64,
    pub offset: f64,
    pub tail: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelatedPurpose {
    Autoplay,
    Radio,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresencePayload {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub artwork: String,
    pub url: Option<String>,
    pub duration_ms: f64,
    pub position_ms: f64,
}

#[derive(Clone, Debug)]
pub struct MediaState {
    pub track: Option<Track>,
    pub playing: bool,
    pub position: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Setting {
    Volume(f64),
    Speed(f64),
    Shuffle(bool),
    Repeat(Repeat),
}

#[derive(Debug)]
pub enum Effect {
    Engine(Command),
    ResolveStream {
        track: Track,
        gen: u64,
        seek_to: f64,
    },
    FetchRelated {
        seed: Track,
        purpose: RelatedPurpose,
    },
    /// Ask Spotify to skip to its own next track.
    SpotifySkip,
    Toast(String),
    RecordPlay(Track),
    AddSeconds(Track, u32),
    SaveSession(Session),
    Presence(Option<PresencePayload>),
    PresenceEnabled(bool),
    MediaControls(MediaState),
    Setting(Setting),
}

#[derive(Debug, Clone)]
pub enum PlayerInput {
    Play(Track, Option<Vec<Track>>),
    PlayIndex(usize),
    Toggle,
    Next,
    Prev,
    Stop,
    /// Absolute source seconds.
    Seek(f64),
    SetVolume(f64),
    SetSpeed(f64),
    ToggleShuffle,
    CycleRepeat,
    SetSleep(SleepRequest),
    AddToQueue(Track),
    EnqueueMany(Vec<Track>),
    PlayNext(Track),
    RemoveFromQueue(usize),
    ReorderQueue(Vec<Track>),
    ClearQueue,
    StartRadio(Track),
    Engine(Event),
    StreamResolved {
        gen: u64,
        result: Result<Stream, String>,
    },
    Related {
        purpose: RelatedPurpose,
        seed: Track,
        result: Result<Vec<Track>, String>,
    },
    Tick,
    SettingsChanged(Settings),
    SpotifyAvailable(bool),
    YoutubeAvailable(bool),
    Restore(Session),
    /// Spotify moved on from the track with URI `from` to `track` by itself.
    AdoptSpotify {
        from: String,
        track: Track,
    },
    /// Spotify's own up-next list for the track with URI `playing`.
    SpotifyUpNext {
        playing: Option<String>,
        tracks: Vec<Track>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pending {
    None,
    PauseRestore,
    SleepEnd,
}

#[derive(Clone, Copy, Debug)]
struct LoadOpts {
    seek_to: f64,
    autoplay: bool,
    fade_in: f64,
}

impl Default for LoadOpts {
    fn default() -> Self {
        LoadOpts {
            seek_to: 0.0,
            autoplay: true,
            fade_in: 0.0,
        }
    }
}

type PresenceKey = (Option<String>, bool, bool, bool);

pub struct PlayerController {
    pub queue: Queue,
    pub track: Option<Track>,
    pub playing: bool,
    pub loading: bool,
    pub error: String,
    pub time: Time,
    pub sleep: Sleep,
    pub active: DeckId,
    pub settings: Settings,
    spotify_available: bool,
    youtube_available: bool,

    gen: u64,
    opts: LoadOpts,
    loaded: bool,
    retried: bool,
    resume_at: f64,
    silence: Option<Silence>,
    /// The track an autoplay fetch is out for. A fetch for a track no longer playing doesn't
    /// hold back one for the track that is.
    fetching_for: Option<TrackId>,
    /// Tracks this player copied from Spotify's up-next list into the end of the queue; while
    /// nothing else follows the current track, they're kept in step with Spotify's list.
    spotify_tail: Vec<TrackId>,
    /// The Spotify track Spotify has nothing lined up after (a song played on its own, not
    /// from an album or playlist): Mosaic's autoplay fills the queue for it instead.
    spotify_quiet: Option<TrackId>,
    /// Tracks autoplay added to the end of the queue (shown as "Autoplay" in the queue).
    pub auto_tail: Vec<TrackId>,
    /// The queue ran out while autoplay was still fetching: play its first track on arrival.
    continue_on_related: bool,
    crossfading: f64,
    handed_over: bool,
    crossfade_for: Option<u64>,
    end_skipped: Option<u64>,
    pending: Pending,

    queue_dirty: bool,
    last_position_save: Option<Instant>,
    record_due: Option<Instant>,
    stat_tick: Option<Instant>,
    seek_presence_at: Option<Instant>,
    presence_key: Option<PresenceKey>,
    last_media: Option<Instant>,
}

impl PlayerController {
    pub fn new(settings: Settings) -> Self {
        PlayerController {
            queue: Queue::default(),
            track: None,
            playing: false,
            loading: false,
            error: String::new(),
            time: Time::default(),
            sleep: Sleep::Off,
            active: DeckId::A,
            settings,
            spotify_available: false,
            youtube_available: false,
            gen: 0,
            opts: LoadOpts::default(),
            loaded: false,
            retried: false,
            resume_at: 0.0,
            silence: None,
            fetching_for: None,
            spotify_tail: Vec::new(),
            spotify_quiet: None,
            auto_tail: Vec::new(),
            continue_on_related: false,
            crossfading: 0.0,
            handed_over: false,
            crossfade_for: None,
            end_skipped: None,
            pending: Pending::None,
            queue_dirty: false,
            last_position_save: None,
            record_due: None,
            stat_tick: None,
            seek_presence_at: None,
            presence_key: None,
            last_media: None,
        }
    }

    /// Effects that bring a fresh engine in line with the stored settings.
    pub fn sync_engine(&self) -> Vec<Effect> {
        let s = &self.settings;
        vec![
            Effect::Engine(Command::SetVolume(s.volume as f32)),
            Effect::Engine(Command::SetSpeed(s.speed as f32)),
            Effect::Engine(eq_command(s)),
            Effect::Engine(Command::SetLeveling(s.leveling)),
            Effect::Engine(Command::SetNormalize(s.normalize)),
            Effect::PresenceEnabled(s.presence),
        ]
    }

    pub fn handle(&mut self, input: PlayerInput, now: Instant) -> Vec<Effect> {
        let mut fx = Vec::new();
        let key_before = self.state_key();
        self.dispatch(input, now, &mut fx);
        self.after(now, key_before, &mut fx);
        fx
    }

    /// Whether the 1 Hz tick has anything to do (stats, sleep timer, debounced presence).
    pub fn wants_tick(&self) -> bool {
        self.playing
            || self.record_due.is_some()
            || self.seek_presence_at.is_some()
            || matches!(self.sleep, Sleep::Timer { .. } | Sleep::Fading)
    }

    /// Why this track can't be played here, if it can't.
    pub fn unavailable(&self, track: &Track) -> Option<&'static str> {
        match track.source {
            Source::YouTube if !self.youtube_available => Some(UNAVAILABLE_YOUTUBE),
            Source::Other(_) => Some(UNAVAILABLE_SOURCE),
            Source::Spotify if !self.spotify_available => Some(UNAVAILABLE_SPOTIFY),
            _ => None,
        }
    }

    fn state_key(&self) -> (Option<String>, bool, bool, bool) {
        (
            self.track.as_ref().map(|t| t.id.key()),
            self.playing,
            self.loading,
            self.settings.presence,
        )
    }

    fn dispatch(&mut self, input: PlayerInput, now: Instant, fx: &mut Vec<Effect>) {
        match input {
            PlayerInput::Play(track, list) => self.play(&track, list.as_deref(), now, fx),
            PlayerInput::PlayIndex(i) => self.play_index(i, LoadOpts::default(), now, fx),
            PlayerInput::Toggle => self.toggle(now, fx),
            PlayerInput::Next => self.step(1, false, false, now, fx),
            PlayerInput::Prev => self.step(-1, false, false, now, fx),
            PlayerInput::Stop => fx.push(Effect::Engine(Command::Pause { deck: self.active })),
            PlayerInput::Seek(secs) => self.seek(secs, fx),
            PlayerInput::SetVolume(v) => {
                self.settings.volume = v;
                fx.push(Effect::Setting(Setting::Volume(v)));
                fx.push(Effect::Engine(Command::SetVolume(v as f32)));
            }
            PlayerInput::SetSpeed(v) => {
                self.settings.speed = v;
                fx.push(Effect::Setting(Setting::Speed(v)));
                fx.push(Effect::Engine(Command::SetSpeed(v as f32)));
            }
            PlayerInput::ToggleShuffle => {
                self.settings.shuffle = !self.settings.shuffle;
                fx.push(Effect::Setting(Setting::Shuffle(self.settings.shuffle)));
            }
            PlayerInput::CycleRepeat => {
                self.settings.repeat = match self.settings.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                fx.push(Effect::Setting(Setting::Repeat(self.settings.repeat)));
            }
            PlayerInput::SetSleep(req) => self.set_sleep(req, now, fx),
            PlayerInput::AddToQueue(t) => self.add_to_queue(&t, now, fx),
            PlayerInput::EnqueueMany(ts) => self.enqueue_many(&ts, now, fx),
            PlayerInput::PlayNext(t) => self.play_next(&t, now, fx),
            PlayerInput::RemoveFromQueue(i) => self.remove_from_queue(i),
            PlayerInput::ReorderQueue(list) => {
                self.queue.reorder(list);
                self.queue_dirty = true;
            }
            PlayerInput::ClearQueue => {
                self.queue.clear();
                self.auto_tail.clear();
                self.spotify_tail.clear();
                self.queue_dirty = true;
            }
            PlayerInput::StartRadio(t) => self.start_radio(t, now, fx),
            PlayerInput::Engine(event) => self.on_event(event, now, fx),
            PlayerInput::StreamResolved { gen, result } => self.on_stream(gen, result, fx),
            PlayerInput::Related {
                purpose,
                seed,
                result,
            } => self.on_related(purpose, seed, result, now, fx),
            PlayerInput::Tick => self.tick(now, fx),
            PlayerInput::SettingsChanged(s) => self.on_settings(s, fx),
            PlayerInput::SpotifyAvailable(on) => self.spotify_available = on,
            PlayerInput::YoutubeAvailable(on) => self.youtube_available = on,
            PlayerInput::Restore(session) => self.restore(session),
            PlayerInput::AdoptSpotify { from, track } => self.adopt_spotify(&from, track, now, fx),
            PlayerInput::SpotifyUpNext { playing, tracks } => {
                self.spotify_up_next(playing.as_deref(), tracks, fx)
            }
        }
    }

    /// Bookkeeping that depends on the final state of one `handle` call.
    fn after(&mut self, now: Instant, before: PresenceKey, fx: &mut Vec<Effect>) {
        if self.queue_dirty {
            self.queue_dirty = false;
            fx.extend(self.session_queue_patch());
        }
        let key = self.state_key();
        if key != before {
            self.on_play_state_change(&before, &key, now);
            self.push_presence(fx);
            self.push_media(now, fx);
        }
        if let Some(at) = self.seek_presence_at.filter(|at| *at <= now) {
            let _ = at;
            self.seek_presence_at = None;
            self.push_presence(fx);
        }
    }

    fn on_play_state_change(&mut self, before: &PresenceKey, after: &PresenceKey, now: Instant) {
        if before.0 != after.0 {
            self.stat_tick = None;
        }
        if self.playing && self.track.is_some() {
            self.stat_tick.get_or_insert(now + STAT_TICK);
        } else {
            self.stat_tick = None;
        }
    }

    fn push_presence(&mut self, fx: &mut Vec<Effect>) {
        self.presence_key = Some(self.state_key());
        if !self.settings.presence {
            return;
        }
        let payload = match &self.track {
            Some(t) if self.playing && !self.loading => Some(PresencePayload {
                title: t.title.clone(),
                artist: t.artist.clone(),
                album: t.album.clone(),
                artwork: t.artwork.clone(),
                url: t.permalink.clone(),
                duration_ms: if self.time.duration > 0.0 {
                    self.time.duration * 1000.0
                } else {
                    t.duration as f64
                },
                position_ms: self.time.current * 1000.0,
            }),
            _ => None,
        };
        fx.push(Effect::Presence(payload));
    }

    fn push_media(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        self.last_media = Some(now);
        fx.push(Effect::MediaControls(MediaState {
            track: self.track.clone(),
            playing: self.playing,
            position: self.time.current,
        }));
    }

    fn session_queue_patch(&self) -> Option<Effect> {
        if self.queue.list.is_empty() {
            return None;
        }
        let index = self.queue.index.max(0) as usize;
        let from = index.saturating_sub(SESSION_BEFORE);
        let to = (from + SESSION_SIZE).min(self.queue.list.len());
        Some(Effect::SaveSession(Session {
            queue: Some(self.queue.list[from..to].to_vec()),
            index: Some((index - from) as i64),
            ..Default::default()
        }))
    }

    fn restore(&mut self, session: Session) {
        let Some(list) = session.queue.filter(|q| !q.is_empty()) else {
            return;
        };
        let index = (session.index.unwrap_or(0).max(0) as usize).min(list.len() - 1);
        let position = session.position.unwrap_or(0.0);
        let track = list[index].clone();
        self.time = Time {
            current: position,
            duration: track.duration as f64 / 1000.0,
            ..Time::default()
        };
        self.queue = Queue {
            list,
            index: index as isize,
        };
        self.track = Some(track);
        self.resume_at = position;
        self.loaded = false;
    }

    fn on_settings(&mut self, new: Settings, fx: &mut Vec<Effect>) {
        let old = std::mem::replace(&mut self.settings, new);
        let s = &self.settings;
        if old.volume != s.volume {
            fx.push(Effect::Engine(Command::SetVolume(s.volume as f32)));
        }
        if old.speed != s.speed {
            fx.push(Effect::Engine(Command::SetSpeed(s.speed as f32)));
        }
        if old.eq_on != s.eq_on || old.eq_gains != s.eq_gains {
            fx.push(Effect::Engine(eq_command(s)));
        }
        if old.normalize != s.normalize {
            fx.push(Effect::Engine(Command::SetNormalize(s.normalize)));
        }
        if old.leveling != s.leveling {
            fx.push(Effect::Engine(Command::SetLeveling(s.leveling)));
        }
        if old.presence != s.presence {
            fx.push(Effect::PresenceEnabled(s.presence));
        }
    }

    fn seek(&mut self, secs: f64, fx: &mut Vec<Effect>) {
        if self.track.is_none() {
            return;
        }
        self.time.current = secs;
        if self.loaded {
            fx.push(Effect::Engine(Command::Seek {
                deck: self.active,
                secs,
            }));
        } else {
            self.resume_at = secs;
        }
    }

    fn toggle(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        let Some(track) = self.track.clone() else {
            return;
        };
        if !self.loaded {
            let opts = LoadOpts {
                seek_to: self.resume_at,
                ..LoadOpts::default()
            };
            return self.load(&track, opts, false, now, fx);
        }
        let deck = self.active;
        let fade = self.settings.fade_pause;
        if self.playing {
            if fade {
                self.pending = Pending::PauseRestore;
                fx.push(Effect::Engine(Command::Fade {
                    deck,
                    to: 0.0,
                    ms: PAUSE_FADE_MS,
                    curve: Curve::Linear,
                    then: AfterFade::Pause,
                }));
            } else {
                fx.push(Effect::Engine(Command::Pause { deck }));
            }
            return;
        }
        self.pending = Pending::None;
        if fade {
            fx.push(Effect::Engine(set_gain(deck, 0.0)));
            fx.push(Effect::Engine(Command::Play { deck }));
            fx.push(Effect::Engine(Command::Fade {
                deck,
                to: 1.0,
                ms: RESUME_FADE_MS,
                curve: Curve::Linear,
                then: AfterFade::Nothing,
            }));
        } else {
            fx.push(Effect::Engine(set_gain(deck, 1.0)));
            fx.push(Effect::Engine(Command::Play { deck }));
        }
    }

    fn on_event(&mut self, event: Event, now: Instant, fx: &mut Vec<Effect>) {
        let (deck, gen) = match &event {
            Event::DeviceChanged => return,
            Event::Ready { deck, gen, .. }
            | Event::Playing { deck, gen }
            | Event::Paused { deck, gen }
            | Event::Buffering { deck, gen, .. }
            | Event::Position { deck, gen, .. }
            | Event::Seeked { deck, gen }
            | Event::FadeDone { deck, gen }
            | Event::Ended { deck, gen }
            | Event::Error { deck, gen, .. } => (*deck, *gen),
        };
        if deck != self.active || gen != self.gen {
            return;
        }
        match event {
            Event::Ready { duration, .. } => {
                if let Some(d) = duration.filter(|d| *d > 0.0) {
                    self.time.duration = d;
                }
                if !self.opts.autoplay {
                    self.loading = false;
                }
            }
            Event::Playing { .. } => {
                self.playing = true;
                self.loading = false;
            }
            Event::Paused { .. } => {
                self.playing = false;
                self.on_paused(fx);
            }
            Event::Buffering { on, .. } => self.loading = on,
            Event::Position { secs, .. } => self.on_position(secs, now, fx),
            Event::Seeked { .. } => {
                self.seek_presence_at = Some(now + SEEK_PRESENCE_DEBOUNCE);
            }
            Event::Ended { .. } => {
                self.playing = false;
                self.step(1, true, true, now, fx);
            }
            Event::Error { message, .. } => self.on_error(&message, now, fx),
            Event::FadeDone { .. } | Event::DeviceChanged => {}
        }
    }

    fn on_paused(&mut self, fx: &mut Vec<Effect>) {
        let pending = std::mem::replace(&mut self.pending, Pending::None);
        if pending == Pending::None {
            return;
        }
        fx.push(Effect::Engine(set_gain(self.active, 1.0)));
        if pending == Pending::SleepEnd {
            self.sleep = Sleep::Off;
            fx.push(Effect::Toast("Sleep timer: goodnight 🌙".into()));
        }
    }

    fn on_error(&mut self, message: &str, now: Instant, fx: &mut Vec<Effect>) {
        // The bar only says "Playback failed"; the log keeps the engine's reason.
        tracing::warn!("engine error: {message}");
        if message == "stream expired" && !self.retried {
            if let Some(track) = self.track.clone() {
                let opts = LoadOpts {
                    seek_to: self.time.current,
                    autoplay: self.opts.autoplay,
                    fade_in: 0.0,
                };
                return self.load(&track, opts, true, now, fx);
            }
        }
        self.error = if message.contains("ffmpeg")
            || message.contains("Spotify")
            || message.contains("YouTube")
        {
            message.to_string()
        } else {
            "Playback failed".into()
        };
        self.loading = false;
    }

    fn on_position(&mut self, secs: f64, now: Instant, fx: &mut Vec<Effect>) {
        let duration = if self.time.duration > 0.0 {
            self.time.duration
        } else {
            self.track
                .as_ref()
                .map_or(0.0, |t| t.duration as f64 / 1000.0)
        };
        // Audio moving forward means it isn't buffering, whatever the last buffering event said.
        if self.loading && self.playing && secs > self.time.current + 0.05 {
            self.loading = false;
        }
        let trim = self.settings.skip_silence.then_some(self.silence).flatten();
        self.time = Time {
            current: secs,
            duration,
            offset: trim.map_or(0.0, |s| s.start),
            tail: trim
                .and_then(|s| s.end)
                .map_or(0.0, |end| (duration - end).max(0.0)),
        };
        if self
            .last_position_save
            .is_none_or(|at| now.duration_since(at) >= POSITION_SAVE_EVERY)
        {
            self.last_position_save = Some(now);
            fx.push(Effect::SaveSession(Session {
                position: Some(secs),
                ..Default::default()
            }));
        }
        self.maybe_crossfade(secs, now, fx);
        self.maybe_skip_tail(secs, now, fx);
    }

    fn maybe_skip_tail(&mut self, secs: f64, now: Instant, fx: &mut Vec<Effect>) {
        let Some(end) = self.silence.and_then(|s| s.end) else {
            return;
        };
        if !self.settings.skip_silence || !self.playing || secs < end {
            return;
        }
        if self.end_skipped == Some(self.gen) {
            return;
        }
        self.end_skipped = Some(self.gen);
        self.step(1, true, false, now, fx);
    }

    fn tick(&mut self, now: Instant, fx: &mut Vec<Effect>) {
        if let Some(due) = self.record_due.filter(|due| *due <= now) {
            let _ = due;
            self.record_due = None;
            if let Some(t) = &self.track {
                fx.push(Effect::RecordPlay(t.clone()));
            }
        }
        if let (Some(at), Some(t)) = (self.stat_tick, &self.track) {
            if self.playing && at <= now {
                fx.push(Effect::AddSeconds(t.clone(), STAT_TICK.as_secs() as u32));
                self.stat_tick = Some(at + STAT_TICK);
            }
        }
        self.tick_sleep(now, fx);
        if self.playing
            && self
                .last_media
                .is_none_or(|at| now.duration_since(at) >= MEDIA_REFRESH_EVERY)
        {
            self.push_media(now, fx);
        }
    }

    /// Steps over unavailable tracks in `direction`; `None` at the end or if nothing is playable.
    /// The track that plays when this one ends, if one is lined up.
    pub fn up_next(&self) -> Option<&Track> {
        if self.settings.repeat == Repeat::One {
            return None;
        }
        self.next_playable(1).and_then(|i| self.queue.list.get(i))
    }

    fn next_playable(&self, direction: isize) -> Option<usize> {
        let len = self.queue.list.len();
        let mut from = self.queue.index;
        for _ in 0..=len {
            let next = step_index(
                len,
                from,
                direction,
                self.settings.shuffle,
                self.settings.repeat,
                |n| rand::random_range(0..n),
            )?;
            if self.unavailable(&self.queue.list[next]).is_none() {
                return Some(next);
            }
            from = next as isize;
        }
        None
    }

    fn step(
        &mut self,
        direction: isize,
        auto: bool,
        ended: bool,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        if self.queue.list.is_empty() {
            return;
        }
        if auto && self.take_end_of_track(fx) {
            if !ended {
                fx.push(Effect::Engine(Command::Pause { deck: self.active }));
            }
            return;
        }
        if auto && self.settings.repeat == Repeat::One {
            return self.repeat_current(ended, now, fx);
        }
        if direction < 0 && self.time.current > PREV_RESTARTS_AFTER {
            return self.seek(0.0, fx);
        }

        if direction > 0 && !auto && self.spotify_led() {
            // Spotify's own next track (its queue or autoplay), which the player then follows.
            fx.push(Effect::SpotifySkip);
            return;
        }
        if let Some(index) = self.next_playable(direction) {
            let fade_in = if auto { self.crossfading } else { 0.0 };
            self.crossfading = 0.0;
            if auto && !self.handed_over {
                self.swap_decks(fx);
            }
            self.handed_over = false;
            let opts = LoadOpts {
                fade_in,
                ..LoadOpts::default()
            };
            return self.play_index(index, opts, now, fx);
        }

        let seed = self.queue.current().cloned();
        let on_spotify = seed.as_ref().is_some_and(|t| t.source == Source::Spotify);
        if on_spotify && !auto && direction > 0 {
            // Nothing lined up here: Spotify's own next track, which the player then follows.
            fx.push(Effect::SpotifySkip);
            return;
        }
        let fetching = self.fetching_for_current();
        let can_continue = auto
            && self.settings.autoplay
            && !fetching
            && seed.as_ref().is_some_and(|t| self.autoplays(t));
        let can_continue = can_continue || (auto && fetching);
        if let (true, Some(seed)) = (can_continue, seed) {
            self.continue_on_related = true;
            if !fetching {
                self.fetching_for = Some(seed.id.clone());
                fx.push(Effect::FetchRelated {
                    seed,
                    purpose: RelatedPurpose::Autoplay,
                });
            }
        }
    }

    /// Up to `n` tracks played before `seed` in the queue, then `seed` itself.
    pub fn recent_seeds(&self, seed: &Track, n: usize) -> Vec<Track> {
        let upto = self
            .queue
            .list
            .iter()
            .position(|t| t.id == seed.id)
            .unwrap_or(0);
        let mut out: Vec<Track> = self.queue.list[upto.saturating_sub(n)..upto]
            .iter()
            .filter(|t| t.source == seed.source)
            .cloned()
            .collect();
        out.push(seed.clone());
        out
    }

    /// Whether an autoplay fetch is out for the track in the queue now.
    fn fetching_for_current(&self) -> bool {
        self.fetching_for.is_some()
            && self.fetching_for.as_ref() == self.queue.current().map(|t| &t.id)
    }

    /// Whether autoplay continues after `t`: the services Mosaic finds similar songs for, and
    /// a Spotify track Spotify itself has nothing lined up after.
    fn autoplays(&self, t: &Track) -> bool {
        match t.source {
            Source::SoundCloud | Source::YouTube | Source::Monochrome => true,
            Source::Spotify => self.spotify_quiet.as_ref() == Some(&t.id),
            _ => false,
        }
    }

    /// Asks for autoplay tracks as soon as the queue gets short, instead of when it runs out.
    pub(super) fn top_up_autoplay(&mut self, fx: &mut Vec<Effect>) {
        let Some(current) = self.queue.current() else {
            return;
        };
        let left = self.queue.list.len() as isize - self.queue.index - 1;
        if !self.settings.autoplay
            || self.fetching_for_current()
            || self.settings.repeat != Repeat::Off
            || left >= AUTOPLAY_LOW_WATER
            || !self.autoplays(current)
        {
            return;
        }
        self.fetching_for = Some(current.id.clone());
        fx.push(Effect::FetchRelated {
            seed: current.clone(),
            purpose: RelatedPurpose::Autoplay,
        });
    }

    /// Whether Spotify's player should be warmed up now: a non-Spotify track is near its end
    /// and a Spotify track comes next. Starting it takes about a second, so this hides that.
    pub fn spotify_due_soon(&self) -> bool {
        const WARM_UP_SECS: f64 = 8.0;
        let playing_other = self
            .track
            .as_ref()
            .is_some_and(|t| t.source != Source::Spotify);
        let left = self.time.duration - self.time.current;
        playing_other
            && self.playing
            && self.time.duration > 0.0
            && left < WARM_UP_SECS
            && self
                .next_playable(1)
                .and_then(|i| self.queue.list.get(i))
                .is_some_and(|t| t.source == Source::Spotify)
    }

    /// Tracks copied in from Spotify's up-next list.
    pub fn spotify_tail(&self) -> &[TrackId] {
        &self.spotify_tail
    }

    /// Whether Spotify decides what follows: a Spotify track is playing and nothing after it
    /// here is Mosaic's own (only what was copied from Spotify's up-next list).
    pub fn spotify_led(&self) -> bool {
        let Some(current) = self.queue.current() else {
            return false;
        };
        if current.source != Source::Spotify
            || self.settings.repeat != Repeat::Off
            || self.settings.shuffle
        {
            return false;
        }
        let after = self.queue.index as usize + 1;
        self.queue
            .list
            .get(after..)
            .is_some_and(|rest| rest.iter().all(|t| self.spotify_tail.contains(&t.id)))
    }

    /// Spotify carried on into `track` (autoplay, or a skip from another device). With nothing
    /// else lined up here, follow it so the player shows what's audible; otherwise Mosaic's own
    /// next track takes over as usual.
    fn adopt_spotify(&mut self, from: &str, track: Track, now: Instant, fx: &mut Vec<Effect>) {
        let current = self.track.as_ref().and_then(|t| t.uri.as_deref());
        if current != Some(from) || self.settings.repeat != Repeat::Off {
            return;
        }
        let led = self.spotify_led();
        if !led && self.next_playable(1).is_some() {
            return;
        }
        if led {
            // What was copied from Spotify's old up-next list may be stale now.
            let after = self.queue.index as usize + 1;
            if self
                .queue
                .list
                .get(after)
                .is_some_and(|t| t.uri == track.uri)
            {
                return;
            }
            self.queue.list.truncate(after);
            self.spotify_tail.clear();
        }
        let _ = self.queue.play_next(&track);
        self.queue_dirty = true;
        // The old track's end may already have been handled (and found nothing next).
        if !self.playing && !self.loading {
            if let Some(index) = self.next_playable(1) {
                self.play_index(index, LoadOpts::default(), now, fx);
            }
        }
    }

    /// Copies Spotify's up-next list in after the current track when nothing else follows it
    /// here (or only tracks copied from an earlier list), so the queue shows what's coming and
    /// skipping forward works. Only a list for the track playing here counts: right after
    /// Spotify moves on, its list is for the next track.
    fn spotify_up_next(&mut self, playing: Option<&str>, tracks: Vec<Track>, fx: &mut Vec<Effect>) {
        let Some(current) = self.queue.current() else {
            return;
        };
        if current.source != Source::Spotify
            || playing.is_none()
            || current.uri.as_deref() != playing
        {
            return;
        }
        if tracks.is_empty() {
            // Nothing lined up on Spotify's side: autoplay here, like the other services.
            if self.spotify_quiet.as_ref() != Some(&current.id) {
                self.spotify_quiet = Some(current.id.clone());
                self.top_up_autoplay(fx);
            }
            return;
        }
        let current_id = current.id.clone();
        let after = self.queue.index as usize + 1;
        let ours = self.queue.list[after..]
            .iter()
            .all(|t| self.spotify_tail.contains(&t.id));
        if !ours {
            return;
        }
        let mut fresh: Vec<Track> = Vec::new();
        for t in tracks {
            if t.id != current_id && !fresh.iter().any(|f| f.id == t.id) {
                fresh.push(t);
            }
        }
        let same = self.queue.list[after..].len() == fresh.len()
            && self.queue.list[after..]
                .iter()
                .zip(&fresh)
                .all(|(a, b)| a.id == b.id);
        if same {
            return;
        }
        self.spotify_tail = fresh.iter().map(|t| t.id.clone()).collect();
        self.queue.list.truncate(after);
        self.queue.list.extend(fresh);
        self.queue_dirty = true;
    }

    fn repeat_current(&mut self, ended: bool, now: Instant, fx: &mut Vec<Effect>) {
        let Some(track) = self.track.clone() else {
            return;
        };
        if ended {
            return self.load(&track, LoadOpts::default(), false, now, fx);
        }
        let start = self.start_position();
        fx.push(Effect::Engine(Command::Seek {
            deck: self.active,
            secs: start,
        }));
        fx.push(Effect::Engine(Command::Play { deck: self.active }));
    }

    fn swap_decks(&mut self, fx: &mut Vec<Effect>) {
        let outgoing = self.active;
        self.active = outgoing.other();
        fx.push(Effect::Engine(Command::Unload { deck: outgoing }));
    }

    fn start_position(&self) -> f64 {
        match self.silence {
            Some(s) if self.settings.skip_silence => s.start,
            _ => 0.0,
        }
    }

    fn on_related(
        &mut self,
        purpose: RelatedPurpose,
        seed: Track,
        result: Result<Vec<Track>, String>,
        now: Instant,
        fx: &mut Vec<Effect>,
    ) {
        match purpose {
            RelatedPurpose::Radio => self.finish_radio(seed, result, now, fx),
            RelatedPurpose::Autoplay => {
                // An answer for a fetch that has since been replaced changes nothing.
                if self.fetching_for.as_ref() != Some(&seed.id) {
                    return;
                }
                self.fetching_for = None;
                let go = std::mem::take(&mut self.continue_on_related);
                if self.queue.current().map(|t| &t.id) != Some(&seed.id) {
                    // Moved on meanwhile: the track playing now gets its own.
                    self.top_up_autoplay(fx);
                    return;
                }
                let related = match result {
                    Ok(r) => r,
                    Err(e) => {
                        if go {
                            fx.push(Effect::Toast(e));
                        }
                        return;
                    }
                };
                // Never a song twice in the queue, even from another service.
                let queued: std::collections::HashSet<String> = self
                    .queue
                    .list
                    .iter()
                    .map(mosaic_services::autoplay::song_key)
                    .collect();
                let similar: Vec<Track> = filter_along(related, Some(&seed))
                    .into_iter()
                    .filter(|t| !self.queue.list.iter().any(|x| x.id == t.id))
                    .filter(|t| !queued.contains(&mosaic_services::autoplay::song_key(t)))
                    .collect();
                let Some(first) = similar.first().cloned() else {
                    return;
                };
                // Added right away, so what's coming can be seen (and changed) in the queue.
                let at = self.queue.list.len();
                self.auto_tail.extend(similar.iter().map(|t| t.id.clone()));
                self.queue.list.extend(similar);
                self.queue_dirty = true;
                if go && !self.playing && !self.loading {
                    self.queue.index = at as isize;
                    self.load(&first, LoadOpts::default(), false, now, fx);
                }
            }
        }
    }
}

fn eq_command(s: &Settings) -> Command {
    Command::SetEq {
        enabled: s.eq_on,
        gains_db: s.eq_gains.map(|g| g as f32),
    }
}

fn set_gain(deck: DeckId, to: f32) -> Command {
    Command::Fade {
        deck,
        to,
        ms: 0,
        curve: Curve::Linear,
        then: AfterFade::Nothing,
    }
}
