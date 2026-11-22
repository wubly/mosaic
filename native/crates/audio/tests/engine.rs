mod common;

use common::*;
use mosaic_audio::{AfterFade, Command, Curve, DeckId, Event};

#[test]
fn resamples_44k_to_48k_keeping_pitch_and_length() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(dir.path(), "a.wav", 44_100, &[(1000.0, 2.0)], 0.5);
    let mut rig = Rig::new();
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));

    let out = mono(&rig.render_secs(3.0));
    let active: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, s)| s.abs() > 0.02)
        .map(|(i, _)| i)
        .collect();
    let secs = (active.last().unwrap() - active.first().unwrap()) as f32 / OUT_RATE as f32;
    assert!((secs - 2.0).abs() < 0.01, "length {secs}");

    let mid = &out[OUT_RATE as usize / 2..OUT_RATE as usize];
    let hz = peak_freq(mid, 950.0, 1050.0);
    assert!((hz - 1000.0).abs() <= 2.0, "pitch {hz}");
    assert!((rms(mid) - 0.5 / 2f32.sqrt()).abs() < 0.01);

    rig.drain();
    assert!(rig.seen.iter().any(|e| matches!(
        e,
        Event::Ended {
            deck: DeckId::A,
            ..
        }
    )));
}

#[test]
fn seek_lands_on_target_and_plays_new_content() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(
        dir.path(),
        "b.wav",
        48_000,
        &[(440.0, 3.0), (880.0, 3.0)],
        0.5,
    );
    let mut rig = Rig::new();
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("ready", |e| {
        matches!(
            e,
            Event::Ready {
                duration: Some(_),
                ..
            }
        )
    });
    rig.render_secs(0.5);

    rig.engine.send(Command::Seek {
        deck: DeckId::A,
        secs: 4.0,
    });
    rig.wait_for("seeked", |e| matches!(e, Event::Seeked { .. }));
    let Event::Position { secs, .. } = rig.wait_for(
        "position",
        |e| matches!(e, Event::Position { secs, .. } if (*secs - 4.0).abs() < 0.5),
    ) else {
        unreachable!()
    };
    assert!((secs - 4.0).abs() <= 0.02, "position {secs}");

    let out = mono(&rig.render_secs(1.0));
    let hz = peak_freq(&out[4800..], 400.0, 900.0);
    assert!((hz - 880.0).abs() <= 2.0, "pitch after seek {hz}");
    let last = rig
        .engine
        .events()
        .try_iter()
        .filter_map(|e| {
            if let Event::Position { secs, .. } = e {
                Some(secs)
            } else {
                None
            }
        })
        .last();
    assert!(last.is_none_or(|s| (s - 5.0).abs() < 0.3));
}

#[test]
fn equal_power_crossfade_keeps_level() {
    let dir = tempfile::tempdir().unwrap();
    let a = write_wav(dir.path(), "a.wav", 48_000, &[(440.0, 8.0)], 0.5);
    let b = write_wav(dir.path(), "b.wav", 48_000, &[(660.0, 8.0)], 0.5);
    let mut rig = Rig::new();
    rig.load(DeckId::A, &a, 0.0, true, 0);
    rig.wait_for("playing", |e| {
        matches!(
            e,
            Event::Playing {
                deck: DeckId::A,
                ..
            }
        )
    });
    let before = mono(&rig.render_secs(1.0));
    let single = rms(&before[24_000..]);

    rig.load(DeckId::B, &b, 0.0, true, 2000);
    rig.wait_for("b playing", |e| {
        matches!(
            e,
            Event::Playing {
                deck: DeckId::B,
                ..
            }
        )
    });
    rig.engine.send(Command::Fade {
        deck: DeckId::A,
        to: 0.0,
        ms: 2000,
        curve: Curve::Out,
        then: AfterFade::Unload,
    });
    rig.engine.sync();

    let fade = mono(&rig.render_secs(2.0));
    for (i, w) in fade.chunks(4800).enumerate() {
        let d = db(rms(w) / single);
        assert!(d.abs() < 1.5, "window {i}: {d:.2} dB");
    }
    let after = mono(&rig.render_secs(0.5));
    assert!(peak_freq(&after[4800..], 400.0, 700.0) >= 658.0);
    rig.drain();
    assert!(rig.seen.iter().any(|e| matches!(
        e,
        Event::FadeDone {
            deck: DeckId::A,
            ..
        }
    )));
}

#[test]
fn fade_out_then_pause_goes_silent() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(dir.path(), "c.wav", 48_000, &[(440.0, 4.0)], 0.5);
    let mut rig = Rig::new();
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    rig.render_secs(0.3);
    rig.engine.send(Command::Fade {
        deck: DeckId::A,
        to: 0.0,
        ms: 200,
        curve: Curve::Out,
        then: AfterFade::Pause,
    });
    rig.engine.sync();

    let out = mono(&rig.render_secs(0.6));
    assert!(out[..2400].iter().any(|s| s.abs() > 0.1));
    assert!(out[10_000..].iter().all(|s| s.abs() < 1e-6));
    rig.drain();
    assert!(rig.seen.iter().any(|e| matches!(e, Event::FadeDone { .. })));
    assert!(rig.seen.iter().any(|e| matches!(e, Event::Paused { .. })));
}

#[test]
fn volume_scales_output() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(dir.path(), "d.wav", 48_000, &[(440.0, 2.0)], 0.5);
    let mut rig = Rig::new();
    rig.engine.send(Command::SetVolume(0.5));
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    let out = mono(&rig.render_secs(1.0));
    let level = rms(&out[24_000..]);
    assert!((level - 0.5 / 2f32.sqrt() * 0.5).abs() < 0.01, "{level}");
}

#[test]
fn bad_file_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.mp3");
    std::fs::write(&path, b"not audio at all").unwrap();
    let mut rig = Rig::new();
    rig.load(DeckId::A, &path, 0.0, true, 0);
    let e = rig.wait_for("error", |e| matches!(e, Event::Error { .. }));
    assert!(matches!(e, Event::Error { message, .. } if !message.is_empty()));
}

#[test]
fn start_offset_begins_mid_track() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(
        dir.path(),
        "e.wav",
        48_000,
        &[(440.0, 2.0), (880.0, 2.0)],
        0.5,
    );
    let mut rig = Rig::new();
    rig.load(DeckId::A, &wav, 2.5, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    let out = mono(&rig.render_secs(0.5));
    assert!((peak_freq(&out[2400..], 400.0, 900.0) - 880.0).abs() <= 2.0);
}

#[test]
fn eq_boosts_band_and_leaves_others() {
    let dir = tempfile::tempdir().unwrap();
    let mid = write_wav(dir.path(), "m.wav", 48_000, &[(910.0, 3.0)], 0.1);
    let low = write_wav(dir.path(), "l.wav", 48_000, &[(60.0, 3.0)], 0.1);

    let level = |path: &std::path::Path, eq_on: bool| {
        let mut rig = Rig::new();
        rig.engine.send(Command::SetEq {
            enabled: eq_on,
            gains_db: [0.0, 0.0, 6.0, 0.0, 0.0],
        });
        rig.load(DeckId::A, path, 0.0, true, 0);
        rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
        rig.render_secs(0.5);
        rms(&mono(&rig.render_secs(1.0)))
    };
    let boost = db(level(&mid, true) / level(&mid, false));
    assert!((boost - 6.0).abs() < 0.5, "boost {boost}");
    assert!(db(level(&low, true) / level(&low, false)).abs() < 0.5);
}

#[test]
fn leveling_compresses_loud_audio() {
    let dir = tempfile::tempdir().unwrap();
    let loud = write_wav(dir.path(), "loud.wav", 48_000, &[(440.0, 4.0)], 0.5);
    let mut rig = Rig::new();
    rig.engine.send(Command::SetLeveling(true));
    rig.load(DeckId::A, &loud, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    rig.render_secs(2.0);
    let level = rms(&mono(&rig.render_secs(1.0)));
    assert!(db(level / (0.5 / 2f32.sqrt())) < -8.0);
}

#[test]
fn analyser_sees_the_output() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(dir.path(), "t.wav", 48_000, &[(1000.0, 3.0)], 0.02);
    let mut rig = Rig::new();
    let mut analyser = rig.engine.analyser();
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));
    let mut bins = [0u8; mosaic_audio::ANALYSER_BINS];
    for _ in 0..30 {
        rig.render_secs(0.02);
        analyser.byte_frequency_data(&mut bins);
    }
    let peak = bins.iter().enumerate().max_by_key(|(_, v)| **v).unwrap().0;
    assert!((10..=12).contains(&peak), "peak bin {peak}");
}

#[test]
fn speed_shortens_output_and_position_tracks_source_time() {
    let dir = tempfile::tempdir().unwrap();
    let wav = write_wav(dir.path(), "s.wav", 48_000, &[(440.0, 10.0)], 0.5);
    let mut rig = Rig::new();
    rig.engine.send(Command::SetSpeed(1.5));
    rig.load(DeckId::A, &wav, 0.0, true, 0);
    rig.wait_for("playing", |e| matches!(e, Event::Playing { .. }));

    let out = mono(&rig.render_secs(9.0));
    let active = out.iter().rposition(|s| s.abs() > 0.02).unwrap();
    let secs = active as f32 / OUT_RATE as f32;
    assert!((secs - 6.67).abs() < 0.3, "output length {secs}");
    let hz = peak_freq(&out[48_000..96_000], 400.0, 480.0);
    assert!((hz - 440.0).abs() <= 2.0, "pitch {hz}");

    rig.drain();
    let last = rig
        .seen
        .iter()
        .filter_map(|e| {
            if let Event::Position { secs, .. } = e {
                Some(*secs)
            } else {
                None
            }
        })
        .fold(0.0f64, f64::max);
    assert!((last - 10.0).abs() < 0.6, "position {last}");
    assert!(rig.seen.iter().any(|e| matches!(e, Event::Ended { .. })));
}
