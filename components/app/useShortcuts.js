'use client';
import { useEffect } from 'react';

const isTyping = (target) => /INPUT|TEXTAREA|SELECT/.test(target.tagName) || target.isContentEditable;

export default function useShortcuts({ player, lib, toast, track, lyricsOpen, paletteOpen, actions }) {
  useEffect(() => {
    const currentTime = () => player.audioRef.current?.currentTime || 0;
    const volumeStep = (delta) => player.setVolume(Math.min(1, Math.max(0, +(player.volume + delta).toFixed(2))));
    const toggleLike = () => track && toast(lib.toggleLike(track) ? 'Added to Liked songs' : 'Removed from Liked songs');

    const onKey = (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        actions.togglePalette();
        return;
      }
      if (isTyping(e.target) || paletteOpen || e.ctrlKey || e.metaKey || e.altKey) return;

      const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
      const handlers = {
        ' ': () => player.toggle(),
        ArrowRight: () => (e.shiftKey ? player.next() : player.seek(currentTime() + 5)),
        ArrowLeft: () => (e.shiftKey ? player.prev() : player.seek(Math.max(0, currentTime() - 5))),
        ArrowUp: () => volumeStep(0.05),
        ArrowDown: () => volumeStep(-0.05),
        m: () => player.setVolume(player.volume === 0 ? 0.8 : 0),
        l: toggleLike,
        s: () => player.toggleShuffle(),
        r: () => player.cycleRepeat(),
        y: () => lib.setSetting('lyricsOpen', !lyricsOpen),
        i: () => actions.toggleMini(),
        f: () => track && actions.toggleNowPlaying(),
        '/': () => actions.search(),
      };

      const handler = handlers[key];
      if (!handler) return;
      if ([' ', 'ArrowRight', 'ArrowLeft', 'ArrowUp', 'ArrowDown', '/'].includes(key)) e.preventDefault();
      handler();
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
}
