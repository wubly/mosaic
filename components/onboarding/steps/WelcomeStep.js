'use client';
import { motion } from 'framer-motion';
import { MdArrowForward } from 'react-icons/md';
import Logo from '@/components/ui/Logo';
import { slide } from '@/components/onboarding/constants';

export default function WelcomeStep({ onNext }) {
  return (
    <motion.div className="ob-step" {...slide}>
      <Logo size={44} />
      <h1>Your music, pieced together.</h1>
      <p>SoundCloud and the files on your computer, in one player. Setup takes about a minute.</p>
      <div className="ob-actions">
        <button className="primary" onClick={onNext}>Get started <MdArrowForward size={18} /></button>
      </div>
    </motion.div>
  );
}
