'use client';
import { useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { MdSearch, MdClose } from 'react-icons/md';

const SPRING = { type: 'spring', stiffness: 420, damping: 32 };
const POP = { initial: { opacity: 0, scale: 0.5 }, animate: { opacity: 1, scale: 1 }, exit: { opacity: 0, scale: 0.5 }, transition: { duration: 0.14 } };

export default function SearchBar({ value, onChange, onSubmit, busy, inputRef, placeholder }) {
  const [focused, setFocused] = useState(false);

  return (
    <motion.form
      className="search"
      onSubmit={(e) => { e.preventDefault(); onSubmit(value); }}
      initial={false}
      animate={{
        maxWidth: focused ? 640 : 520,
        boxShadow: focused ? '0 0 0 3px color-mix(in srgb, var(--md-primary) 22%, transparent)' : '0 0 0 0px transparent',
      }}
      transition={SPRING}
    >
      <motion.span className="search-ico" animate={{ scale: focused ? 1.1 : 1, color: focused ? 'var(--md-primary)' : 'var(--md-on-surface-variant)' }} transition={SPRING}>
        <MdSearch size={22} />
      </motion.span>
      <input
        ref={inputRef}
        data-search
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={(e) => { if (e.key === 'Escape') { onChange(''); e.target.blur(); } }}
      />
      <AnimatePresence initial={false} mode="popLayout">
        {busy && <motion.span key="busy" className="search-spin" {...POP}><i /></motion.span>}
        {!busy && value && (
          <motion.button key="clear" type="button" className="icon search-clear" aria-label="Clear search" {...POP} whileTap={{ scale: 0.85 }}
            onClick={() => { onChange(''); inputRef?.current?.focus(); }}>
            <MdClose size={18} />
          </motion.button>
        )}
        {!busy && !value && !focused && <motion.kbd key="hint" {...POP}>/</motion.kbd>}
      </AnimatePresence>
    </motion.form>
  );
}
