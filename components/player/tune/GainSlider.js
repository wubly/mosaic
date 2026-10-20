'use client';
import { useRef } from 'react';

export default function GainSlider({ value, onChange, label }) {
  const ref = useRef(null);

  const setFromPointer = (e) => {
    const rect = ref.current.getBoundingClientRect();
    const fraction = 1 - Math.min(1, Math.max(0, (e.clientY - rect.top) / rect.height));
    onChange(Math.round((fraction * 24 - 12) * 2) / 2);
  };

  const onKeyDown = (e) => {
    if (e.key === 'ArrowUp') onChange(Math.min(12, value + 1));
    else if (e.key === 'ArrowDown') onChange(Math.max(-12, value - 1));
  };

  const percent = ((value + 12) / 24) * 100;

  return (
    <div
      ref={ref}
      className="gain"
      role="slider"
      aria-label={label}
      aria-valuemin={-12}
      aria-valuemax={12}
      aria-valuenow={value}
      tabIndex={0}
      onPointerDown={(e) => { e.currentTarget.setPointerCapture(e.pointerId); setFromPointer(e); }}
      onPointerMove={(e) => e.buttons && setFromPointer(e)}
      onKeyDown={onKeyDown}
      onDoubleClick={() => onChange(0)}
    >
      <span className="gain-track" />
      <span className="gain-fill" style={{ bottom: value >= 0 ? '50%' : `${percent}%`, height: `${Math.abs(percent - 50)}%` }} />
      <span className="gain-handle" style={{ bottom: `calc(${percent}% - 8px)` }} />
    </div>
  );
}
