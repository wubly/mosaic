'use client';
import { createContext, useCallback, useContext, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';

const Ctx = createContext(() => {});
export const useToast = () => useContext(Ctx);

export default function ToastProvider({ children }) {
  const [toasts, setToasts] = useState([]);
  const n = useRef(0);

  const show = useCallback((message) => {
    const id = ++n.current;
    setToasts((t) => [...t.slice(-2), { id, message }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 2600);
  }, []);

  return (
    <Ctx.Provider value={show}>
      {children}
      <div className="toasts" aria-live="polite">
        <AnimatePresence>
          {toasts.map((t) => (
            <motion.div key={t.id} className="toast" layout initial={{ opacity: 0, y: 16, scale: 0.96 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, scale: 0.96 }} transition={{ type: 'spring', stiffness: 400, damping: 30 }}>
              {t.message}
            </motion.div>
          ))}
        </AnimatePresence>
      </div>
    </Ctx.Provider>
  );
}
