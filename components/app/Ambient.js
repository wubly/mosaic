'use client';
import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { usePlayer } from '@/components/player/PlayerProvider';

export default function Ambient() {
  const { track } = usePlayer();
  const wanted = track?.thumb || track?.artwork;
  const [src, setSrc] = useState(null);

  useEffect(() => {
    if (!wanted) return setSrc(null);
    let live = true;
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.src = wanted;
    img.decode().then(() => {
      const c = document.createElement('canvas');
      c.width = c.height = 48;
      const ctx = c.getContext('2d');
      ctx.filter = 'blur(3px) saturate(1.6)';
      ctx.drawImage(img, -6, -6, 60, 60);
      if (live) setSrc(c.toDataURL('image/jpeg', 0.9));
    }).catch(() => live && setSrc(null));
    return () => { live = false; };
  }, [wanted]);

  return (
    <div className="ambient" aria-hidden>
      <div className="ambient-layers">
        <AnimatePresence>
          {src && (
            <motion.div
              key={src}
              className="ambient-fade"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 1, transition: { duration: 1.2 } }}
              transition={{ duration: 1.2 }}
            >
              <div className="ambient-art" style={{ backgroundImage: `url(${src})` }} />
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}
