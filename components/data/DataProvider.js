'use client';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';

const Ctx = createContext(null);
export const useData = () => useContext(Ctx);

const DEFAULTS = {
  likes: [],
  playlists: [],
  history: [],
  plays: {},
  days: {},
  sources: [],
  settings: {
    autoplay: true,
    crossfade: 0,
    fadePause: true,
    leveling: false,
    eqOn: false,
    eqGains: [0, 0, 0, 0, 0],
    eqPreset: 'Flat',
    speed: 1,
    volume: 0.8,
    shuffle: false,
    repeat: 'off',
    lyricsOpen: true,
    presence: true,
    barHeight: 100,
    sideWidth: 360,
  },
};

const dayKey = (date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
const uid = () => Math.random().toString(36).slice(2, 10);
const strip = (t) => {
  const { lrcPath, ...rest } = t;
  return t.source === 'local' ? { ...rest, lrcPath } : rest;
};

export default function DataProvider({ children }) {
  const [data, setData] = useState(null);
  const [local, setLocal] = useState({ tracks: [], scanning: false });
  const dataRef = useRef(null);
  const sessionRef = useRef(null);
  const timer = useRef(null);

  const flush = useCallback(() => {
    if (!dataRef.current) return;
    window.mosaic.saveStore({ ...dataRef.current, session: sessionRef.current });
  }, []);

  const schedule = useCallback(() => {
    clearTimeout(timer.current);
    timer.current = setTimeout(flush, 600);
  }, [flush]);

  const update = useCallback((fn) => {
    const next = fn(dataRef.current);
    dataRef.current = next;
    setData(next);
    schedule();
  }, [schedule]);

  const scan = useCallback(async (sources) => {
    if (!sources.length) return setLocal({ tracks: [], scanning: false });
    setLocal((l) => ({ ...l, scanning: true }));
    try {
      const tracks = await window.mosaic.scanLocal(sources);
      setLocal({ tracks, scanning: false });
    } catch {
      setLocal((l) => ({ ...l, scanning: false }));
    }
  }, []);

  useEffect(() => {
    window.mosaic.loadStore().then((s) => {
      const merged = { ...DEFAULTS, ...s, settings: { ...DEFAULTS.settings, ...(s.settings || {}) } };
      delete merged.session;
      sessionRef.current = s.session || null;
      dataRef.current = merged;
      setData(merged);
      scan(merged.sources);
    });
    const onUnload = () => flush();
    window.addEventListener('beforeunload', onUnload);
    return () => window.removeEventListener('beforeunload', onUnload);
  }, [scan, flush]);

  const api = useMemo(() => {
    if (!data) return null;
    const likedIds = new Set(data.likes.map((t) => t.id));

    return {
      data,
      settings: data.settings,
      local,
      session: sessionRef.current,

      setSetting: (key, value) => update((d) => ({ ...d, settings: { ...d.settings, [key]: value } })),
      saveSession: (partial) => {
        sessionRef.current = { ...(sessionRef.current || {}), ...partial };
        schedule();
      },

      isLiked: (id) => likedIds.has(id),
      toggleLike: (t) => {
        update((d) => ({
          ...d,
          likes: d.likes.some((x) => x.id === t.id) ? d.likes.filter((x) => x.id !== t.id) : [strip(t), ...d.likes],
        }));
        return !likedIds.has(t.id);
      },

      createPlaylist: (name, tracks = []) => {
        const id = uid();
        update((d) => ({ ...d, playlists: [...d.playlists, { id, name: name.trim() || 'Untitled playlist', tracks: tracks.map(strip) }] }));
        return id;
      },
      renamePlaylist: (id, name) => update((d) => ({ ...d, playlists: d.playlists.map((p) => (p.id === id ? { ...p, name: name.trim() || p.name } : p)) })),
      deletePlaylist: (id) => update((d) => ({ ...d, playlists: d.playlists.filter((p) => p.id !== id) })),
      addToPlaylist: (id, t) => {
        let added = false;
        update((d) => ({
          ...d,
          playlists: d.playlists.map((p) => {
            if (p.id !== id || p.tracks.some((x) => x.id === t.id)) return p;
            added = true;
            return { ...p, tracks: [...p.tracks, strip(t)] };
          }),
        }));
        return added;
      },
      removeFromPlaylist: (id, trackId) =>
        update((d) => ({ ...d, playlists: d.playlists.map((p) => (p.id === id ? { ...p, tracks: p.tracks.filter((x) => x.id !== trackId) } : p)) })),
      reorderPlaylist: (id, tracks) => update((d) => ({ ...d, playlists: d.playlists.map((p) => (p.id === id ? { ...p, tracks } : p)) })),

      recordPlay: (t) =>
        update((d) => {
          const prev = d.plays[t.id] || { count: 0, seconds: 0 };
          return {
            ...d,
            history: [{ track: strip(t), at: Date.now() }, ...d.history.filter((h, i) => !(i === 0 && h.track.id === t.id))].slice(0, 100),
            plays: { ...d.plays, [t.id]: { track: strip(t), count: prev.count + 1, seconds: prev.seconds } },
          };
        }),
      addSeconds: (t, secs) =>
        update((d) => {
          const prev = d.plays[t.id] || { track: strip(t), count: 0, seconds: 0 };
          const day = dayKey(new Date());
          return { ...d, plays: { ...d.plays, [t.id]: { ...prev, seconds: prev.seconds + secs } }, days: { ...d.days, [day]: (d.days?.[day] || 0) + secs } };
        }),
      clearHistory: () => update((d) => ({ ...d, history: [], plays: {}, days: {} })),

      addSources: (paths) => {
        const next = [...new Set([...dataRef.current.sources, ...paths])];
        update((d) => ({ ...d, sources: next }));
        scan(next);
      },
      removeSource: (p) => {
        const next = dataRef.current.sources.filter((s) => s !== p);
        update((d) => ({ ...d, sources: next }));
        scan(next);
      },
      rescan: () => scan(dataRef.current.sources),
    };
  }, [data, local, update, schedule, scan]);

  if (!api) return null;
  return <Ctx.Provider value={api}>{children}</Ctx.Provider>;
}
