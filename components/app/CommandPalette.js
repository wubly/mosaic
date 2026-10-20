'use client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { MdSearch, MdPlayArrow, MdBolt } from 'react-icons/md';

export default function CommandPalette({ open, onClose, actions, tracks, onSearch, onPlay }) {
  const [q, setQ] = useState('');
  const [sel, setSel] = useState(0);
  const inputRef = useRef(null);

  useEffect(() => {
    if (open) { setQ(''); setSel(0); setTimeout(() => inputRef.current?.focus(), 30); }
  }, [open]);

  const items = useMemo(() => {
    const s = q.trim().toLowerCase();
    const acts = actions.filter((a) => !s || a.label.toLowerCase().includes(s)).map((a) => ({ kind: 'action', ...a }));
    const seen = new Set();
    const trk = s.length > 1
      ? tracks.filter((t) => `${t.title} ${t.artist}`.toLowerCase().includes(s) && !seen.has(t.id) && seen.add(t.id)).slice(0, 6).map((t) => ({ kind: 'track', id: t.id, label: t.title, hint: t.artist, track: t }))
      : [];
    const web = s ? [{ kind: 'search', id: 'search', label: `Search SoundCloud for “${q.trim()}”`, run: () => onSearch(q.trim()) }] : [];
    return [...trk, ...acts, ...web].slice(0, 10);
  }, [q, actions, tracks, onSearch]);

  useEffect(() => setSel(0), [q]);

  const run = (it) => {
    onClose();
    if (it.kind === 'track') onPlay(it.track);
    else it.run();
  };

  const onKey = (e) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setSel((s) => (s + 1) % Math.max(1, items.length)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSel((s) => (s - 1 + items.length) % Math.max(1, items.length)); }
    else if (e.key === 'Enter' && items[sel]) run(items[sel]);
    else if (e.key === 'Escape') onClose();
  };

  return (
    <AnimatePresence>
      {open && (
        <motion.div className="scrim" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={onClose}>
          <motion.div className="palette" initial={{ opacity: 0, y: -12, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: -8, scale: 0.98 }} transition={{ type: 'spring', stiffness: 420, damping: 32 }} onMouseDown={(e) => e.stopPropagation()}>
            <div className="pal-input">
              <MdSearch size={22} />
              <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} onKeyDown={onKey} placeholder="Type a command or search…" />
              <kbd>Esc</kbd>
            </div>
            <ul>
              {items.map((it, i) => (
                <li key={it.kind + it.id}>
                  <button className={i === sel ? 'sel' : ''} onMouseEnter={() => setSel(i)} onClick={() => run(it)}>
                    <span className="pi">{it.kind === 'track' ? <MdPlayArrow size={20} /> : it.kind === 'search' ? <MdSearch size={20} /> : it.icon || <MdBolt size={20} />}</span>
                    <span className="pl">{it.label}</span>
                    {it.hint && <span className="ph">{it.hint}</span>}
                  </button>
                </li>
              ))}
              {items.length === 0 && <li className="none">No matches</li>}
            </ul>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
