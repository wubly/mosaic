'use client';
import { motion } from 'framer-motion';
import { MdArrowForward, MdArrowBack } from 'react-icons/md';
import { slide } from '@/components/onboarding/constants';

export default function NameStep({ name, setName, onBack, onNext }) {
  return (
    <motion.form className="ob-step" onSubmit={(e) => { e.preventDefault(); onNext(); }} {...slide}>
      <h1>What should I call you?</h1>
      <p>It's only used for the greeting on the home screen.</p>
      <input autoFocus placeholder="Your name" value={name} onChange={(e) => setName(e.target.value)} />
      <div className="ob-actions">
        <button type="button" className="ghost" onClick={onBack}><MdArrowBack size={18} /> Back</button>
        <button className="primary">{name.trim() ? 'Continue' : 'Skip'} <MdArrowForward size={18} /></button>
      </div>
    </motion.form>
  );
}
