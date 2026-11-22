use std::time::{Duration, Instant};

use mosaic_audio::{AfterFade, Command, DeckId, Event, SourceSpec};
use mosaic_core::model::{Repeat, Session, Settings, Track};
use mosaic_services::soundcloud::silence::Silence;

use super::*;

struct Harness {
    c: PlayerController,
    now: Instant,
}

fn track(id: u64, source: &str, secs: u64) -> Track {
    serde_json::from_value(serde_json::json!({
        "id": id, "source": source, "title": format!("song {id}"), "artist": "a",
        "duration": secs * 1000,
    }))
    .unwrap()
}

fn sc(id: u64) -> Track {
    track(id, "soundcloud", 200)
}

fn stream() -> Stream {
    Stream {
        source: SourceSpec::File("x.mp3".into()),
        silence: None,
    }
}

impl Harness {
    fn new() -> Self {
        Self::with(Settings::default())
    }

    fn with(settings: Settings) -> Self {
        Harness {
            c: PlayerController::new(settings),
            now: Instant::now(),
        }
    }

    fn send(&mut self, input: PlayerInput) -> Vec<Effect> {
        self.c.handle(input, self.now)
    }

    fn advance(&mut self, secs: u64) {
        self.now += Duration::from_secs(secs);
    }

    /// Plays `list`, answers the stream request and reports the deck as playing.
    fn start(&mut self, target: &Track, list: &[Track]) -> Vec<Effect> {
        let fx = self.send(PlayerInput::Play(target.clone(), Some(list.to_vec())));
        self.resolve(&fx, Ok(stream()))
    }

    fn resolve(&mut self, fx: &[Effect], result: Result<Stream, String>) -> Vec<Effect> {
        let gen = fx
            .iter()
            .find_map(|e| match e {
                Effect::ResolveStream { gen, .. } => Some(*gen),
                _ => None,
            })
            .expect("a stream request");
        let out = self.send(PlayerInput::StreamResolved { gen, result });
        let deck = self.c.active;
        self.send(PlayerInput::Engine(Event::Playing { deck, gen }));
        out
    }

    fn position(&mut self, secs: f64) -> Vec<Effect> {
        let (deck, gen) = (self.c.active, self.c.gen);
        self.send(PlayerInput::Engine(Event::Position { deck, gen, secs }))
    }

    fn ended(&mut self) -> Vec<Effect> {
        let (deck, gen) = (self.c.active, self.c.gen);
        self.send(PlayerInput::Engine(Event::Ended { deck, gen }))
    }

    fn ready(&mut self, duration: f64) {
        let (deck, gen) = (self.c.active, self.c.gen);
        self.send(PlayerInput::Engine(Event::Ready {
            deck,
            gen,
            duration: Some(duration),
        }));
    }
}

fn resolves(fx: &[Effect]) -> Vec<u64> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::ResolveStream { track, .. } => track.id.key().parse().ok(),
            _ => None,
        })
        .collect()
}

fn loads(fx: &[Effect]) -> Vec<&Command> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Engine(c @ Command::Load { .. }) => Some(c),
            _ => None,
        })
        .collect()
}

fn toasts(fx: &[Effect]) -> Vec<&str> {
    fx.iter()
        .filter_map(|e| match e {
            Effect::Toast(s) => Some(s.as_str()),
            _ => None,
        })
        .collect()
}

fn has_cmd(fx: &[Effect], f: impl Fn(&Command) -> bool) -> bool {
    fx.iter().any(|e| matches!(e, Effect::Engine(c) if f(c)))
}

fn list(ids: &[u64]) -> Vec<Track> {
    ids.iter().map(|i| sc(*i)).collect()
}

#[test]
fn play_resolves_then_loads_on_the_active_deck() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(sc(1), Some(list(&[1, 2]))));
    assert_eq!(resolves(&fx), [1]);
    assert!(h.c.loading);
    let fx = h.resolve(&fx, Ok(stream()));
    match loads(&fx)[..] {
        [Command::Load {
            deck,
            gen,
            start_at,
            autoplay,
            fade_in_ms,
            duration_hint,
            ..
        }] => {
            assert_eq!(*deck, DeckId::A);
            assert_eq!(*gen, 1);
            assert_eq!(*start_at, 0.0);
            assert!(*autoplay);
            assert_eq!(*fade_in_ms, 0);
            assert_eq!(*duration_hint, Some(200.0));
        }
        ref other => panic!("unexpected {other:?}"),
    }
    assert!(h.c.playing && !h.c.loading);
}

#[test]
fn stale_generations_are_ignored() {
    let mut h = Harness::new();
    let first = h.send(PlayerInput::Play(sc(1), Some(list(&[1, 2]))));
    let second = h.send(PlayerInput::PlayIndex(1));
    assert_eq!(resolves(&second), [2]);
    let stale = h.resolve_gen(&first);
    assert!(loads(&stale).is_empty());
    assert!(h.c.loading, "the newer load is still pending");

    let (deck, old) = (h.c.active, 1);
    h.send(PlayerInput::Engine(Event::Error {
        deck,
        gen: old,
        message: "boom".into(),
    }));
    assert!(h.c.error.is_empty());
    h.send(PlayerInput::Engine(Event::Position {
        deck,
        gen: old,
        secs: 99.0,
    }));
    assert_eq!(h.c.time.current, 0.0);
}

impl Harness {
    fn resolve_gen(&mut self, fx: &[Effect]) -> Vec<Effect> {
        let gen = fx
            .iter()
            .find_map(|e| match e {
                Effect::ResolveStream { gen, .. } => Some(*gen),
                _ => None,
            })
            .unwrap();
        self.send(PlayerInput::StreamResolved {
            gen,
            result: Ok(stream()),
        })
    }
}

#[test]
fn resolve_errors_show_the_service_message() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(sc(1), None));
    let gen = h.c.gen;
    h.send(PlayerInput::StreamResolved {
        gen,
        result: Err("Could not reach SoundCloud. Check your connection.".into()),
    });
    let _ = fx;
    assert_eq!(
        h.c.error,
        "Could not reach SoundCloud. Check your connection."
    );
    assert!(!h.c.loading);
}

#[test]
fn skip_silence_starts_at_the_silence_end_of_the_intro() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(sc(1), None));
    let silent = Stream {
        source: SourceSpec::File("x".into()),
        silence: Some(Silence {
            start: 2.5,
            end: Some(190.0),
        }),
    };
    let fx = h.resolve(&fx, Ok(silent));
    assert!(matches!(loads(&fx)[..], [Command::Load { start_at, .. }] if *start_at == 2.5));

    h.ready(200.0);
    h.position(10.0);
    assert_eq!(h.c.time.offset, 2.5);
    assert_eq!(h.c.time.tail, 10.0);
}

#[test]
fn skip_silence_off_loads_from_zero_and_hides_trim() {
    let mut h = Harness::new();
    h.c.settings.skip_silence = false;
    let fx = h.send(PlayerInput::Play(sc(1), None));
    let silent = Stream {
        source: SourceSpec::File("x".into()),
        silence: Some(Silence {
            start: 2.5,
            end: Some(190.0),
        }),
    };
    let fx = h.resolve(&fx, Ok(silent));
    assert!(matches!(loads(&fx)[..], [Command::Load { start_at, .. }] if *start_at == 0.0));
    h.position(195.0);
    assert_eq!((h.c.time.offset, h.c.time.tail), (0.0, 0.0));
}

#[test]
fn tail_skip_steps_once_per_load() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(sc(1), Some(list(&[1, 2, 3]))));
    let silent = Stream {
        source: SourceSpec::File("x".into()),
        silence: Some(Silence {
            start: 0.0,
            end: Some(190.0),
        }),
    };
    h.resolve(&fx, Ok(silent));
    h.ready(200.0);
    assert!(resolves(&h.position(189.0)).is_empty());
    let fx = h.position(190.5);
    assert_eq!(resolves(&fx), [2]);
    assert_eq!(h.c.active, DeckId::B, "auto steps load on the other deck");
    assert!(has_cmd(&fx, |c| matches!(
        c,
        Command::Unload { deck: DeckId::A }
    )));
}

#[test]
fn next_and_prev_walk_the_queue() {
    let mut h = Harness::new();
    h.start(&sc(2), &list(&[1, 2, 3]));
    assert_eq!(resolves(&h.send(PlayerInput::Next)), [3]);
    assert_eq!(h.c.active, DeckId::A, "manual steps stay on the deck");
    assert_eq!(resolves(&h.send(PlayerInput::Prev)), [2]);
    assert_eq!(resolves(&h.send(PlayerInput::Prev)), [1]);
}

#[test]
fn prev_restarts_after_three_seconds() {
    let mut h = Harness::new();
    h.start(&sc(2), &list(&[1, 2, 3]));
    h.ready(200.0);
    h.position(3.5);
    let fx = h.send(PlayerInput::Prev);
    assert!(resolves(&fx).is_empty());
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::Seek { secs, .. } if *secs == 0.0)
    ));
    h.position(2.0);
    assert_eq!(resolves(&h.send(PlayerInput::Prev)), [1]);
}

#[test]
fn repeat_modes_at_the_end_of_the_queue() {
    let mut h = Harness::new();
    h.start(&sc(2), &list(&[1, 2]));
    assert!(resolves(&h.ended()).is_empty(), "off: stops");

    h.c.settings.repeat = Repeat::All;
    h.start(&sc(2), &list(&[1, 2]));
    assert_eq!(resolves(&h.ended()), [1], "all: wraps");

    h.c.settings.repeat = Repeat::One;
    h.start(&sc(1), &list(&[1, 2]));
    h.ready(200.0);
    h.position(100.0);
    let fx = h.send(PlayerInput::Engine(Event::Ended {
        deck: h.c.active,
        gen: h.c.gen,
    }));
    assert_eq!(resolves(&fx), [1], "one: reloads the same track after EOF");
    assert_eq!(h.c.queue.index, 0);
}

#[test]
fn repeat_one_from_tail_skip_seeks_instead_of_reloading() {
    let mut h = Harness::new();
    h.c.settings.repeat = Repeat::One;
    let fx = h.send(PlayerInput::Play(sc(1), Some(list(&[1, 2]))));
    let silent = Stream {
        source: SourceSpec::File("x".into()),
        silence: Some(Silence {
            start: 1.0,
            end: Some(150.0),
        }),
    };
    h.resolve(&fx, Ok(silent));
    h.ready(200.0);
    let fx = h.position(150.5);
    assert!(resolves(&fx).is_empty());
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::Seek { secs, .. } if *secs == 1.0)
    ));
    assert!(has_cmd(&fx, |c| matches!(c, Command::Play { .. })));
}

#[test]
fn shuffle_never_repeats_the_current_index() {
    let mut h = Harness::new();
    h.c.settings.shuffle = true;
    h.start(&sc(1), &list(&[1, 2, 3, 4]));
    for _ in 0..200 {
        let before = h.c.queue.index;
        h.send(PlayerInput::Next);
        assert_ne!(h.c.queue.index, before);
    }
}

#[test]
fn steps_skip_unavailable_tracks_in_both_directions() {
    let mut h = Harness::new();
    let tracks = vec![
        sc(1),
        track(2, "youtube", 100),
        track(3, "spotify", 100),
        sc(4),
    ];
    h.start(&sc(1), &tracks);
    assert_eq!(resolves(&h.send(PlayerInput::Next)), [4]);
    assert_eq!(resolves(&h.send(PlayerInput::Prev)), [1]);
}

#[test]
fn everything_unavailable_stops() {
    let mut h = Harness::new();
    h.start(&sc(1), &[sc(1), track(2, "youtube", 100)]);
    assert!(resolves(&h.send(PlayerInput::Next)).is_empty());
    assert_eq!(h.c.queue.index, 0);
}

#[test]
fn spotify_tracks_play_when_available() {
    let mut h = Harness::new();
    h.send(PlayerInput::SpotifyAvailable(true));
    let list = vec![sc(1), track(2, "spotify", 100)];
    h.start(&sc(1), &list);
    assert_eq!(resolves(&h.send(PlayerInput::Next)), [2]);
}

#[test]
fn loading_an_unavailable_track_reports_why() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(track(2, "youtube", 100), None));
    assert!(resolves(&fx).is_empty());
    assert_eq!(h.c.error, "Not available in this version");
    h.send(PlayerInput::Play(track(3, "spotify", 100), None));
    assert_eq!(h.c.error, "Connect Spotify in Settings → Beta");
}

#[test]
fn autoplay_continues_with_similar_tracks_at_the_end() {
    let mut h = Harness::new();
    h.start(&sc(2), &list(&[1, 2]));
    let fx = h.ended();
    let seed = match &fx[..] {
        [Effect::FetchRelated { seed, purpose }, ..] => {
            assert_eq!(*purpose, RelatedPurpose::Autoplay);
            seed.clone()
        }
        other => panic!("expected a related fetch, got {other:?}"),
    };
    assert!(
        h.ended()
            .iter()
            .all(|e| !matches!(e, Effect::FetchRelated { .. })),
        "only one fetch at a time"
    );
    let fx = h.send(PlayerInput::Related {
        purpose: RelatedPurpose::Autoplay,
        seed,
        result: Ok(list(&[2, 10, 11])),
    });
    assert_eq!(resolves(&fx), [10]);
    assert_eq!(toasts(&fx), ["Autoplay: similar tracks added"]);
    assert_eq!(h.c.queue.list.len(), 4);
    assert_eq!(h.c.queue.index, 2);
}

#[test]
fn autoplay_respects_the_setting_and_source() {
    let mut h = Harness::new();
    h.c.settings.autoplay = false;
    h.start(&sc(1), &list(&[1]));
    assert!(h
        .ended()
        .iter()
        .all(|e| !matches!(e, Effect::FetchRelated { .. })));

    let mut h = Harness::new();
    h.start(&track(1, "local", 100), &[track(1, "local", 100)]);
    assert!(h
        .ended()
        .iter()
        .all(|e| !matches!(e, Effect::FetchRelated { .. })));
}

#[test]
fn autoplay_result_for_a_changed_track_is_dropped() {
    let mut h = Harness::new();
    h.start(&sc(2), &list(&[1, 2]));
    h.ended();
    h.send(PlayerInput::Play(sc(5), None));
    let fx = h.send(PlayerInput::Related {
        purpose: RelatedPurpose::Autoplay,
        seed: sc(2),
        result: Ok(list(&[10])),
    });
    assert!(resolves(&fx).is_empty());
    assert_eq!(h.c.queue.list.len(), 1);
}

fn crossfade_harness(seconds: f64) -> Harness {
    let mut h = Harness::new();
    h.c.settings.crossfade = seconds;
    h.c.settings.skip_silence = false;
    h.start(&sc(1), &list(&[1, 2, 3]));
    h.ready(200.0);
    h
}

#[test]
fn crossfade_triggers_inside_the_window_only() {
    let mut h = crossfade_harness(6.0);
    assert!(resolves(&h.position(150.0)).is_empty());
    assert!(resolves(&h.position(192.9)).is_empty(), "left 7.1 > 7");
    let fx = h.position(193.5);
    assert_eq!(resolves(&fx), [2]);
    assert_eq!(h.c.active, DeckId::B);
    let fade = fx.iter().find_map(|e| match e {
        Effect::Engine(Command::Fade {
            deck,
            to,
            ms,
            curve,
            then,
        }) => Some((*deck, *to, *ms, *curve, *then)),
        _ => None,
    });
    assert_eq!(
        fade,
        Some((
            DeckId::A,
            0.0,
            6400,
            mosaic_audio::Curve::Out,
            AfterFade::Unload
        ))
    );
}

#[test]
fn crossfade_fades_the_next_track_in() {
    let mut h = crossfade_harness(6.0);
    let fx = h.position(194.0);
    let fx = h.resolve(&fx, Ok(stream()));
    assert!(matches!(
        loads(&fx)[..],
        [Command::Load {
            deck: DeckId::B,
            fade_in_ms: 6000,
            ..
        }]
    ));
}

#[test]
fn crossfade_fires_once_per_load() {
    let mut h = crossfade_harness(6.0);
    h.position(194.0);
    let (deck, gen) = (h.c.active, h.c.gen);
    assert_eq!(gen, 2);
    // Stale position events from the outgoing deck can't retrigger it.
    h.send(PlayerInput::Engine(Event::Position {
        deck: DeckId::A,
        gen: 1,
        secs: 195.0,
    }));
    assert_eq!((h.c.active, h.c.gen), (deck, gen));
}

#[test]
fn crossfade_bails_when_it_should() {
    let mut h = crossfade_harness(6.0);
    assert!(
        resolves(&h.position(199.8)).is_empty(),
        "less than 0.4 s left"
    );

    let mut h = crossfade_harness(6.0);
    h.c.settings.repeat = Repeat::One;
    assert!(resolves(&h.position(194.0)).is_empty());

    let mut h = crossfade_harness(6.0);
    h.send(PlayerInput::SetSleep(SleepRequest::EndOfTrack));
    assert!(resolves(&h.position(194.0)).is_empty());

    let mut h = crossfade_harness(0.0);
    assert!(resolves(&h.position(199.0)).is_empty());

    let mut h = Harness::new();
    h.c.settings.crossfade = 6.0;
    h.start(&sc(2), &list(&[1, 2]));
    h.ready(200.0);
    assert!(resolves(&h.position(194.0)).is_empty(), "no next track");

    let mut h = Harness::new();
    h.c.settings.crossfade = 6.0;
    h.start(&track(1, "soundcloud", 10), &list(&[1, 2]));
    h.ready(10.0);
    assert!(
        resolves(&h.position(5.0)).is_empty(),
        "shorter than twice the fade"
    );
}

#[test]
fn crossfade_never_goes_into_a_spotify_track() {
    let mut h = Harness::new();
    h.c.settings.crossfade = 6.0;
    h.send(PlayerInput::SpotifyAvailable(true));
    h.start(&sc(1), &[sc(1), track(2, "spotify", 200)]);
    h.ready(200.0);
    assert!(resolves(&h.position(194.0)).is_empty());
}

#[test]
fn pause_fades_out_then_restores_the_gain() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let fx = h.send(PlayerInput::Toggle);
    assert!(has_cmd(&fx, |c| matches!(
        c,
        Command::Fade { to, ms: 250, then: AfterFade::Pause, .. } if *to == 0.0
    )));
    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::FadeDone { deck, gen }));
    let fx = h.send(PlayerInput::Engine(Event::Paused { deck, gen }));
    assert!(!h.c.playing);
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::Fade { to, ms: 0, .. } if *to == 1.0)
    ));
}

#[test]
fn resume_fades_in_over_300ms() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::Paused { deck, gen }));
    let fx = h.send(PlayerInput::Toggle);
    assert!(has_cmd(&fx, |c| matches!(c, Command::Play { .. })));
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::Fade { to, ms: 300, .. } if *to == 1.0)
    ));
}

#[test]
fn pause_without_fade_is_immediate() {
    let mut h = Harness::new();
    h.c.settings.fade_pause = false;
    h.start(&sc(1), &list(&[1]));
    let fx = h.send(PlayerInput::Toggle);
    assert!(has_cmd(&fx, |c| matches!(c, Command::Pause { .. })));
    assert!(!has_cmd(&fx, |c| matches!(c, Command::Fade { .. })));
}

#[test]
fn restored_session_loads_from_the_saved_position_on_toggle() {
    let mut h = Harness::new();
    h.send(PlayerInput::Restore(Session {
        queue: Some(list(&[1, 2, 3])),
        index: Some(1),
        position: Some(42.5),
        ..Default::default()
    }));
    assert_eq!(h.c.track.as_ref().map(|t| t.id.key()), Some("2".into()));
    assert_eq!(h.c.time.current, 42.5);
    assert!(!h.c.playing && !h.c.loading);

    let fx = h.send(PlayerInput::Toggle);
    assert_eq!(resolves(&fx), [2]);
    let fx = h.resolve(&fx, Ok(stream()));
    assert!(matches!(loads(&fx)[..], [Command::Load { start_at, .. }] if *start_at == 42.5));
}

#[test]
fn restore_clamps_the_index_and_ignores_empty_sessions() {
    let mut h = Harness::new();
    h.send(PlayerInput::Restore(Session::default()));
    assert!(h.c.track.is_none());
    h.send(PlayerInput::Restore(Session {
        queue: Some(list(&[1, 2])),
        index: Some(9),
        ..Default::default()
    }));
    assert_eq!(h.c.queue.index, 1);
}

#[test]
fn seeking_before_load_updates_the_resume_point() {
    let mut h = Harness::new();
    h.send(PlayerInput::Restore(Session {
        queue: Some(list(&[1])),
        position: Some(5.0),
        ..Default::default()
    }));
    let fx = h.send(PlayerInput::Seek(30.0));
    assert!(fx.iter().all(|e| !matches!(e, Effect::Engine(_))));
    let fx = h.send(PlayerInput::Toggle);
    let fx = h.resolve(&fx, Ok(stream()));
    assert!(matches!(loads(&fx)[..], [Command::Load { start_at, .. }] if *start_at == 30.0));
}

#[test]
fn sleep_timer_counts_down_and_fades_out() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let fx = h.send(PlayerInput::SetSleep(SleepRequest::Minutes(1)));
    assert_eq!(toasts(&fx), ["Sleep timer set for 1 min"]);
    h.advance(30);
    h.send(PlayerInput::Tick);
    assert_eq!(h.c.sleep.seconds_left(), 30);

    h.advance(30);
    let fx = h.send(PlayerInput::Tick);
    assert!(has_cmd(&fx, |c| matches!(
        c,
        Command::Fade {
            ms: 2500,
            then: AfterFade::Pause,
            ..
        }
    )));
    assert_eq!(h.c.sleep, Sleep::Fading);
    h.advance(1);
    let fx = h.send(PlayerInput::Tick);
    assert!(
        !has_cmd(&fx, |c| matches!(c, Command::Fade { .. })),
        "fires once"
    );

    let (deck, gen) = (h.c.active, h.c.gen);
    let fx = h.send(PlayerInput::Engine(Event::Paused { deck, gen }));
    assert_eq!(toasts(&fx), ["Sleep timer: goodnight 🌙"]);
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::Fade { to, ms: 0, .. } if *to == 1.0)
    ));
    assert_eq!(h.c.sleep, Sleep::Off);
}

#[test]
fn sleep_end_of_track_stops_instead_of_advancing() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1, 2]));
    let fx = h.send(PlayerInput::SetSleep(SleepRequest::EndOfTrack));
    assert_eq!(toasts(&fx), ["Will stop after this track"]);
    let fx = h.ended();
    assert_eq!(toasts(&fx), ["Sleep timer: stopped at end of track"]);
    assert!(resolves(&fx).is_empty());
    assert_eq!(h.c.sleep, Sleep::Off);
    assert!(!h.c.playing);
    h.send(PlayerInput::SetSleep(SleepRequest::Off));
}

#[test]
fn sleep_toasts() {
    let mut h = Harness::new();
    assert_eq!(
        toasts(&h.send(PlayerInput::SetSleep(SleepRequest::Off))),
        ["Sleep timer off"]
    );
}

#[test]
fn stats_record_after_twenty_seconds_and_tick_every_fifteen() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let stat = |fx: &[Effect]| {
        fx.iter()
            .any(|e| matches!(e, Effect::AddSeconds(..) | Effect::RecordPlay(_)))
    };
    h.advance(14);
    assert!(!stat(&h.send(PlayerInput::Tick)));
    h.advance(1);
    let fx = h.send(PlayerInput::Tick);
    assert!(fx.iter().any(|e| matches!(e, Effect::AddSeconds(_, 15))));
    assert!(fx.iter().all(|e| !matches!(e, Effect::RecordPlay(_))));
    h.advance(4);
    assert!(!stat(&h.send(PlayerInput::Tick)));
    h.advance(1);
    let fx = h.send(PlayerInput::Tick);
    assert_eq!(
        fx.iter()
            .filter(|e| matches!(e, Effect::RecordPlay(_)))
            .count(),
        1
    );
    h.advance(1);
    assert!(h
        .send(PlayerInput::Tick)
        .iter()
        .all(|e| !matches!(e, Effect::RecordPlay(_))));
}

#[test]
fn changing_track_cancels_the_pending_play_record() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1, 2]));
    h.advance(10);
    h.send(PlayerInput::Next);
    h.advance(11);
    let fx = h.send(PlayerInput::Tick);
    assert!(
        fx.iter().all(|e| !matches!(e, Effect::RecordPlay(_))),
        "restarted at the new load"
    );
}

#[test]
fn paused_playback_adds_no_listening_seconds() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::Paused { deck, gen }));
    h.advance(40);
    assert!(h
        .send(PlayerInput::Tick)
        .iter()
        .all(|e| !matches!(e, Effect::AddSeconds(..))));
}

#[test]
fn presence_follows_play_state_and_debounces_seeks() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::Play(sc(1), None));
    let presence = |fx: &[Effect]| {
        fx.iter()
            .filter_map(|e| match e {
                Effect::Presence(p) => Some(p.clone()),
                _ => None,
            })
            .next_back()
    };
    assert_eq!(presence(&fx), Some(None), "loading sends nothing");
    let fx = h.resolve(&fx, Ok(stream()));
    let _ = fx;
    h.ready(200.0);
    let (deck, gen) = (h.c.active, h.c.gen);
    let fx = h.send(PlayerInput::Engine(Event::Seeked { deck, gen }));
    assert!(presence(&fx).is_none());
    h.advance(1);
    let fx = h.position(50.0);
    assert_eq!(
        presence(&fx).flatten().map(|p| p.position_ms),
        Some(50_000.0)
    );
}

#[test]
fn presence_disabled_sends_nothing() {
    let mut h = Harness::new();
    h.c.settings.presence = false;
    let fx = h.send(PlayerInput::Play(sc(1), None));
    assert!(fx.iter().all(|e| !matches!(e, Effect::Presence(_))));
}

#[test]
fn queue_changes_save_a_session_window() {
    let mut h = Harness::new();
    let big: Vec<Track> = (0..400).map(sc).collect();
    let fx = h.send(PlayerInput::Play(sc(100), Some(big)));
    let session = fx.iter().find_map(|e| match e {
        Effect::SaveSession(s) if s.queue.is_some() => Some(s.clone()),
        _ => None,
    });
    let s = session.expect("session saved");
    assert_eq!(s.queue.as_ref().map(Vec::len), Some(300));
    assert_eq!(s.index, Some(20));
    assert_eq!(s.queue.unwrap()[0].id.key(), "80");
}

#[test]
fn position_saves_are_throttled_to_four_seconds() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    h.ready(200.0);
    let saved = |fx: &[Effect]| {
        fx.iter()
            .any(|e| matches!(e, Effect::SaveSession(s) if s.position.is_some()))
    };
    assert!(saved(&h.position(1.0)));
    h.advance(2);
    assert!(!saved(&h.position(3.0)));
    h.advance(2);
    assert!(saved(&h.position(5.0)));
}

#[test]
fn queue_ops_use_the_js_toasts() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::AddToQueue(sc(1)));
    assert_eq!(resolves(&fx), [1], "empty queue plays instead");
    assert_eq!(
        toasts(&h.send(PlayerInput::AddToQueue(sc(1)))),
        ["Already in your queue"]
    );
    assert_eq!(
        toasts(&h.send(PlayerInput::AddToQueue(sc(2)))),
        ["Added to queue"]
    );
    assert_eq!(
        toasts(&h.send(PlayerInput::EnqueueMany(list(&[2, 3, 4])))),
        ["Added 2 tracks to the queue"]
    );
    assert_eq!(
        toasts(&h.send(PlayerInput::PlayNext(sc(9)))),
        ["Playing next"]
    );
    assert_eq!(h.c.queue.list[1].id.key(), "9");
}

#[test]
fn radio_starts_with_related_tracks() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::StartRadio(sc(1)));
    assert_eq!(toasts(&fx), ["Starting radio from “song 1”"]);
    let seed = match &fx[..] {
        [_, Effect::FetchRelated {
            seed,
            purpose: RelatedPurpose::Radio,
        }] => seed.clone(),
        other => panic!("{other:?}"),
    };
    let fx = h.send(PlayerInput::Related {
        purpose: RelatedPurpose::Radio,
        seed: seed.clone(),
        result: Ok(list(&[1, 5, 6])),
    });
    assert_eq!(resolves(&fx), [1]);
    assert_eq!(h.c.queue.list.len(), 3);

    let fx = h.send(PlayerInput::Related {
        purpose: RelatedPurpose::Radio,
        seed: seed.clone(),
        result: Ok(vec![]),
    });
    assert_eq!(toasts(&fx), ["No similar tracks found"]);
    let fx = h.send(PlayerInput::Related {
        purpose: RelatedPurpose::Radio,
        seed,
        result: Err("x".into()),
    });
    assert_eq!(toasts(&fx), ["Could not start radio"]);
}

#[test]
fn radio_from_a_local_track_has_no_similar_tracks() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::StartRadio(track(1, "local", 100)));
    assert!(toasts(&fx).contains(&"No similar tracks found"));
    assert_eq!(resolves(&fx), [1]);
}

#[test]
fn expired_streams_are_re_resolved_once_at_the_current_position() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    h.ready(200.0);
    h.position(80.0);
    let (deck, gen) = (h.c.active, h.c.gen);
    let fx = h.send(PlayerInput::Engine(Event::Error {
        deck,
        gen,
        message: "stream expired".into(),
    }));
    assert_eq!(resolves(&fx), [1]);
    let fx = h.resolve_gen(&fx);
    assert!(matches!(loads(&fx)[..], [Command::Load { start_at, .. }] if *start_at == 80.0));

    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::Error {
        deck,
        gen,
        message: "stream expired".into(),
    }));
    assert_eq!(h.c.error, "Playback failed");
    assert!(!h.c.loading);
}

#[test]
fn other_engine_errors_show_playback_failed() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::Error {
        deck,
        gen,
        message: "decode".into(),
    }));
    assert_eq!(h.c.error, "Playback failed");
    h.send(PlayerInput::Engine(Event::Error {
        deck,
        gen,
        message: "This format needs ffmpeg installed".into(),
    }));
    assert_eq!(h.c.error, "This format needs ffmpeg installed");
}

#[test]
fn settings_intents_emit_engine_commands_and_setting_effects() {
    let mut h = Harness::new();
    let fx = h.send(PlayerInput::SetVolume(0.5));
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::SetVolume(v) if *v == 0.5)
    ));
    assert!(fx
        .iter()
        .any(|e| matches!(e, Effect::Setting(Setting::Volume(v)) if *v == 0.5)));
    h.send(PlayerInput::CycleRepeat);
    assert_eq!(h.c.settings.repeat, Repeat::All);
    h.send(PlayerInput::CycleRepeat);
    h.send(PlayerInput::CycleRepeat);
    assert_eq!(h.c.settings.repeat, Repeat::Off);
    h.send(PlayerInput::ToggleShuffle);
    assert!(h.c.settings.shuffle);

    let mut s = h.c.settings.clone();
    s.eq_on = true;
    s.leveling = true;
    s.speed = 1.5;
    let fx = h.send(PlayerInput::SettingsChanged(s));
    assert!(has_cmd(&fx, |c| matches!(
        c,
        Command::SetEq { enabled: true, .. }
    )));
    assert!(has_cmd(&fx, |c| matches!(c, Command::SetLeveling(true))));
    assert!(has_cmd(
        &fx,
        |c| matches!(c, Command::SetSpeed(v) if *v == 1.5)
    ));
}

#[test]
fn buffering_toggles_loading() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let (deck, gen) = (h.c.active, h.c.gen);
    h.send(PlayerInput::Engine(Event::Buffering {
        deck,
        gen,
        on: true,
    }));
    assert!(h.c.loading);
    h.send(PlayerInput::Engine(Event::Buffering {
        deck,
        gen,
        on: false,
    }));
    assert!(!h.c.loading);
}

#[test]
fn media_controls_refresh_every_five_seconds_while_playing() {
    let mut h = Harness::new();
    h.start(&sc(1), &list(&[1]));
    let media = |fx: &[Effect]| fx.iter().any(|e| matches!(e, Effect::MediaControls(_)));
    h.advance(2);
    assert!(!media(&h.send(PlayerInput::Tick)));
    h.advance(4);
    assert!(media(&h.send(PlayerInput::Tick)));
}
