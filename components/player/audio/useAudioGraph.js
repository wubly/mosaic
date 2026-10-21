'use client';
import { useCallback, useEffect, useRef } from 'react';
import { createAudioGraph } from '@/components/player/audio/audioGraph';

export default function useAudioGraph(audioRef, settings, track) {
  const graph = useRef(null);
  const analyserWanted = useRef(false);

  const ensureGraph = useCallback(() => {
    if (!graph.current) graph.current = createAudioGraph(audioRef.all);
    return graph.current;
  }, [audioRef]);

  const resume = useCallback(() => graph.current?.ctx.resume().catch(() => {}), []);

  const prepare = useCallback((equalizerOn) => {
    if (equalizerOn || analyserWanted.current) ensureGraph().ctx.resume().catch(() => {});
  }, [ensureGraph]);

  const getAnalyser = useCallback(() => {
    analyserWanted.current = true;
    if (!graph.current && audioRef.current) ensureGraph().ctx.resume().catch(() => {});
    return graph.current?.analyser ?? null;
  }, [audioRef, ensureGraph]);

  useEffect(() => {
    const wanted = settings.eqOn || settings.leveling;
    const active = wanted ? ensureGraph() : graph.current;
    if (!active) return;
    if (wanted) active.ctx.resume().catch(() => {});
    active.filters.forEach((filter, i) => { filter.gain.value = settings.eqOn ? settings.eqGains[i] : 0; });
    active.leveler.threshold.value = settings.leveling ? -24 : 0;
    active.leveler.ratio.value = settings.leveling ? 4 : 1;
  }, [settings.eqOn, settings.eqGains, settings.leveling, track, ensureGraph]);

  return { prepare, resume, getAnalyser };
}
