'use client';
import { useCallback, useEffect, useRef } from 'react';
import { useData } from '@/components/data/DataProvider';

export const BAR = { min: 80, max: 200, def: 100 };
export const SIDE = { min: 280, max: 560, def: 360 };

const clamp = (value, lo, hi) => Math.min(hi, Math.max(lo, value));

export default function usePanelSizes() {
  const { settings, setSetting } = useData();
  const sizes = useRef({ bar: settings.barHeight ?? BAR.def, side: settings.sideWidth ?? SIDE.def });

  const apply = useCallback(() => {
    const root = document.documentElement.style;
    root.setProperty('--bar-h', `${sizes.current.bar}px`);
    root.setProperty('--side-w', `${sizes.current.side}px`);
  }, []);

  useEffect(() => {
    sizes.current = { bar: settings.barHeight ?? BAR.def, side: settings.sideWidth ?? SIDE.def };
    apply();
  }, [settings.barHeight, settings.sideWidth, apply]);

  const commit = useCallback(() => {
    setSetting('barHeight', sizes.current.bar);
    setSetting('sideWidth', sizes.current.side);
  }, [setSetting]);

  const dragBar = (e) => {
    sizes.current.bar = clamp(Math.round(window.innerHeight - 24 - e.clientY), BAR.min, BAR.max);
    apply();
  };

  const dragSide = (e) => {
    sizes.current.side = clamp(Math.round(window.innerWidth - 24 - e.clientX), SIDE.min, SIDE.max);
    apply();
  };

  const resetBar = () => { sizes.current.bar = BAR.def; apply(); commit(); };
  const resetSide = () => { sizes.current.side = SIDE.def; apply(); commit(); };

  return { dragBar, dragSide, resetBar, resetSide, commit };
}
