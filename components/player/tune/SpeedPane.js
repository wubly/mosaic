'use client';
import { usePlayer } from '@/components/player/PlayerProvider';
import { SPEEDS } from '@/components/player/tune/presets';

export default function SpeedPane() {
  const { speed, setSpeed } = usePlayer();

  return (
    <div className="pane">
      <div className="big-num">{speed.toFixed(2).replace(/0$/, '')}×</div>
      <div className="chip-row">
        {SPEEDS.map((value) => (
          <button key={value} className={`chip ${speed === value ? 'on' : ''}`} onClick={() => setSpeed(value)}>{value}×</button>
        ))}
      </div>
      <p className="muted small">Pitch is preserved, so vocals keep their natural sound.</p>
    </div>
  );
}
