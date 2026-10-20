'use client';
import { useCallback, useEffect, useRef, useState } from 'react';

const FADE_STEPS = 25;

export default function useSleepTimer(audioRef, toast) {
  const [sleep, setSleepState] = useState({ mode: 'off' });
  const [sleepLeft, setSleepLeft] = useState(0);
  const sleepRef = useRef(sleep);

  const apply = useCallback((next) => {
    sleepRef.current = next;
    setSleepState(next);
  }, []);

  const setSleep = useCallback((mode, minutes = 0) => {
    apply(mode === 'timer' ? { mode, endsAt: Date.now() + minutes * 60000 } : { mode });
    setSleepLeft(mode === 'timer' ? minutes * 60 : 0);
    if (mode === 'timer') toast(`Sleep timer set for ${minutes} min`);
    else if (mode === 'eot') toast('Will stop after this track');
    else toast('Sleep timer off');
  }, [apply, toast]);

  const takeEndOfTrack = useCallback(() => {
    if (sleepRef.current.mode !== 'eot') return false;
    apply({ mode: 'off' });
    toast('Sleep timer: stopped at end of track');
    return true;
  }, [apply, toast]);

  useEffect(() => {
    if (sleep.mode !== 'timer') return;

    const tick = setInterval(() => {
      const left = Math.max(0, Math.round((sleep.endsAt - Date.now()) / 1000));
      setSleepLeft(left);
      if (left > 0) return;
      clearInterval(tick);

      const audio = audioRef.current;
      const original = audio.volume;
      let step = 0;
      const fade = setInterval(() => {
        step++;
        audio.volume = Math.max(0, original * (1 - step / FADE_STEPS));
        if (step < FADE_STEPS) return;
        clearInterval(fade);
        audio.pause();
        audio.volume = original;
        apply({ mode: 'off' });
        toast('Sleep timer: goodnight 🌙');
      }, 100);
    }, 1000);

    return () => clearInterval(tick);
  }, [sleep, audioRef, apply, toast]);

  return { sleep, sleepLeft, setSleep, takeEndOfTrack };
}
