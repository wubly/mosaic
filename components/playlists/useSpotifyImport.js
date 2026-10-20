'use client';
import { useCallback, useEffect, useRef, useState } from 'react';

const CONCURRENCY = 4;

export default function useSpotifyImport() {
  const [step, setStep] = useState('input');
  const [error, setError] = useState('');
  const [canSignIn, setCanSignIn] = useState(false);
  const [data, setData] = useState(null);
  const [results, setResults] = useState([]);
  const run = useRef(0);
  const lastUrl = useRef('');

  useEffect(() => () => { run.current++; }, []);

  const match = useCallback((tracks) => {
    const token = ++run.current;
    setStep('matching');
    setResults(new Array(tracks.length).fill(undefined));
    let next = 0;
    const worker = async () => {
      while (next < tracks.length) {
        const i = next++;
        let found = null;
        try {
          found = await Promise.race([
            window.mosaic.matchTrack(tracks[i]),
            new Promise((resolve) => setTimeout(() => resolve(null), 30000)),
          ]);
        } catch {}
        if (token !== run.current) return;
        setResults((prev) => { const copy = [...prev]; copy[i] = found; return copy; });
      }
    };
    Promise.all(Array.from({ length: CONCURRENCY }, worker)).then(() => token === run.current && setStep('review'));
  }, []);

  const start = useCallback(async (url) => {
    lastUrl.current = url.trim();
    setError('');
    setCanSignIn(false);
    setStep('fetching');
    let r;
    try { r = await window.mosaic.spotifyPlaylist(lastUrl.current); } catch { r = { error: 'failed' }; }
    if (r.error === 'badurl') { setStep('input'); setError('That doesn’t look like a Spotify playlist or album link.'); return; }
    if (r.needsLogin) { setStep('login'); return; }
    if (r.error || !r.tracks?.length) {
      setStep('input');
      setError('Couldn’t read that playlist. If it’s private, sign in to Spotify and try again.');
      setCanSignIn(true);
      return;
    }
    setData(r);
    match(r.tracks);
  }, [match]);

  const signIn = useCallback(async () => {
    const ok = await window.mosaic.spotifyLogin();
    if (ok) start(lastUrl.current);
  }, [start]);

  const reset = useCallback(() => {
    run.current++;
    setStep('input');
    setError('');
    setCanSignIn(false);
    setData(null);
    setResults([]);
  }, []);

  const back = useCallback(() => setStep('input'), []);
  const done = results.filter((r) => r !== undefined).length;

  return { step, error, canSignIn, data, results, done, start, signIn, reset, back };
}
