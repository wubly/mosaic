'use client';
import { useEffect } from 'react';
import { motion } from 'framer-motion';
import { MdKeyboardArrowDown } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import Controls from '@/components/player/Controls';
import LyricsPanel from '@/components/lyrics/LyricsPanel';
import Visualizer from '@/components/player/Visualizer';

export default function NowPlaying({ onClose }) {
  const { track } = usePlayer();

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  if (!track) return null;

  return (
    <motion.div className="nowplaying" initial={{ opacity: 0, y: 40 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 40 }} transition={{ type: 'spring', stiffness: 260, damping: 32 }}>
      <button className="icon np-close" title="Close (Esc)" onClick={onClose}><MdKeyboardArrowDown size={30} /></button>

      <div className="np-body">
        <div className="np-left">
          <motion.div key={track.id} className="np-art" initial={{ scale: 0.92, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} style={{ backgroundImage: track.artwork ? `url(${track.artwork})` : undefined }} />
          <h1>{track.title}</h1>
          <p>{track.artist}</p>
          <Visualizer height={64} />
        </div>
        <div className="np-right"><LyricsPanel large /></div>
      </div>

      <div className="np-controls"><Controls /></div>
    </motion.div>
  );
}
