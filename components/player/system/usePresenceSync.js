'use client';
import { useCallback, useEffect, useRef, useState } from 'react';

export default function usePresenceSync({ audioRef, track, playing, loading, enabled }) {
  const [seekTick, setSeekTick] = useState(0);
  const seekTimer = useRef(null);

  useEffect(() => { window.mosaic.setPresence(!!enabled); }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    if (!track || !playing || loading) {
      window.mosaic.updatePresence(null);
      return;
    }

    const audio = audioRef.current;
    window.mosaic.updatePresence({
      title: track.title,
      artist: track.artist,
      album: track.album,
      artwork: track.artwork,
      url: track.permalink,
      durationMs: audio?.duration ? audio.duration * 1000 : track.duration,
      positionMs: (audio?.currentTime || 0) * 1000,
    });
  }, [track?.id, playing, loading, enabled, seekTick]);

  return useCallback(() => {
    clearTimeout(seekTimer.current);
    seekTimer.current = setTimeout(() => setSeekTick((n) => n + 1), 800);
  }, []);
}
