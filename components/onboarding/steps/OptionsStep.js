'use client';
import { motion } from 'framer-motion';
import { MdArrowBack } from 'react-icons/md';
import Switch from '@/components/ui/Switch';
import { slide } from '@/components/onboarding/constants';

export default function OptionsStep({ presence, setPresence, lyrics, setLyrics, busy, onBack, onFinish }) {
  return (
    <motion.div className="ob-step" {...slide}>
      <h1>Two quick things</h1>
      <p>Both can be changed later in Settings.</p>

      <div className="ob-options">
        <label className="switch-row">
          <span><b>Discord status</b><em>Show what you're playing on your Discord profile.</em></span>
          <Switch on={presence} onChange={setPresence} />
        </label>
        <label className="switch-row">
          <span><b>Lyrics panel</b><em>Words follow the music in a panel on the right.</em></span>
          <Switch on={lyrics} onChange={setLyrics} />
        </label>
      </div>

      <div className="ob-actions">
        <button className="ghost" onClick={onBack}><MdArrowBack size={18} /> Back</button>
        <button className="primary" disabled={busy} onClick={onFinish}>{busy ? 'Setting up…' : 'Start listening'}</button>
      </div>
    </motion.div>
  );
}
