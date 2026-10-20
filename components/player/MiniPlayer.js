'use client';
import { motion } from 'framer-motion';
import { MdOpenInFull } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import Controls from '@/components/player/Controls';
import LyricsPanel from '@/components/lyrics/LyricsPanel';

export default function MiniPlayer({ onExpand }) {
  const { track } = usePlayer();

  return (
    <div className="mini">
      <button className="icon mini-top" title="Back to full player (I)" onClick={onExpand}><MdOpenInFull size={20} /></button>

      {track ? (
        <>
          <motion.div key={track.id} className="mini-art" initial={{ scale: 0.92, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} style={{ backgroundImage: track.artwork ? `url(${track.artwork})` : undefined }} />
          <div className="mini-meta">
            <h1>{track.title}</h1>
            <p>{track.artist}</p>
          </div>
          <LyricsPanel bare />
        </>
      ) : (
        <p className="muted" style={{ textAlign: 'center', marginTop: 80 }}>Nothing playing yet.</p>
      )}

      <div className="mini-bottom"><Controls /></div>
    </div>
  );
}
