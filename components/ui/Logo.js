'use client';
import { useId } from 'react';

const TONES = [
  ['#eaddff', '#d0bcff', '#b69df8'],
  ['#d0bcff', null, '#9a82db'],
  ['#b69df8', '#9a82db', '#7f67be'],
];

export default function Logo({ size = 20, text = true, className = '' }) {
  const id = useId().replace(/:/g, '');
  const pad = 84;
  const gap = 14;
  const cell = (512 - pad * 2 - gap * 2) / 3;

  return (
    <span className={`logo ${className}`}>
      <svg width={size} height={size} viewBox="0 0 512 512" aria-hidden="true">
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#2b2930" />
            <stop offset="1" stopColor="#141218" />
          </linearGradient>
        </defs>
        <rect width="512" height="512" rx="112" fill={`url(#${id})`} />
        {TONES.flatMap((row, r) => row.map((fill, q) => {
          const x = pad + q * (cell + gap);
          const y = pad + r * (cell + gap);
          if (fill) return <rect key={`${r}${q}`} x={x} y={y} width={cell} height={cell} rx="30" fill={fill} />;
          return (
            <g key={`${r}${q}`}>
              <rect x={x} y={y} width={cell} height={cell} rx="30" fill="#d0bcff" />
              <path d={`M${x + cell * 0.38} ${y + cell * 0.26}L${x + cell * 0.38} ${y + cell * 0.74}L${x + cell * 0.76} ${y + cell * 0.5}Z`} fill="#381e72" />
            </g>
          );
        }))}
      </svg>
      {text && <span>Mosaic</span>}
    </span>
  );
}
