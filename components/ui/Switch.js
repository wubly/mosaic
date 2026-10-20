'use client';
import { motion } from 'framer-motion';

export default function Switch({ on, onChange }) {
  return (
    <button type="button" role="switch" aria-checked={on} className={`switch ${on ? 'on' : ''}`} onClick={() => onChange(!on)}>
      <motion.span className="knob" layout transition={{ type: 'spring', stiffness: 500, damping: 32 }} />
    </button>
  );
}
