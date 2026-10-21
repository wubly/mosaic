'use client';
import { useEffect, useRef } from 'react';

export default function useSystemMedia({ track, playing, commands }) {
  const latest = useRef(commands);
  latest.current = commands;

  useEffect(() => window.mosaic.onMediaKey((command) => latest.current[command]?.()), []);

  useEffect(() => {
    if (!('mediaSession' in navigator)) return;
    const handlers = { play: 'toggle', pause: 'toggle', nexttrack: 'next', previoustrack: 'prev', stop: 'stop' };
    for (const [action, command] of Object.entries(handlers)) {
      try { navigator.mediaSession.setActionHandler(action, () => latest.current[command]?.()); } catch {}
    }
    return () => {
      for (const action of Object.keys(handlers)) {
        try { navigator.mediaSession.setActionHandler(action, null); } catch {}
      }
    };
  }, []);

  useEffect(() => {
    document.title = track && playing ? `${track.title} — ${track.artist}` : 'Mosaic';
  }, [track, playing]);

  useEffect(() => {
    if (!('mediaSession' in navigator) || !track) return;
    navigator.mediaSession.metadata = new MediaMetadata({
      title: track.title,
      artist: track.artist,
      album: track.album || '',
      artwork: track.artwork ? [{ src: track.artwork, sizes: '500x500' }] : [],
    });
  }, [track]);

  useEffect(() => {
    if ('mediaSession' in navigator) navigator.mediaSession.playbackState = playing ? 'playing' : 'paused';
  }, [playing]);
}
