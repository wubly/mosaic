'use client';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import { rampVolume, cancelFade } from '@/components/player/audio/fade';
import useAudioGraph from '@/components/player/audio/useAudioGraph';
import useSleepTimer from '@/components/player/audio/useSleepTimer';
import { resolveSource, attachStream, detachStream } from '@/components/player/audio/stream';
import { filterAlong } from '@/components/player/queue/relevance';
import { useSpotify } from '@/components/spotify/SpotifyProvider';
import useSpotifyPlayer from '@/components/player/spotify/useSpotifyPlayer';
import { useYoutube } from '@/components/youtube/YoutubeProvider';
import useYoutubePlayer from '@/components/player/youtube/useYoutubePlayer';
import useQueueState from '@/components/player/queue/useQueueState';
import useQueueOps from '@/components/player/queue/useQueueOps';
import usePresenceSync from '@/components/player/system/usePresenceSync';
import useSystemMedia from '@/components/player/system/useSystemMedia';
import useListeningStats from '@/components/player/system/useListeningStats';
import useSessionPersistence from '@/components/player/system/useSessionPersistence';

const PlayerCtx = createContext(null);
const TimeCtx = createContext({ current: 0, duration: 0, offset: 0, tail: 0 });

export const usePlayer = () => useContext(PlayerCtx);
export const useTime = () => useContext(TimeCtx);

const LEAD = 1;
const NEXT_REPEAT = { off: 'all', all: 'one', one: 'off' };

export default function PlayerProvider({ children }) {
  const { settings, setSetting } = useData();
  const toast = useToast();

  const deckA = useRef(null);
  const deckB = useRef(null);
  const activeDeck = useRef(0);
  const hlsRefs = useRef([{ current: null }, { current: null }]).current;
  const spotifyActive = useRef(false);
  const spRef = useRef(null);
  const ytRef = useRef(null);
  const ytBox = useRef(null);
  const slots = useRef({});
  const videoSlot = useCallback((name) => (el) => { if (el) slots.current[name] = el; else delete slots.current[name]; }, []);
  const engineKind = useRef(null);
  const engineRef = useMemo(() => ({
    get current() { return engineKind.current === 'youtube' ? ytRef.current : engineKind.current === 'spotify' ? spRef.current : null; },
  }), []);
  const spPos = useRef({ pos: 0, at: 0, paused: true, duration: 0, volume: 0.8 });
  const spEndedFor = useRef(null);
  const trackRef = useRef(null);

  const realAudio = () => [deckA.current, deckB.current][activeDeck.current];
  const spotifyShim = useMemo(() => ({
    get currentTime() { const s = spPos.current; return (s.pos + (s.paused ? 0 : Date.now() - s.at)) / 1000; },
    set currentTime(v) { spPos.current = { ...spPos.current, pos: v * 1000, at: Date.now() }; engineRef.current?.seek(v * 1000); },
    get duration() { return spPos.current.duration / 1000; },
    get paused() { return spPos.current.paused; },
    get volume() { return spPos.current.volume; },
    set volume(v) { spPos.current.volume = v; engineRef.current?.setVolume(v); },
    seeking: false,
    defaultPlaybackRate: 1,
    playbackRate: 1,
    preservesPitch: true,
    play() { engineRef.current?.resume(); return Promise.resolve(); },
    pause() { engineRef.current?.pause(); },
    getAttribute: () => 'spotify',
  }), []);
  const audioRef = useMemo(() => ({
    get current() { return spotifyActive.current ? spotifyShim : [deckA.current, deckB.current][activeDeck.current]; },
    get all() { return [deckA.current, deckB.current]; },
  }), [spotifyShim]);
  const hlsRef = useMemo(() => ({
    get current() { return hlsRefs[activeDeck.current].current; },
    set current(value) { hlsRefs[activeDeck.current].current = value; },
  }), [hlsRefs]);
  const loadSeq = useRef(0);
  const loadedRef = useRef(false);
  const resumeAt = useRef(0);
  const fetchingMore = useRef(false);
  const crossfading = useRef(0);
  const crossfadeFor = useRef(null);
  const endSkipped = useRef(null);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  const { queue, queueRef, setQueue } = useQueueState();
  const [track, setTrack] = useState(null);
  trackRef.current = track;
  const [playing, setPlaying] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [time, setTime] = useState({ current: 0, duration: 0 });

  const graph = useAudioGraph(audioRef, settings, track);
  const silenceRef = useRef(null);
  const speedRef = useRef(settings.speed);
  speedRef.current = settings.speed;
  const { sleep, sleepLeft, setSleep, takeEndOfTrack } = useSleepTimer(audioRef, toast);

  const loadExternal = async (target, { seekTo = 0, autoplay = true } = {}) => {
    const token = ++loadSeq.current;
    const engine = target.source === 'youtube' ? ytRef.current : spRef.current;
    if (engineKind.current && engineKind.current !== target.source) engineRef.current?.pause();
    engineKind.current = target.source;
    const idle = realAudio();
    cancelFade(idle);
    idle.pause();
    detachStream(hlsRef);
    idle.removeAttribute('src');
    idle.load();
    silenceRef.current = null;
    spotifyActive.current = true;
    spPos.current = { ...spPos.current, pos: seekTo * 1000, at: Date.now(), paused: true, duration: target.duration };

    setTrack(target);
    setError('');
    setTime({ current: seekTo, duration: target.duration / 1000, offset: 0, tail: 0 });
    if (!autoplay) { loadedRef.current = false; setLoading(false); return; }

    loadedRef.current = true;
    setLoading(true);
    try {
      if (engine.enabled === false) throw new Error('Connect this service in Settings → Beta first');
      await engine.start(target, seekTo * 1000);
      engine.setVolume(settingsRef.current.volume);
    } catch (e) {
      if (token !== loadSeq.current) return;
      setError(e.message || 'Could not play this track');
      setLoading(false);
    }
  };

  const load = useCallback(async (target, options = {}) => {
    if (target.source === 'spotify' || target.source === 'youtube') return loadExternal(target, options);
    if (spotifyActive.current) { spotifyActive.current = false; engineRef.current?.pause(); }

    const { seekTo = 0, autoplay = true, fadeIn = 0 } = options;
    const audio = realAudio();
    const token = ++loadSeq.current;
    loadedRef.current = true;

    graph.prepare(settingsRef.current.eqOn || settingsRef.current.leveling);
    setTrack(target);
    setError('');
    setLoading(true);
    setTime({ current: seekTo, duration: target.duration / 1000 });
    cancelFade(audio);
    audio.volume = settingsRef.current.volume;
    detachStream(hlsRef);
    silenceRef.current = null;
    endSkipped.current = null;

    const onReady = () => {
      const skipTo = settingsRef.current.skipSilence ? silenceRef.current?.start : 0;
      if (seekTo || skipTo) audio.currentTime = seekTo || skipTo;
      if (autoplay && fadeIn) {
        audio.volume = 0;
        audio.play().then(() => rampVolume(audio, 0, settingsRef.current.volume, fadeIn * 1000, undefined, 'in')).catch(() => {});
      } else if (autoplay) audio.play().catch(() => {});
      else setLoading(false);
    };
    const fail = (message) => {
      if (token !== loadSeq.current) return;
      setError(message);
      setLoading(false);
    };

    try {
      const { url, type, silence } = await resolveSource(target);
      if (token !== loadSeq.current) return;
      silenceRef.current = silence || null;
      attachStream({ audio, hlsRef, url, type, onReady, onFatal: () => fail('Playback failed') });
    } catch (e) {
      fail(e.message || 'Could not play this track');
    }
  }, [graph]);

  const playIndex = useCallback((index, options) => {
    const current = queueRef.current;
    if (!current.list[index]) return;
    setQueue({ ...current, index });
    load(current.list[index], options);
  }, [queueRef, setQueue, load]);

  const play = useCallback((target, list) => {
    const seen = new Set();
    const unique = list ? list.filter((t) => !seen.has(t.id) && seen.add(t.id)) : [target];
    if (!unique.some((t) => t.id === target.id)) unique.unshift(target);
    setQueue({ list: unique, index: Math.max(0, unique.findIndex((t) => t.id === target.id)) });
    load(target);
  }, [setQueue, load]);

  const queueOps = useQueueOps({ queueRef, setQueue, play, toast });

  const nextIndex = (queueState, direction, prefs) => {
    if (prefs.shuffle && direction > 0 && queueState.list.length > 1) {
      let pick;
      do { pick = Math.floor(Math.random() * queueState.list.length); } while (pick === queueState.index);
      return pick;
    }
    const index = queueState.index + direction;
    if (index >= queueState.list.length) return prefs.repeat === 'all' ? 0 : null;
    return Math.max(0, index);
  };

  const continueWithSimilar = useCallback(async (queueState) => {
    fetchingMore.current = true;
    try {
      const last = queueState.list[queueState.index];
      const similar = filterAlong(await window.mosaic.related(last.id, 40), last).filter((t) => !queueState.list.some((x) => x.id === t.id));
      if (!similar.length) return;
      const list = [...queueState.list, ...similar];
      setQueue({ list, index: queueState.list.length });
      load(list[queueState.list.length]);
      toast('Autoplay: similar tracks added');
    } catch {} finally {
      fetchingMore.current = false;
    }
  }, [setQueue, load, toast]);

  const step = useCallback(async (direction, auto = false) => {
    const queueState = queueRef.current;
    const audio = audioRef.current;
    const prefs = settingsRef.current;
    if (!queueState.list.length) return;

    if (auto && takeEndOfTrack()) return;
    if (auto && prefs.repeat === 'one') {
      audio.currentTime = 0;
      audio.play().catch(() => {});
      return;
    }
    if (direction < 0 && audio.currentTime > 3) {
      audio.currentTime = 0;
      return;
    }

    const index = nextIndex(queueState, direction, prefs);
    if (index !== null) {
      const seconds = auto ? crossfading.current : 0;
      crossfading.current = 0;
      return playIndex(index, { fadeIn: seconds });
    }

    const canContinue = auto && prefs.autoplay && queueState.list[queueState.index]?.source === 'soundcloud' && !fetchingMore.current;
    if (canContinue) await continueWithSimilar(queueState);
  }, [queueRef, takeEndOfTrack, playIndex, continueWithSimilar]);

  const toggle = useCallback(() => {
    const audio = audioRef.current;
    if (!audio || !track) return;
    if (!loadedRef.current) return load(track, { seekTo: resumeAt.current });
    const fade = settingsRef.current.fadePause;
    const volume = settingsRef.current.volume;
    if (audio.paused) {
      graph.resume();
      if (!fade) return void audio.play().catch(() => {});
      audio.volume = 0;
      audio.play().then(() => rampVolume(audio, 0, volume, 300)).catch(() => {});
    } else if (fade) {
      rampVolume(audio, audio.volume, 0, 250, () => { audio.pause(); audio.volume = settingsRef.current.volume; });
    } else {
      audio.pause();
    }
  }, [track, load, graph]);

  const stop = useCallback(() => audioRef.current?.pause(), []);
  const seek = useCallback((seconds) => { if (audioRef.current) audioRef.current.currentTime = seconds; }, []);
  const setVolume = useCallback((value) => setSetting('volume', value), [setSetting]);
  const setSpeed = useCallback((value) => setSetting('speed', value), [setSetting]);
  const toggleShuffle = useCallback(() => setSetting('shuffle', !settingsRef.current.shuffle), [setSetting]);
  const cycleRepeat = useCallback(() => setSetting('repeat', NEXT_REPEAT[settingsRef.current.repeat]), [setSetting]);

  useEffect(() => { if (audioRef.current) { cancelFade(audioRef.current); audioRef.current.volume = settings.volume; } }, [settings.volume]);

  useEffect(() => {
    const audio = audioRef.current;
    if (!audio) return;
    audio.defaultPlaybackRate = settings.speed;
    audio.playbackRate = settings.speed;
    audio.preservesPitch = true;
  }, [settings.speed, track]);

  const savePosition = useSessionPersistence({
    queue,
    restore: ({ list, index, position }) => {
      setQueue({ list, index });
      setTrack(list[index]);
      resumeAt.current = position;
      setTime({ current: position, duration: list[index].duration / 1000 });
    },
  });

  const { enabled: spotifyEnabled } = useSpotify();
  const { enabled: youtubeEnabled } = useYoutube();

  const onEngineState = (kind, st) => {
    if (!spotifyActive.current || engineKind.current !== kind) return;
    spPos.current = { ...spPos.current, pos: st.position, at: Date.now(), paused: st.paused, duration: st.duration };
    setPlaying(!st.paused);
    if (!st.loading) setLoading(false);
    setTime({ current: st.position / 1000, duration: st.duration / 1000, offset: 0, tail: 0 });

    const key = `${trackRef.current?.id}:${loadSeq.current}`;
    if (st.ended && spEndedFor.current !== key) {
      spEndedFor.current = key;
      step(1, true);
    }
  };

  const onSpotifyState = (state) => {
    if (!state || !spotifyActive.current || engineKind.current !== 'spotify') return;
    const before = spPos.current;
    const wasPlayingTo = before.pos + (before.paused ? 0 : Date.now() - before.at);
    const nearEnd = before.duration > 0 && wasPlayingTo >= before.duration - 4000;
    const now = trackRef.current;
    const ended = nearEnd && state.paused && state.position === 0 && !!now?.uri
      && state.track_window.previous_tracks.some((t) => t.uri === now.uri);
    onEngineState('spotify', { position: state.position, duration: state.duration, paused: state.paused, loading: state.loading, ended });
  };

  const sp = useSpotifyPlayer({ enabled: spotifyEnabled, volume: settings.volume, onState: onSpotifyState });
  spRef.current = sp;
  const yt = useYoutubePlayer({ enabled: youtubeEnabled, boxRef: ytBox, volume: settings.volume, onState: (st) => onEngineState('youtube', st) });
  ytRef.current = yt;

  useEffect(() => {
    const box = ytBox.current;
    if (!box || track?.source !== 'youtube' || !settings.ytVideo) return;

    let frame;
    let last = '';
    const tick = () => {
      frame = requestAnimationFrame(tick);
      const nowPlaying = slots.current.nowplaying;
      const el = nowPlaying || slots.current.lyrics;
      if (!el) { box.classList.remove('on'); last = ''; return; }

      const r = el.getBoundingClientRect();
      const z = nowPlaying ? 61 : 30;
      box.classList.add('on');
      const key = [r.left, r.top, r.width, r.height, z].join();
      if (key === last) return;
      last = key;

      const iw = Math.max(r.width, (r.height * 16) / 9);
      Object.assign(box.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px`, zIndex: z, borderRadius: getComputedStyle(el).borderRadius });
      box.style.setProperty('--iw', `${iw}px`);
      box.style.setProperty('--ih', `${(iw * 9) / 16}px`);
    };
    tick();

    return () => {
      cancelAnimationFrame(frame);
      box.classList.remove('on');
      box.removeAttribute('style');
    };
  }, [track?.source, settings.ytVideo]);

  useEffect(() => {
    const message = track?.source === 'youtube' ? yt.error : track?.source === 'spotify' ? sp.error : '';
    if (!message || !spotifyActive.current) return;
    setError(message);
    setLoading(false);
  }, [sp.error, yt.error]);

  useEffect(() => {
    if (track?.source !== 'spotify' && track?.source !== 'youtube') return;
    if (!playing) return;
    const timer = setInterval(() => {
      const current = spotifyShim.currentTime;
      setTime((t) => ({ ...t, current, offset: 0, tail: 0 }));
      savePosition(current);
    }, 500);
    return () => clearInterval(timer);
  }, [track?.source, playing]);

  useListeningStats(track, playing);
  useSystemMedia({ track, playing, commands: { toggle, next: () => step(1), prev: () => step(-1), stop, seek } });
  const noteSeek = usePresenceSync({ audioRef, track, playing, loading, enabled: settings.presence });

  const onTimeUpdate = (e) => {
    const audio = e.currentTarget;
    const duration = audio.duration || track?.duration / 1000 || 0;
    const trim = settingsRef.current.skipSilence ? silenceRef.current : null;
    setTime({ current: audio.currentTime, duration, offset: trim?.start || 0, tail: trim?.end ? Math.max(0, duration - trim.end) : 0 });
    savePosition(audio.currentTime);
    maybeCrossfade(audio);
    maybeSkipTail(audio);
  };

  const maybeSkipTail = (audio) => {
    const end = silenceRef.current?.end;
    if (!end || !settingsRef.current.skipSilence || audio.paused || audio.currentTime < end) return;
    const key = track.id + loadSeq.current;
    if (endSkipped.current === key) return;
    endSkipped.current = key;
    step(1, true);
  };

  const maybeCrossfade = (audio) => {
    const seconds = settingsRef.current.crossfade;
    const length = audio.duration;
    if (!seconds || !track || audio.paused || !length || !Number.isFinite(length)) return;
    const left = length - audio.currentTime;
    if (left > seconds + LEAD || left < 0.4 || length < seconds * 2) return;
    if (crossfadeFor.current === track.id + loadSeq.current) return;
    const state = queueRef.current;
    const prefs = settingsRef.current;
    if (prefs.repeat === 'one' || sleep.mode === 'eot') return;
    if (nextIndex(state, 1, prefs) === null) return;
    if (state.list[state.index + 1]?.source === 'spotify') return;
    crossfadeFor.current = track.id + loadSeq.current;
    crossfading.current = seconds;
    handOver(Math.max(0.5, left - 0.1));
    step(1, true);
  };

  const handOver = (ms) => {
    const outgoing = realAudio();
    const outgoingHls = hlsRefs[activeDeck.current];
    activeDeck.current ^= 1;
    const incoming = realAudio();
    incoming.defaultPlaybackRate = settingsRef.current.speed;
    incoming.playbackRate = settingsRef.current.speed;

    rampVolume(outgoing, outgoing.volume, 0, ms * 1000, () => {
      outgoing.pause();
      detachStream(outgoingHls);
      outgoing.removeAttribute('src');
      outgoing.load();
    }, 'out');
  };

  const onError = () => {
    if (!audioRef.current?.getAttribute('src')) return;
    setError('Playback failed');
    setLoading(false);
  };

  const mine = (i, fn) => (e) => { if (activeDeck.current === i) fn(e); };

  const value = {
    track, queue, playing, loading, error,
    volume: settings.volume, shuffle: settings.shuffle, repeat: settings.repeat, speed: settings.speed,
    sleep, sleepLeft, audioRef, videoSlot, getAnalyser: graph.getAnalyser,
    play, playIndex, toggle, stop, seek, setVolume, setSpeed, setSleep, toggleShuffle, cycleRepeat,
    next: () => step(1), prev: () => step(-1),
    ...queueOps,
  };

  return (
    <PlayerCtx.Provider value={value}>
      <TimeCtx.Provider value={time}>
        {[deckA, deckB].map((ref, i) => (
          <audio
            key={i}
            ref={ref}
            crossOrigin="anonymous"
            onPlay={mine(i, () => setPlaying(true))}
            onPause={mine(i, () => setPlaying(false))}
            onPlaying={mine(i, () => setLoading(false))}
            onWaiting={mine(i, () => setLoading(true))}
            onTimeUpdate={mine(i, onTimeUpdate)}
            onSeeked={mine(i, noteSeek)}
            onEnded={mine(i, () => step(1, true))}
            onError={mine(i, onError)}
          />
        ))}
        <div ref={ytBox} className="yt-box" />
        {children}
      </TimeCtx.Provider>
    </PlayerCtx.Provider>
  );
}
