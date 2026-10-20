'use client';
import { useEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { MdTune, MdBedtime } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import EqualizerPane from '@/components/player/tune/EqualizerPane';
import SpeedPane from '@/components/player/tune/SpeedPane';
import SleepPane from '@/components/player/tune/SleepPane';

const TABS = [['eq', 'Equalizer'], ['speed', 'Speed'], ['sleep', 'Sleep']];
const PANES = { eq: EqualizerPane, speed: SpeedPane, sleep: SleepPane };

function useDismiss(open, close, ref) {
  useEffect(() => {
    if (!open) return;
    const onDown = (e) => { if (!ref.current?.contains(e.target)) close(); };
    const onKey = (e) => e.key === 'Escape' && close();
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open, close, ref]);
}

export default function TunePopover() {
  const { speed, sleep } = usePlayer();
  const { settings } = useData();
  const [open, setOpen] = useState(false);
  const [tab, setTab] = useState('eq');
  const wrap = useRef(null);

  useDismiss(open, () => setOpen(false), wrap);

  const timerActive = sleep.mode !== 'off';
  const Pane = PANES[tab];

  return (
    <div className="pop-wrap" ref={wrap}>
      <button
        className={`icon ${open || settings.eqOn || speed !== 1 || timerActive ? 'on' : ''}`}
        title="Equalizer, speed & sleep timer"
        onClick={() => setOpen((o) => !o)}
      >
        {timerActive ? <MdBedtime size={22} /> : <MdTune size={22} />}
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            className="popover"
            initial={{ opacity: 0, y: 10, scale: 0.96 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.97 }}
            transition={{ type: 'spring', stiffness: 420, damping: 32 }}
          >
            <div className="tabs">
              {TABS.map(([id, label]) => (
                <button key={id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
                  <span className="tab-label">
                    {label}
                    {tab === id && <motion.span layoutId="tune-tab" className="tab-ind" />}
                  </span>
                </button>
              ))}
            </div>
            <Pane />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
