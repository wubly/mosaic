'use client';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import { rampVolume, cancelFade } from '@/components/player/audio/fade';
import useAudioGraph from '@/components/player/audio/useAudioGraph';
import useSleepTimer from '@/components/player/audio/useSleepTimer';
import { resolveSource, attachStream, detachStream } from '@/components/player/audio/stream';
import { filterAlong } from '@/components/player/queue/relevance';
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
  const audioRef = useMemo(() => ({
    get current() { return [deckA.current, deckB.current][activeDeck.current]; },
    get all() { return [deckA.current, deckB.current]; },
  }), []);
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
  const [playing, setPlaying] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState('');
  const [time, setTime] = useState({ current: 0, duration: 0 });

  const graph = useAudioGraph(audioRef, settings, track);
  const silenceRef = useRef(null);
  const speedRef = useRef(settings.speed);
  speedRef.current = settings.speed;
  const { sleep, sleepLeft, setSleep, takeEndOfTrack } = useSleepTimer(audioRef, toast);

  const load = useCallback(async (target, { seekTo = 0, autoplay = true, fadeIn = 0 } = {}) => {
    const audio = audioRef.current;
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
    crossfadeFor.current = track.id + loadSeq.current;
    crossfading.current = seconds;
    handOver(Math.max(0.5, left - 0.1));
    step(1, true);
  };

  const handOver = (ms) => {
    const outgoing = audioRef.current;
    const outgoingHls = hlsRefs[activeDeck.current];
    activeDeck.current ^= 1;
    const incoming = audioRef.current;
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
    sleep, sleepLeft, audioRef, getAnalyser: graph.getAnalyser,
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
        {children}
      </TimeCtx.Provider>
    </PlayerCtx.Provider>
  );
}
