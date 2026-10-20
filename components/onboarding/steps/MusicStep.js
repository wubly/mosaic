'use client';
import { motion } from 'framer-motion';
import { MdArrowForward, MdArrowBack, MdCreateNewFolder, MdFolder, MdClose } from 'react-icons/md';
import { slide, baseName } from '@/components/onboarding/constants';

export default function MusicStep({ folders, setFolders, onBack, onNext }) {
  const choose = async () => {
    const picked = await window.mosaic.pickFolder();
    if (picked.length) setFolders((current) => [...new Set([...current, ...picked])]);
  };

  return (
    <motion.div className="ob-step" {...slide}>
      <h1>Got music on this computer?</h1>
      <p>Pick a folder and it shows up next to SoundCloud. You can add more any time, or drag files into the window.</p>

      <button className="tonal ob-pick" onClick={choose}><MdCreateNewFolder size={20} /> Choose a folder</button>

      {folders.length > 0 && (
        <div className="ob-folders">
          {folders.map((folder) => (
            <span key={folder} className="src-chip" title={folder}>
              <MdFolder size={16} />{baseName(folder)}
              <button aria-label="Remove" onClick={() => setFolders((all) => all.filter((f) => f !== folder))}><MdClose size={14} /></button>
            </span>
          ))}
        </div>
      )}

      <div className="ob-actions">
        <button className="ghost" onClick={onBack}><MdArrowBack size={18} /> Back</button>
        <button className="primary" onClick={onNext}>{folders.length ? 'Continue' : 'Skip for now'} <MdArrowForward size={18} /></button>
      </div>
    </motion.div>
  );
}
