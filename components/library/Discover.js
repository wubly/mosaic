'use client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { MdSearch, MdHistory } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Logo from '@/components/ui/Logo';
import TrackRow from '@/components/tracks/TrackRow';
import TrackTile from '@/components/tracks/TrackTile';
import { gridVariants } from '@/components/tracks/variants';

export default function Discover({ name, hidden, request }) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [more, setMore] = useState({ seed: null, list: [], busy: false });
  const seq = useRef(0);
  const inputRef = useRef(null);
  const lib = useData();

  async function run(q) {
    q = q.trim();
    const id = ++seq.current;
    if (!q) { setResults([]); setBusy(false); return; }
    setBusy(true);
    setError('');
    try {
      const r = await window.mosaic.search(q);
      if (id === seq.current) setResults(r);
    } catch (e) {
      if (id === seq.current) setError(e.message);
    } finally {
      if (id === seq.current) setBusy(false);
    }
  }

  useEffect(() => {
    const t = setTimeout(() => (query.trim().length > 1 ? run(query) : !query.trim() && run('')), 400);
    return () => clearTimeout(t);
  }, [query]);

  useEffect(() => {
    if (!request) return;
    if (request.q) { setQuery(request.q); run(request.q); }
    inputRef.current?.focus();
  }, [request]);

  const recent = useMemo(() => {
    const seen = new Set();
    return lib.data.history.map((h) => h.track).filter((t) => !seen.has(t.id) && seen.add(t.id)).slice(0, 12);
  }, [lib.data.history]);

  const seed = recent.find((t) => t.source === 'soundcloud') || null;
  useEffect(() => {
    if (!seed) { setMore({ seed: null, list: [], busy: false }); return; }
    let live = true;
    setMore((m) => ({ ...m, seed, busy: true }));
    const have = new Set(recent.map((t) => t.id));
    window.mosaic.related(seed.id)
      .then((list) => live && setMore({ seed, list: list.filter((t) => !have.has(t.id)), busy: false }))
      .catch(() => live && setMore({ seed, list: [], busy: false }));
    return () => { live = false; };
  }, [seed?.id]);

  const searching = query.trim().length > 0;
  const q = query.trim().toLowerCase();
  const deviceHits = useMemo(
    () => (q.length > 1 ? lib.local.tracks.filter((t) => `${t.title} ${t.artist} ${t.album}`.toLowerCase().includes(q)).slice(0, 5) : []),
    [q, lib.local.tracks],
  );

  return (
    <div className="library" hidden={hidden}>
      <header className="page-head">
        <div>
          <Logo size={18} className="small" />
          <h2>{name ? `Hey ${name}, what's next?` : "What's next?"}</h2>
        </div>
      </header>

      <form className="search" onSubmit={(e) => { e.preventDefault(); run(query); }}>
        <MdSearch size={22} />
        <input ref={inputRef} data-search placeholder="Search SoundCloud and your library…" value={query} onChange={(e) => setQuery(e.target.value)} />
      </form>

      {searching ? (
        <>
          {deviceHits.length > 0 && (
            <>
              <h3>On your device</h3>
              <ol className="rows">{deviceHits.map((t, i) => <TrackRow key={t.id} t={t} i={i} list={deviceHits} showIndex={false} />)}</ol>
            </>
          )}

          <h3>{busy ? 'Searching…' : `Results for “${query.trim()}”`}</h3>
          <AnimatePresence>
            {error && <motion.div className="error" initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }}>{error}</motion.div>}
          </AnimatePresence>
          {busy && results.length === 0 && (
            <div className="grid">{Array.from({ length: 12 }).map((_, i) => <div key={i} className="skel"><i /><b /><b /></div>)}</div>
          )}
          {!busy && !error && results.length === 0 && <p className="muted">Nothing found.</p>}
          <motion.div className="grid" key={`s-${query.trim()}`} initial="hidden" animate="show" variants={gridVariants}>
            {results.map((t) => <TrackTile key={t.id} t={t} list={results} />)}
          </motion.div>
        </>
      ) : recent.length > 0 ? (
        <>
          <h3>Your last listening</h3>
          <motion.div className="grid" initial="hidden" animate="show" variants={gridVariants}>
            {recent.map((t) => <TrackTile key={t.id} t={t} list={recent} />)}
          </motion.div>

          {more.seed && (more.busy || more.list.length > 0) && (
            <>
              <h3>More like “{more.seed.title}”</h3>
              {more.busy && more.list.length === 0 ? (
                <div className="grid">{Array.from({ length: 6 }).map((_, i) => <div key={i} className="skel"><i /><b /><b /></div>)}</div>
              ) : (
                <motion.div className="grid" initial="hidden" animate="show" variants={gridVariants}>
                  {more.list.map((t) => <TrackTile key={t.id} t={t} list={more.list} />)}
                </motion.div>
              )}
            </>
          )}
        </>
      ) : (
        <div className="empty">
          <div className="empty-ico"><MdHistory size={32} /></div>
          <b>Nothing played yet</b>
          <p className="muted">Search for a song above, or add your own music under Device.<br />What you listen to shows up here.</p>
        </div>
      )}
    </div>
  );
}
