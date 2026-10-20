'use client';
import { useEffect } from 'react';
import { useData } from '@/components/data/DataProvider';

const COUNT_AFTER_MS = 20000;
const TICK_MS = 15000;

export default function useListeningStats(track, playing) {
  const { recordPlay, addSeconds } = useData();

  useEffect(() => {
    if (!track) return;
    const timer = setTimeout(() => recordPlay(track), COUNT_AFTER_MS);
    return () => clearTimeout(timer);
  }, [track?.id]);

  useEffect(() => {
    if (!track || !playing) return;
    const timer = setInterval(() => addSeconds(track, TICK_MS / 1000), TICK_MS);
    return () => clearInterval(timer);
  }, [track?.id, playing]);
}
