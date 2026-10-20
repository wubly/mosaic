'use client';
import { useEffect, useId, useRef, useState } from 'react';

const WAVELENGTH = 28;
const AMPLITUDE = 3.5;
const HEIGHT = 28;
const MID = HEIGHT / 2;
const GAP = 7;

function wavePath(length) {
  const half = WAVELENGTH / 2;
  const count = Math.ceil(length / half) + 2;
  let d = `M0 0 Q${WAVELENGTH / 4} ${-2 * AMPLITUDE} ${half} 0`;
  for (let i = 2; i <= count; i++) d += ` T${half * i} 0`;
  return d;
}

export default function WavySlider({ value, max, onChange, wavy = true, playing = true, label, step = 5, className = '', style }) {
  const ref = useRef(null);
  const dragging = useRef(false);
  const [w, setW] = useState(0);
  const [held, setHeld] = useState(false);
  const clipId = useId().replace(/:/g, '');

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setW(el.clientWidth));
    ro.observe(el);
    setW(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const frac = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
  const x = frac * w;

  const fromPointer = (e) => {
    const r = ref.current.getBoundingClientRect();
    onChange(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * max);
  };

  const onKeyDown = (e) => {
    const inc = max <= 1 ? 0.05 : step;
    if (e.key === 'ArrowRight' || e.key === 'ArrowUp') onChange(Math.min(max, value + inc));
    else if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') onChange(Math.max(0, value - inc));
    else return;
    e.preventDefault();
  };

  const smooth = held ? 'none' : 'transform .25s linear, width .25s linear';
  const activeEnd = Math.max(0, x - GAP);
  const restStart = Math.min(w, x + GAP);

  return (
    <div
      ref={ref}
      className={`ws ${className}`}
      style={style}
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={Math.round(max)}
      aria-valuenow={Math.round(value)}
      onKeyDown={onKeyDown}
      onPointerDown={(e) => {
        dragging.current = true;
        setHeld(true);
        e.currentTarget.setPointerCapture(e.pointerId);
        fromPointer(e);
      }}
      onPointerMove={(e) => dragging.current && fromPointer(e)}
      onPointerUp={() => { dragging.current = false; setHeld(false); }}
      onPointerCancel={() => { dragging.current = false; setHeld(false); }}
    >
      <svg width={w} height={HEIGHT} viewBox={`0 0 ${w} ${HEIGHT}`}>
        <defs>
          <clipPath id={clipId}>
            <rect x="0" y="0" height={HEIGHT} style={{ width: activeEnd, transition: smooth }} />
          </clipPath>
        </defs>

        <line className="ws-rest" x1={restStart} x2={w} y1={MID} y2={MID} />

        <g clipPath={`url(#${clipId})`}>
          <g style={{ transform: `translateY(${MID}px) scaleY(${wavy && playing ? 1 : 0.02})`, transition: 'transform .45s cubic-bezier(.3,0,0,1)' }}>
            <path className="ws-active ws-wave" d={wavePath(w)} style={{ animationPlayState: playing ? 'running' : 'paused' }} />
          </g>
        </g>

        <rect className="ws-thumb" x="-2" y="3" width={held ? 3 : 4} height={HEIGHT - 6} rx="2" style={{ transform: `translateX(${x}px)`, transition: smooth }} />
      </svg>
    </div>
  );
}
