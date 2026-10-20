'use client';
import { usePlayer } from '@/components/player/PlayerProvider';
import { TIMERS } from '@/components/player/tune/presets';

const countdown = (seconds) => `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;

export default function SleepPane() {
  const { sleep, sleepLeft, setSleep } = usePlayer();
  const active = sleep.mode !== 'off';

  const label = sleep.mode === 'timer' ? countdown(sleepLeft) : sleep.mode === 'eot' ? 'End of track' : 'Off';

  return (
    <div className="pane">
      <div className="big-num">{label}</div>
      <div className="chip-row">
        {TIMERS.map((minutes) => <button key={minutes} className="chip" onClick={() => setSleep('timer', minutes)}>{minutes} min</button>)}
        <button className={`chip ${sleep.mode === 'eot' ? 'on' : ''}`} onClick={() => setSleep('eot')}>End of track</button>
        {active && <button className="chip" onClick={() => setSleep('off')}>Turn off</button>}
      </div>
      <p className="muted small">Music fades out gently, then pauses.</p>
    </div>
  );
}
