'use client';
import { useEffect, useState } from 'react';
import { motion } from 'framer-motion';
import { MdArrowForward, MdArrowBack, MdClose, MdPlaylistPlay, MdLock } from 'react-icons/md';
import useSpotifyImport from '@/components/playlists/useSpotifyImport';
import { slide, newId } from '@/components/onboarding/constants';

export default function SpotifyStep({ imports, setImports, onBack, onNext }) {
  const sp = useSpotifyImport();
  const [url, setUrl] = useState('');
  const importing = sp.step === 'fetching' || sp.step === 'matching';
  const total = sp.data?.tracks.length ?? 0;

  useEffect(() => {
    if (sp.step !== 'review' || !sp.data) return;
    const tracks = sp.results.map((r) => r?.track).filter(Boolean);
    setImports((list) => [...list, { id: newId(), name: sp.data.name, total: sp.data.tracks.length, tracks }]);
    setUrl('');
    sp.reset();
  }, [sp.step]);

  const submit = (e) => {
    e.preventDefault();
    if (url.trim() && !importing) sp.start(url);
  };

  const leave = (action) => () => {
    sp.reset();
    action();
  };

  return (
    <motion.div className="ob-step" {...slide}>
      <h1>Coming from Spotify?</h1>
      <p>Paste a playlist or album link and the songs get matched on SoundCloud. Public ones just work.</p>

      <form className="ob-spotify" onSubmit={submit}>
        <input placeholder="https://open.spotify.com/playlist/…" value={url} disabled={importing} onChange={(e) => setUrl(e.target.value)} />
        <button className="tonal" disabled={!url.trim() || importing}>{sp.step === 'fetching' ? 'Reading…' : 'Import'}</button>
      </form>

      <div className="ob-status">
        {sp.step === 'matching' && (
          <>
            <span className="muted">Finding “{sp.data?.name}” on SoundCloud… {sp.done} / {total}</span>
            <div className="imp-bar"><motion.i animate={{ width: `${total ? (sp.done / total) * 100 : 0}%` }} transition={{ type: 'spring', stiffness: 160, damping: 24 }} /></div>
          </>
        )}
        {sp.step === 'login' && (
          <>
            <span className="muted">That playlist is private. Sign in to Spotify in the window that opens and the import carries on. Your password goes to Spotify, not to Mosaic.</span>
            <div className="ob-inline">
              <button className="tonal" onClick={sp.signIn}><MdLock size={18} /> Sign in to Spotify</button>
              <button className="ghost" onClick={sp.reset}>Cancel</button>
            </div>
          </>
        )}
        {sp.step === 'input' && sp.error && (
          <>
            <span className="ob-error">{sp.error}</span>
            {sp.canSignIn && <div className="ob-inline"><button className="tonal" onClick={sp.signIn}><MdLock size={18} /> Sign in to Spotify</button></div>}
          </>
        )}
      </div>

      {imports.length > 0 && (
        <div className="ob-imports">
          {imports.map((item) => (
            <span key={item.id} className="src-chip" title={`${item.tracks.length} of ${item.total} songs found`}>
              <MdPlaylistPlay size={18} />{item.name} · {item.tracks.length}/{item.total}
              <button aria-label="Remove" onClick={() => setImports((all) => all.filter((x) => x.id !== item.id))}><MdClose size={14} /></button>
            </span>
          ))}
        </div>
      )}

      <div className="ob-actions">
        <button className="ghost" onClick={leave(onBack)}><MdArrowBack size={18} /> Back</button>
        <button className="primary" disabled={importing} onClick={leave(onNext)}>{imports.length ? 'Continue' : 'Skip for now'} <MdArrowForward size={18} /></button>
      </div>
    </motion.div>
  );
}
