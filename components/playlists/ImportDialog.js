'use client';
import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { MdClose, MdCheckBox, MdCheckBoxOutlineBlank, MdArrowForward, MdLock, MdCloudDownload, MdSearchOff } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import useSpotifyImport from '@/components/playlists/useSpotifyImport';

export default function ImportDialog({ onClose, onCreated }) {
  const lib = useData();
  const toast = useToast();
  const sp = useSpotifyImport();
  const [url, setUrl] = useState('');
  const [name, setName] = useState('');
  const [skip, setSkip] = useState(() => new Set());

  useEffect(() => {
    const onKey = (e) => e.key === 'Escape' && (e.stopPropagation(), onClose());
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  useEffect(() => { if (sp.data) setName(sp.data.name); }, [sp.data]);

  const { step, data, results, done } = sp;
  const matched = results.map((r, i) => (r && !skip.has(i) ? r.track : null)).filter(Boolean);
  const total = data?.tracks.length ?? 0;
  const missing = results.filter((r) => r === null).length;

  const create = () => {
    const id = lib.createPlaylist(name, matched);
    toast(`Created “${name.trim() || 'Untitled playlist'}” with ${matched.length} tracks`);
    onCreated(id);
    onClose();
  };

  const toggle = (i) => setSkip((s) => { const n = new Set(s); n.has(i) ? n.delete(i) : n.add(i); return n; });

  return (
    <motion.div className="scrim" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={onClose}>
      <motion.div className="dialog" initial={{ opacity: 0, y: 24, scale: 0.97 }} animate={{ opacity: 1, y: 0, scale: 1 }} exit={{ opacity: 0, y: 16, scale: 0.98 }} transition={{ type: 'spring', stiffness: 380, damping: 32 }} onMouseDown={(e) => e.stopPropagation()}>
        <header className="dlg-head">
          <div>
            <h2>Import from Spotify</h2>
            <p className="muted">Bring a playlist or album over. Songs are matched on SoundCloud.</p>
          </div>
          <button className="icon" title="Close (Esc)" onClick={onClose}><MdClose size={22} /></button>
        </header>

        {(step === 'input' || step === 'fetching') && (
          <div className="dlg-body">
            <form className="imp-form" onSubmit={(e) => { e.preventDefault(); url.trim() && sp.start(url); }}>
              <input autoFocus value={url} onChange={(e) => setUrl(e.target.value)} placeholder="Paste a Spotify playlist or album link" disabled={step === 'fetching'} />
              <button className="primary" disabled={!url.trim() || step === 'fetching'}>{step === 'fetching' ? 'Reading…' : <>Import <MdArrowForward size={18} /></>}</button>
            </form>
            {sp.error && (
              <div className="error">
                {sp.error}
                {sp.canSignIn && <button className="ghost" type="button" onClick={sp.signIn}><MdLock size={18} /> Sign in to Spotify</button>}
              </div>
            )}
            <p className="muted small">Works with public playlists right away. Private or shared-by-link playlists need you to sign in once, in a Spotify window. Mosaic never sees your password.</p>
          </div>
        )}

        {step === 'login' && (
          <div className="dlg-body imp-center">
            <div className="empty-ico"><MdLock size={32} /></div>
            <b>This playlist is private</b>
            <p className="muted">Sign in to Spotify in the window that opens. Your password goes straight to Spotify, never to Mosaic. After signing in, the import continues automatically.</p>
            <div className="page-actions">
              <button className="primary" onClick={sp.signIn}><MdLock size={18} /> Sign in to Spotify</button>
              <button className="ghost" onClick={sp.back}>Back</button>
            </div>
          </div>
        )}

        {step === 'matching' && (
          <div className="dlg-body imp-center">
            <div className="empty-ico"><MdCloudDownload size={32} /></div>
            <b>{data?.name}</b>
            <p className="muted">Finding {total} songs on SoundCloud… {done} / {total}</p>
            <div className="imp-bar"><motion.i animate={{ width: `${total ? (done / total) * 100 : 0}%` }} transition={{ type: 'spring', stiffness: 160, damping: 24 }} /></div>
          </div>
        )}

        {step === 'review' && (
          <>
            <div className="dlg-sub">
              <input className="imp-name" value={name} onChange={(e) => setName(e.target.value)} aria-label="Playlist name" />
              <span className="muted">{matched.length} of {total} songs will be added{missing ? ` · ${missing} not found` : ''}</span>
            </div>
            <ul className="imp-list">
              {data.tracks.map((s, i) => {
                const m = results[i];
                const off = !m || skip.has(i);
                return (
                  <li key={i} className={`imp-row ${m ? '' : 'miss'} ${off ? 'off' : ''}`}>
                    <button className="imp-check" disabled={!m} onClick={() => toggle(i)} aria-label={off ? 'Include' : 'Skip'}>
                      {m ? (off ? <MdCheckBoxOutlineBlank size={22} /> : <MdCheckBox size={22} />) : <MdSearchOff size={22} />}
                    </button>
                    <span className="imp-col"><b>{s.title}</b><em>{s.artist}</em></span>
                    <MdArrowForward size={16} className="imp-arrow" />
                    <span className="imp-col">
                      {m ? <><b>{m.track.title}</b><em>{m.track.artist} · {Math.round(m.score * 100)}% match</em></> : <em>No match on SoundCloud</em>}
                    </span>
                  </li>
                );
              })}
            </ul>
            <footer className="dlg-foot">
              <button className="ghost" onClick={onClose}>Cancel</button>
              <button className="primary" disabled={!matched.length} onClick={create}>Create playlist · {matched.length}</button>
            </footer>
          </>
        )}
      </motion.div>
    </motion.div>
  );
}
