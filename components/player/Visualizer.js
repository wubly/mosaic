'use client';
import { useEffect, useRef } from 'react';
import { usePlayer } from '@/components/player/PlayerProvider';

const BARS = 40;

export default function Visualizer({ height = 72 }) {
  const { getAnalyser, playing } = usePlayer();
  const canvas = useRef(null);
  const levels = useRef(new Float32Array(BARS));

  useEffect(() => {
    const el = canvas.current;
    const ctx = el.getContext('2d');
    let raf;
    let color = '#d0bcff';
    let lastColorRead = 0;
    const data = new Uint8Array(256);

    const draw = (now) => {
      if (document.hidden) { raf = requestAnimationFrame(draw); return; }
      const dpr = window.devicePixelRatio || 1;
      const w = el.clientWidth;
      const h = el.clientHeight;
      if (el.width !== w * dpr || el.height !== h * dpr) { el.width = w * dpr; el.height = h * dpr; }
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      if (now - lastColorRead > 800) {
        color = getComputedStyle(document.documentElement).getPropertyValue('--md-primary').trim() || color;
        lastColorRead = now;
      }

      const analyser = getAnalyser();
      if (analyser) analyser.getByteFrequencyData(data);
      const half = BARS / 2;
      const gap = 4;
      const bw = (w - gap * (BARS - 1)) / BARS;
      ctx.fillStyle = color;
      for (let i = 0; i < BARS; i++) {

        const bin = Math.floor((Math.abs(i - half + 0.5) / half) ** 1.4 * 110) + 1;
        const target = analyser && playing ? data[bin] / 255 : 0;
        levels.current[i] += (target - levels.current[i]) * (target > levels.current[i] ? 0.5 : 0.12);
        const bh = Math.max(4, levels.current[i] * h);
        const x = i * (bw + gap);
        ctx.globalAlpha = 0.45 + levels.current[i] * 0.55;
        ctx.beginPath();
        ctx.roundRect(x, (h - bh) / 2, bw, bh, bw / 2);
        ctx.fill();
      }

      if (!playing && Math.max(...levels.current) < 0.01) return;
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [getAnalyser, playing]);

  return <canvas ref={canvas} className="viz" style={{ height }} aria-hidden />;
}
