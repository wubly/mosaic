'use client';
import { useCallback, useEffect, useRef, useState } from 'react';

const API_URL = 'https://www.youtube.com/iframe_api';
let apiPromise = null;

function loadApi() {
  if (window.YT?.Player) return Promise.resolve();
  apiPromise ??= new Promise((resolve, reject) => {
    window.onYouTubeIframeAPIReady = resolve;
    const script = document.createElement('script');
    script.src = API_URL;
    script.onerror = () => { apiPromise = null; reject(new Error('Could not load the YouTube player')); };
    document.body.appendChild(script);
  });
  return apiPromise;
}

const ERRORS = {
  2: 'YouTube rejected this video',
  5: 'YouTube couldn’t play this video here',
  100: 'This video was removed or is private',
  101: 'The owner doesn’t allow playing this video outside YouTube',
  150: 'The owner doesn’t allow playing this video outside YouTube',
  153: 'YouTube blocked the player (referrer missing)',
};

export default function useYoutubePlayer({ enabled, boxRef, volume, onState }) {
  const player = useRef(null);
  const handler = useRef(onState);
  handler.current = onState;
  const volumeRef = useRef(volume);
  volumeRef.current = volume;
  const pending = useRef(null);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!enabled || !boxRef.current) return;
    let live = true;
    let instance;
    let timer;
    const mount = document.createElement('div');
    boxRef.current.appendChild(mount);

    const report = (data) => {
      if (!instance?.getCurrentTime) return;
      const code = data ?? instance.getPlayerState();
      handler.current?.({
        position: instance.getCurrentTime() * 1000,
        duration: (instance.getDuration() || 0) * 1000,
        paused: code !== 1 && code !== 3,
        loading: code === 3 || code === -1,
        ended: code === 0,
      });
    };

    loadApi().then(() => {
      if (!live) return;
      instance = new window.YT.Player(mount, {
        width: '100%',
        height: '100%',
        playerVars: { origin: window.location.origin, playsinline: 1, cc_load_policy: 0, controls: 0, disablekb: 1, iv_load_policy: 3, rel: 0, modestbranding: 1 },
        events: {
          onReady: () => {
            player.current = instance;
            instance.setVolume(volumeRef.current * 100);
            setReady(true);
            setError('');
            if (pending.current) { const next = pending.current; pending.current = null; instance.loadVideoById({ videoId: next.videoId, startSeconds: next.at }); }
          },
          onStateChange: (e) => {
            if (e.data === 1) { try { instance.unloadModule('captions'); } catch {} }
            report(e.data);
          },
          onError: (e) => setError(ERRORS[e.data] || `YouTube error ${e.data}`),
        },
      });
      timer = setInterval(() => { if (player.current?.getPlayerState?.() === 1) report(1); }, 500);
    }).catch((e) => live && setError(e.message));

    return () => {
      live = false;
      clearInterval(timer);
      try { instance?.destroy(); } catch {}
      mount.remove();
      player.current = null;
      setReady(false);
    };
  }, [enabled]);

  const start = useCallback(async (track, positionMs = 0) => {
    setError('');
    const at = positionMs / 1000;
    if (!player.current) { pending.current = { videoId: track.videoId, at }; return; }
    player.current.loadVideoById({ videoId: track.videoId, startSeconds: at });
  }, []);

  return {
    enabled, ready, error, start,
    pause: useCallback(() => player.current?.pauseVideo(), []),
    resume: useCallback(() => player.current?.playVideo(), []),
    seek: useCallback((ms) => player.current?.seekTo(ms / 1000, true), []),
    setVolume: useCallback((v) => player.current?.setVolume(v * 100), []),
  };
}
