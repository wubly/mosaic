'use client';
import { useCallback, useEffect, useRef, useState } from 'react';

const SDK_URL = 'https://sdk.scdn.co/spotify-player.js';
let sdkPromise = null;

function loadSdk() {
  if (window.Spotify) return Promise.resolve();
  sdkPromise ??= new Promise((resolve, reject) => {
    window.onSpotifyWebPlaybackSDKReady = resolve;
    const script = document.createElement('script');
    script.src = SDK_URL;
    script.onerror = () => { sdkPromise = null; reject(new Error('Could not load the Spotify player')); };
    document.body.appendChild(script);
  });
  return sdkPromise;
}

export default function useSpotifyPlayer({ enabled, volume, onState }) {
  const player = useRef(null);
  const device = useRef(null);
  const handler = useRef(onState);
  handler.current = onState;
  const volumeRef = useRef(volume);
  volumeRef.current = volume;
  const [ready, setReady] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let instance;

    loadSdk().then(() => {
      if (!live) return;
      instance = new window.Spotify.Player({
        name: 'Mosaic',
        volume: volumeRef.current,
        getOAuthToken: (cb) => window.mosaic.spToken().then(cb).catch(() => setError('Spotify sign-in expired')),
      });
      instance.addListener('ready', ({ device_id }) => { device.current = device_id; setReady(true); setError(''); });
      instance.addListener('not_ready', () => { device.current = null; setReady(false); });
      instance.addListener('player_state_changed', (state) => handler.current?.(state));
      for (const kind of ['initialization_error', 'authentication_error', 'account_error', 'playback_error']) {
        instance.addListener(kind, ({ message }) => setError(kind === 'initialization_error' ? `This build can’t play Spotify (${message})` : message));
      }
      instance.connect();
      player.current = instance;
    }).catch((e) => live && setError(e.message));

    return () => {
      live = false;
      instance?.disconnect();
      player.current = null;
      device.current = null;
      setReady(false);
    };
  }, [enabled]);

  const start = useCallback(async (track, positionMs = 0) => {
    if (!device.current) throw new Error(error || 'Spotify player isn’t ready yet');
    await player.current.activateElement?.();
    await window.mosaic.spPlay({ deviceId: device.current, uri: track.uri, positionMs });
  }, [error]);

  return {
    ready, error, start,
    pause: useCallback(() => player.current?.pause(), []),
    resume: useCallback(() => player.current?.resume(), []),
    seek: useCallback((ms) => player.current?.seek(Math.round(ms)), []),
    setVolume: useCallback((v) => player.current?.setVolume(v), []),
    getState: useCallback(() => player.current?.getCurrentState() ?? Promise.resolve(null), []),
  };
}
