'use client';
import { useEffect } from 'react';
import { motion } from 'framer-motion';
import { MdKeyboardArrowDown } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { usePlayer } from '@/components/player/PlayerProvider';
import Controls from '@/components/player/Controls';
import LyricsPanel from '@/components/lyrics/LyricsPanel';
import Visualizer from '@/components/player/Visualizer';

export default function NowPlaying({ onClose }) {
  const { track, videoSlot } = usePlayer();
  const lib = useData();

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
          <motion.div ref={videoSlot('nowplaying')} key={track.id} className="np-art" data-video={track.source === 'youtube' && lib.settings.ytVideo ? '' : undefined} initial={{ scale: 0.92, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} style={{ backgroundImage: track.artwork ? `url(${track.artwork})` : undefined }} />
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
