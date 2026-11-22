use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{unbounded, Receiver, Sender};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::deck::{CbEvent, DeckMsg, DeckPlayer, DeckShared, Marker, Rings};
use crate::decode::{self, DecCmd, DecoderCtx, Internal};
use crate::dsp::analyser::{analyser_pair, Analyser};
use crate::output::{self, DeviceStream, Mixer, MixerParams, OfflineRenderer};
use crate::{AfterFade, Command, Curve, DeckId, Engine, Event, Msg, SourceSpec};

const TICK: Duration = Duration::from_millis(10);
const REOPEN_DELAY: Duration = Duration::from_millis(500);

pub(crate) enum Mode {
    Device,
    Offline(u32),
}

struct Decoder {
    join: JoinHandle<Rings>,
    cmds: Sender<DecCmd>,
    stop: Arc<AtomicBool>,
}

struct Deck {
    id: DeckId,
    gen: u64,
    load_id: u64,
    epoch: u32,
    msgs: Producer<DeckMsg>,
    shared: Arc<DeckShared>,
    rings: Option<Rings>,
    decoder: Option<Decoder>,
    source: Option<SourceSpec>,
    duration_hint: Option<f64>,
    ready: bool,
    playing: bool,
    autoplay: Option<u32>,
    starved: bool,
}

struct Control {
    rate: u32,
    params: Arc<MixerParams>,
    decks: [Deck; 2],
    events: Sender<Event>,
    internal_tx: Sender<Internal>,
    internal_rx: Receiver<Internal>,
    cb_events: Consumer<CbEvent>,
    last_position_frame: u64,
    next_load_id: u64,
}

type Ready = (Option<OfflineRenderer>, Analyser);

pub(crate) fn spawn(mode: Mode) -> anyhow::Result<(Engine, Option<OfflineRenderer>)> {
    let (tx, rx) = unbounded::<Msg>();
    let (event_tx, event_rx) = unbounded::<Event>();
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<Result<Ready, String>>(1);

    let thread = std::thread::Builder::new()
        .name("mosaic-audio".into())
        .spawn(move || run(mode, rx, event_tx, ready_tx))?;

    match ready_rx.recv() {
        Ok(Ok((renderer, analyser))) => Ok((
            Engine {
                tx,
                events: event_rx,
                analyser,
                thread: Some(thread),
            },
            renderer,
        )),
        Ok(Err(message)) => {
            let _ = thread.join();
            anyhow::bail!(message)
        }
        Err(_) => anyhow::bail!("audio thread failed to start"),
    }
}

fn make_deck(id: DeckId, rate: u32) -> (Deck, DeckPlayer) {
    let (sample_tx, sample_rx) = RingBuffer::<f32>::new(rate as usize);
    let (marker_tx, marker_rx) = RingBuffer::<Marker>::new(512);
    let (msg_tx, msg_rx) = RingBuffer::<DeckMsg>::new(64);
    let shared = Arc::new(DeckShared::new());
    let player = DeckPlayer::new(id, sample_rx, marker_rx, msg_rx, Arc::clone(&shared));
    let deck = Deck {
        id,
        gen: 0,
        load_id: 0,
        epoch: 0,
        msgs: msg_tx,
        shared,
        rings: Some(Rings {
            samples: sample_tx,
            markers: marker_tx,
            pushed: 0,
        }),
        decoder: None,
        source: None,
        duration_hint: None,
        ready: false,
        playing: false,
        autoplay: None,
        starved: false,
    };
    (deck, player)
}

fn run(mode: Mode, rx: Receiver<Msg>, events: Sender<Event>, ready: Sender<Result<Ready, String>>) {
    let offline_rate = match mode {
        Mode::Offline(rate) => Some(rate),
        Mode::Device => None,
    };
    let rate = offline_rate.unwrap_or(48_000);

    let (deck_a, player_a) = make_deck(DeckId::A, rate);
    let (deck_b, player_b) = make_deck(DeckId::B, rate);
    let (cb_tx, cb_rx) = RingBuffer::<CbEvent>::new(256);
    let params = Arc::new(MixerParams::new());
    let (tap, analyser) = analyser_pair();
    let mixer = Arc::new(Mutex::new(Mixer::new(
        rate,
        [player_a, player_b],
        cb_tx,
        Arc::clone(&params),
        tap,
        offline_rate.is_some(),
    )));
    let failed = Arc::new(AtomicBool::new(false));

    let mut device: Option<DeviceStream> = None;
    let mut rate = rate;
    if offline_rate.is_none() {
        match output::open_device(&mixer, &failed) {
            Ok(d) => {
                rate = d.rate;
                device = Some(d);
            }
            Err(e) => {
                let _ = ready.send(Err(e.to_string()));
                return;
            }
        }
    }

    let (internal_tx, internal_rx) = unbounded();
    let mut control = Control {
        rate,
        params,
        decks: [deck_a, deck_b],
        events,
        internal_tx,
        internal_rx,
        cb_events: cb_rx,
        last_position_frame: 0,
        next_load_id: 1,
    };

    let renderer = offline_rate.map(|_| OfflineRenderer::new(Arc::clone(&mixer)));
    if ready.send(Ok((renderer, analyser))).is_err() {
        return;
    }

    let mut reopen_at: Option<Instant> = None;
    loop {
        match rx.recv_timeout(TICK) {
            Ok(Msg::Cmd(Command::Shutdown))
            | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Ok(Msg::Cmd(cmd)) => control.handle(cmd),
            Ok(Msg::Sync(done)) => {
                control.poll();
                control.poll();
                let _ = done.send(());
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        control.poll();

        if offline_rate.is_none() {
            if failed.swap(false, Ordering::Relaxed) && reopen_at.is_none() {
                device = None;
                reopen_at = Some(Instant::now() + REOPEN_DELAY);
            }
            if reopen_at.is_some_and(|t| Instant::now() >= t) {
                match output::open_device(&mixer, &failed) {
                    Ok(d) => {
                        let changed = d.rate != control.rate;
                        device = Some(d);
                        reopen_at = None;
                        control.device_reopened(
                            device.as_ref().map_or(control.rate, |d| d.rate),
                            changed,
                        );
                    }
                    Err(_) => reopen_at = Some(Instant::now() + REOPEN_DELAY),
                }
            }
        }
    }

    for i in 0..2 {
        control.stop_decoder(i);
    }
    drop(device);
}

impl Control {
    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }

    fn stop_decoder(&mut self, i: usize) {
        let deck = &mut self.decks[i];
        if let Some(dec) = deck.decoder.take() {
            dec.stop.store(true, Ordering::Relaxed);
            if let Ok(rings) = dec.join.join() {
                deck.rings = Some(rings);
            }
        }
    }

    fn start_decoder(&mut self, i: usize, source: SourceSpec, start_at: f64) {
        self.next_load_id += 1;
        let load_id = self.next_load_id;
        let deck = &mut self.decks[i];
        deck.load_id = load_id;
        let Some(rings) = deck.rings.take() else {
            return;
        };
        let (cmds_tx, cmds_rx) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let ctx = DecoderCtx {
            deck: deck.id,
            load_id,
            epoch: deck.epoch,
            source,
            start_at,
            duration_hint: deck.duration_hint,
            out_rate: self.rate,
            shared: Arc::clone(&deck.shared),
            params: Arc::clone(&self.params),
            events: self.internal_tx.clone(),
            cmds: cmds_rx,
            stop: Arc::clone(&stop),
        };
        let spawned = std::thread::Builder::new()
            .name(format!("mosaic-decode-{:?}", deck.id))
            .spawn(move || decode::run(ctx, rings));
        match spawned {
            Ok(join) => {
                deck.decoder = Some(Decoder {
                    join,
                    cmds: cmds_tx,
                    stop,
                })
            }
            Err(e) => {
                let (gen, id) = (deck.gen, deck.id);
                self.emit(Event::Error {
                    deck: id,
                    gen,
                    message: format!("Could not start decoder: {e}"),
                });
            }
        }
    }

    fn bump_epoch(&mut self, i: usize) -> u32 {
        let deck = &mut self.decks[i];
        deck.epoch += 1;
        deck.shared.flush_epoch.store(deck.epoch, Ordering::Release);
        deck.epoch
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Load {
                deck,
                gen,
                source,
                start_at,
                autoplay,
                fade_in_ms,
                duration_hint,
            } => {
                let i = deck.index();
                self.stop_decoder(i);
                self.bump_epoch(i);
                let d = &mut self.decks[i];
                d.gen = gen;
                d.ready = false;
                d.playing = false;
                d.autoplay = autoplay.then_some(fade_in_ms);
                d.duration_hint = duration_hint;
                d.source = Some(source.clone());
                d.starved = false;
                let _ = d.msgs.push(DeckMsg::Pause);
                let _ = d.msgs.push(DeckMsg::SetGain(if autoplay && fade_in_ms > 0 {
                    0.0
                } else {
                    1.0
                }));
                d.shared.set_position(start_at);
                self.start_decoder(i, source, start_at);
            }
            Command::Play { deck } => self.play(deck.index(), 0),
            Command::Pause { deck } => {
                let d = &mut self.decks[deck.index()];
                d.playing = false;
                d.autoplay = None;
                let _ = d.msgs.push(DeckMsg::Pause);
                let gen = d.gen;
                self.emit(Event::Paused { deck, gen });
            }
            Command::Unload { deck } => self.unload(deck.index()),
            Command::Seek { deck, secs } => {
                let i = deck.index();
                if self.decks[i].decoder.is_some() {
                    let epoch = self.bump_epoch(i);
                    let d = &self.decks[i];
                    d.shared.set_position(secs);
                    if let Some(dec) = &d.decoder {
                        let _ = dec.cmds.send(DecCmd::Seek { secs, epoch });
                    }
                }
            }
            Command::Fade {
                deck,
                to,
                ms,
                curve,
                then,
            } => self.fade(deck.index(), to, ms, curve, then),
            Command::SetVolume(v) => self.params.set_volume(v),
            Command::SetEq { enabled, gains_db } => {
                self.params
                    .set_eq(if enabled { gains_db } else { [0.0; 5] })
            }
            Command::SetLeveling(on) => self.params.leveling.store(on, Ordering::Relaxed),
            Command::SetSpeed(v) => self.params.set_speed(v),
            Command::Shutdown => {}
        }
    }

    fn frames_for(&self, ms: u32) -> u32 {
        (u64::from(ms) * u64::from(self.rate) / 1000) as u32
    }

    fn fade(&mut self, i: usize, to: f32, ms: u32, curve: Curve, then: AfterFade) {
        let frames = self.frames_for(ms);
        let d = &mut self.decks[i];
        let _ = d.msgs.push(DeckMsg::Fade {
            to,
            frames,
            curve,
            then,
        });
    }

    fn play(&mut self, i: usize, fade_in_ms: u32) {
        let d = &mut self.decks[i];
        d.playing = true;
        d.autoplay = None;
        let _ = d.msgs.push(DeckMsg::Play);
        let (id, gen) = (d.id, d.gen);
        if fade_in_ms > 0 {
            self.fade(i, 1.0, fade_in_ms, Curve::In, AfterFade::Nothing);
        }
        self.emit(Event::Playing { deck: id, gen });
    }

    fn unload(&mut self, i: usize) {
        self.stop_decoder(i);
        self.bump_epoch(i);
        let d = &mut self.decks[i];
        d.ready = false;
        d.playing = false;
        d.autoplay = None;
        d.source = None;
        d.starved = false;
        let _ = d.msgs.push(DeckMsg::Pause);
    }

    fn device_reopened(&mut self, rate: u32, changed: bool) {
        self.emit(Event::DeviceChanged);
        if !changed {
            return;
        }
        self.rate = rate;
        for i in 0..2 {
            let Some(source) = self.decks[i].source.clone() else {
                continue;
            };
            let at = self.decks[i].shared.position();
            self.stop_decoder(i);
            self.bump_epoch(i);
            self.decks[i].ready = false;
            self.start_decoder(i, source, at);
            self.decks[i].autoplay = self.decks[i].playing.then_some(0);
        }
    }

    fn poll(&mut self) {
        while let Ok(msg) = self.internal_rx.try_recv() {
            self.internal(msg);
        }
        while let Ok(event) = self.cb_events.pop() {
            self.callback_event(event);
        }

        let rendered = self.params.rendered.load(Ordering::Relaxed);
        if rendered.saturating_sub(self.last_position_frame) >= u64::from(self.rate / 10) {
            self.last_position_frame = rendered;
            for d in &self.decks {
                if d.ready && d.playing {
                    self.emit(Event::Position {
                        deck: d.id,
                        gen: d.gen,
                        secs: d.shared.position(),
                    });
                }
            }
        }

        for i in 0..2 {
            let starved =
                self.decks[i].playing && self.decks[i].shared.starved.load(Ordering::Relaxed);
            if starved != self.decks[i].starved {
                self.decks[i].starved = starved;
                let (deck, gen) = (self.decks[i].id, self.decks[i].gen);
                self.emit(Event::Buffering {
                    deck,
                    gen,
                    on: starved,
                });
            }
        }
    }

    fn internal(&mut self, msg: Internal) {
        match msg {
            Internal::Ready {
                deck,
                load_id,
                duration,
            } => {
                let i = deck.index();
                if self.decks[i].load_id != load_id {
                    return;
                }
                self.decks[i].ready = true;
                let gen = self.decks[i].gen;
                self.emit(Event::Ready {
                    deck,
                    gen,
                    duration,
                });
                if let Some(fade_in) = self.decks[i].autoplay.take() {
                    self.play(i, fade_in);
                }
            }
            Internal::Seeked {
                deck,
                load_id,
                epoch,
            } => {
                let d = &self.decks[deck.index()];
                if d.load_id != load_id || d.epoch != epoch {
                    return;
                }
                let (gen, secs) = (d.gen, d.shared.position());
                self.emit(Event::Seeked { deck, gen });
                self.emit(Event::Position { deck, gen, secs });
            }
            Internal::Buffering { deck, load_id, on } => {
                let d = &self.decks[deck.index()];
                if d.load_id != load_id {
                    return;
                }
                let gen = d.gen;
                self.emit(Event::Buffering { deck, gen, on });
            }
            Internal::Error {
                deck,
                load_id,
                message,
            } => {
                let d = &self.decks[deck.index()];
                if d.load_id != load_id {
                    return;
                }
                let gen = d.gen;
                self.emit(Event::Error { deck, gen, message });
            }
        }
    }

    fn callback_event(&mut self, event: CbEvent) {
        match event {
            CbEvent::FadeDone { deck, then, .. } => {
                let i = deck.index();
                let gen = self.decks[i].gen;
                match then {
                    AfterFade::Nothing => {}
                    AfterFade::Pause => {
                        self.decks[i].playing = false;
                    }
                    AfterFade::Unload => self.unload(i),
                }
                self.emit(Event::FadeDone { deck, gen });
                if then == AfterFade::Pause {
                    self.emit(Event::Paused { deck, gen });
                }
            }
            CbEvent::Ended { deck, epoch } => {
                let d = &mut self.decks[deck.index()];
                if d.epoch != epoch {
                    return;
                }
                d.playing = false;
                let gen = d.gen;
                self.emit(Event::Ended { deck, gen });
            }
        }
    }
}
