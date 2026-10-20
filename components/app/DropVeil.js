'use client';
import { AnimatePresence, motion } from 'framer-motion';
import { MdUploadFile } from 'react-icons/md';

export default function DropVeil({ visible }) {
  return (
    <AnimatePresence>
      {visible && (
        <motion.div className="dropveil" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }}>
          <MdUploadFile size={56} />
          <b>Drop to add to Mosaic</b>
          <span>Music files and folders are added to your library</span>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
