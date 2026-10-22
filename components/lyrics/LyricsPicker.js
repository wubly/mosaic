'use client';
import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { MdClose, MdCheck, MdSearch, MdAutoAwesome } from 'react-icons/md';
import { useToast } from '@/components/ui/Toast';
import Switch from '@/components/ui/Switch';
import { useSpotify } from '@/components/spotify/SpotifyProvider';

export const KIND_LABEL = {
  word: 'Word synced',
  synced: 'Synced',
  plain: 'Unsynced',
  instrumental: 'Instrumental',
  none: 'No lyrics',
};

const mmss = (s) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, '0')}`;

export function KindBadge({ kind }) {
  return <span className={`kind ${kind}`}>{KIND_LABEL[kind] || kind}</span>;
}

export default function LyricsPicker({ track, current, onClose, onPicked }) {
  const toast = useToast();
  const [data, setData] = useState({ list: null, query: '' });
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState(true);
  const { beta } = useSpotify();
  const [genius, setGenius] = useState(false);
  useEffect(() => { window.mosaic.getGenius().then(setGenius); }, []);

  const args = { id: track.id, title: track.title, artist: track.artist, duration: track.duration, lrcPath: track.lrcPath };

  const load = (query) => {
    setBusy(true);
    window.mosaic
      .lyricCandidates({ ...args, query })
      .then((r) => { setData(r); if (query === undefined) setQ(r.query); })
      .catch(() => setData({ list: [], query: '' }))
      .finally(() => setBusy(false));
  };

  useEffect(() => { load(); }, []);

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && (e.stopPropagation(), onClose());
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  const pick = async (candId) => {
    const lyrics = await window.mosaic.selectLyrics({ ...args, candId });
    onPicked(lyrics);
    toast(candId ? 'Lyrics changed' : 'Using the automatic match');
    onClose();
  };

  const toggleGenius = async (on) => {
    setGenius(on);
    await window.mosaic.setGenius(on);
    if (on && !current?.id) onPicked(await window.mosaic.selectLyrics({ ...args, candId: null }));
  };

  const isCurrent = (c) => (c.id === 'file' ? current?.origin === 'file' : current?.id === c.id);
  const list = data.list ?? [];

  return (
    <motion.div className="lyr-pick" initial={{ opacity: 0, y: 24 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 24 }} transition={{ type: 'spring', stiffness: 380, damping: 34 }}>
      <header>
        <b>Lyrics sources</b>
        <button className="icon" title="Close (Esc)" onClick={onClose}><MdClose size={22} /></button>
      </header>

      <form className="search small" onSubmit={(e) => { e.preventDefault(); load(q.trim() || undefined); }}>
        <MdSearch size={20} />
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search LRCLIB" />
      </form>

      <dl className="lyr-legend">
        <div><KindBadge kind="word" /><span>Timed word by word</span></div>
        <div><KindBadge kind="synced" /><span>Timed line by line</span></div>
        <div><KindBadge kind="plain" /><span>Plain text, no timing</span></div>
      </dl>

      {beta && <label className="lyr-genius">
        <span>
          <b>Genius <span className="beta">beta</span></b>
          <em>{genius ? 'On. Used when LRCLIB has nothing; plain lyrics only.' : 'Scrape plain, unsynced lyrics from genius.com when LRCLIB has nothing. Can break if Genius changes its pages.'}</em>
        </span>
        <Switch on={genius} onChange={toggleGenius} />
      </label>}

      <ul>
        {current?.origin === 'custom' && (
          <li>
            <button className="lyr-opt auto" onClick={() => pick(null)}>
              <MdAutoAwesome size={20} />
              <span className="lo-text"><b>Use automatic match</b><em>Let Mosaic choose the best version</em></span>
            </button>
          </li>
        )}

        {busy && list.length === 0 && Array.from({ length: 5 }).map((_, i) => <li key={i}><div className="lyr-skel" /></li>)}
        {!busy && list.length === 0 && <li className="lyr-none">Nothing found. Try another search.</li>}

        {list.map((c) => {
          const on = isCurrent(c);
          return (
            <li key={c.id}>
              <button className={`lyr-opt ${on ? 'on' : ''}`} onClick={() => pick(c.id)}>
                <span className="lo-text">
                  <b>{c.title}</b>
                  <em>{[c.artist, c.album].filter(Boolean).join(' · ')}</em>
                  <span className="lo-tags">
                    <KindBadge kind={c.kind} />
                    {c.duration > 0 && <span className="lo-dur">{mmss(c.duration)}{c.diff ? ` (${c.diff > 0 ? '+' : ''}${c.diff}s)` : ''}</span>}
                  </span>
                </span>
                {on && <MdCheck size={22} className="lo-check" />}
              </button>
            </li>
          );
        })}
      </ul>
    </motion.div>
  );
}
