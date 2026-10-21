'use client';
import { motion, AnimatePresence } from 'framer-motion';
import { MdPlayArrow, MdPause, MdSkipPrevious, MdSkipNext, MdShuffle, MdRepeat, MdRepeatOne } from 'react-icons/md';
import { usePlayer, useTime } from '@/components/player/PlayerProvider';
import WavySlider from '@/components/player/WavySlider';

const fmt = (s) => {
  s = Math.max(0, Math.floor(s || 0));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
};

function Spinner() {
  return (
    <svg className="spin" width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round">
      <path d="M12 3a9 9 0 1 0 9 9" />
    </svg>
  );
}

export default function Controls() {
  const { playing, loading, shuffle, repeat, toggle, next, prev, seek, toggleShuffle, cycleRepeat } = usePlayer();
  const time = useTime();
  const offset = time.offset || 0;
  const shownDuration = Math.max(0, time.duration - offset - (time.tail || 0));
  const shownCurrent = Math.min(Math.max(0, time.current - offset), shownDuration || Infinity);
  const RepeatIcon = repeat === 'one' ? MdRepeatOne : MdRepeat;

  return (
    <div className="center">
      <div className="transport">
        <button className={`icon ${shuffle ? 'on' : ''}`} title="Shuffle (S)" onClick={toggleShuffle}><MdShuffle size={22} /></button>
        <button className="icon skip" title="Previous" onClick={prev}><MdSkipPrevious size={30} /></button>
        <motion.button
          className="play"
          title={playing ? 'Pause' : 'Play'}
          onClick={toggle}
          whileTap={{ scale: 0.92 }}
          animate={{ borderRadius: playing ? 28 : 18 }}
          transition={{ type: 'spring', stiffness: 420, damping: 28 }}
        >
          <AnimatePresence mode="wait" initial={false}>
            <motion.span key={loading ? 'l' : playing ? 'p' : 'a'} initial={{ scale: 0.4, opacity: 0, rotate: -30 }} animate={{ scale: 1, opacity: 1, rotate: 0 }} exit={{ scale: 0.4, opacity: 0 }} transition={{ duration: 0.14 }} style={{ display: 'grid' }}>
              {loading ? <Spinner /> : playing ? <MdPause size={32} /> : <MdPlayArrow size={32} />}
            </motion.span>
          </AnimatePresence>
        </motion.button>
        <button className="icon skip" title="Next" onClick={next}><MdSkipNext size={30} /></button>
        <button className={`icon ${repeat !== 'off' ? 'on' : ''}`} title={`Repeat: ${repeat} (R)`} onClick={cycleRepeat}><RepeatIcon size={22} /></button>
      </div>

      <div className="seek">
        <span>{fmt(shownCurrent)}</span>
        <WavySlider label="Seek" value={shownCurrent} max={shownDuration || 1} playing={playing && !loading} onChange={(v) => seek(v + offset)} />
        <span>{fmt(shownDuration)}</span>
      </div>
    </div>
  );
}
