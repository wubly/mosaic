'use client';
import { AnimatePresence, motion } from 'framer-motion';
import { usePlayer } from '@/components/player/PlayerProvider';

export default function Ambient() {
  const { track } = usePlayer();
  const src = track?.thumb || track?.artwork;

  return (
    <div className="ambient" aria-hidden>
      <AnimatePresence>
        {src && (
          <motion.div
            key={src}
            className="ambient-fade"
            initial={{ opacity: 0 }}
            animate={{ opacity: 0.55 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 1.2 }}
          >
            <div className="ambient-art" style={{ backgroundImage: `url(${src})` }} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
