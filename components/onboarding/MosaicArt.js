'use client';
import { motion } from 'framer-motion';

const TONES = [
  ['#eaddff', '#d0bcff', '#b69df8'],
  ['#d0bcff', null, '#9a82db'],
  ['#b69df8', '#9a82db', '#7f67be'],
];
const ORDER = [0, 1, 2, 5, 8, 7, 6, 3, 4];
const LIT = [3, 5, 7, 8, 9];

export default function MosaicArt({ step }) {
  const lit = LIT[step];
  const rank = (index) => ORDER.indexOf(index);

  return (
    <div className="ob-grid" aria-hidden>
      {TONES.flat().map((fill, index) => {
        const on = rank(index) < lit;
        return (
          <motion.div
            key={index}
            className="ob-tile"
            style={{ background: fill || '#d0bcff' }}
            initial={{ opacity: 0, scale: 0.6 }}
            animate={{ opacity: on ? 1 : 0.14, scale: on ? 1 : 0.92 }}
            transition={{ type: 'spring', stiffness: 220, damping: 20, delay: on ? rank(index) * 0.05 : 0 }}
          >
            {!fill && (
              <svg viewBox="0 0 24 24" width="38%" height="38%">
                <path d="M8 5.5v13l11-6.5z" fill="#381e72" />
              </svg>
            )}
          </motion.div>
        );
      })}
    </div>
  );
}
