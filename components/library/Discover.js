'use client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { useData } from '@/components/data/DataProvider';
import { filterAlong } from '@/components/player/queue/relevance';
import { useSpotify } from '@/components/spotify/SpotifyProvider';
import { AlbumCards, ArtistCards } from '@/components/spotify/SpotifyView';
import { useYoutube } from '@/components/youtube/YoutubeProvider';
import SearchBar from '@/components/ui/SearchBar';
import Logo from '@/components/ui/Logo';
import TrackRow from '@/components/tracks/TrackRow';
import TrackTile from '@/components/tracks/TrackTile';
import { gridVariants } from '@/components/tracks/variants';

const GENRES = [
  ['all-music', 'Trending'],
  ['hip hop', 'Hip hop'],
  ['electronic', 'Electronic'],
  ['pop', 'Pop'],
  ['rnb', 'R&B'],
  ['rock', 'Rock'],
  ['lofi', 'Lo-fi'],
];

const NO_SP = { tracks: [], artists: [], albums: [] };
const queueable = (list) => filterAlong(list, null);

const Skeletons = ({ n = 6 }) => <div className="shelf">{Array.from({ length: n }).map((_, i) => <div key={i} className="skel tile"><i /><b /><b /></div>)}</div>;

export default function Discover({ name, hidden, request }) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [more, setMore] = useState({ seed: null, list: [], busy: false });
  const [genre, setGenre] = useState('all-music');
  const [charts, setCharts] = useState({ genre: null, list: [] });
  const spotify = useSpotify();
  const youtube = useYoutube();
  const [ytResults, setYtResults] = useState([]);
  const [ytLiked, setYtLiked] = useState([]);
  const [spResults, setSpResults] = useState({ tracks: [], artists: [], albums: [] });
  const [spSaved, setSpSaved] = useState([]);
  const seq = useRef(0);
  const inputRef = useRef(null);
  const lib = useData();

  async function run(q) {
    q = q.trim();
    const id = ++seq.current;
    if (!q) { setResults([]); setSpResults(NO_SP); setYtResults([]); setBusy(false); return; }
    if (youtube.enabled) window.mosaic.ytSearch(q).then((r) => id === seq.current && setYtResults(r)).catch(() => id === seq.current && setYtResults([]));
    if (spotify.enabled) window.mosaic.spSearch(q).then((r) => id === seq.current && setSpResults(r)).catch(() => id === seq.current && setSpResults(NO_SP));
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
    const trimmed = query.trim();
    if (trimmed.length > 1) { setBusy(true); setError(''); }
    const t = setTimeout(() => (trimmed.length > 1 ? run(query) : !trimmed && run('')), 400);
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
    window.mosaic.related(seed.id, 40)
      .then((list) => live && setMore({ seed, list: filterAlong(list, seed).filter((t) => !have.has(t.id)).slice(0, 20), busy: false }))
      .catch(() => live && setMore({ seed, list: [], busy: false }));
    return () => { live = false; };
  }, [seed?.id]);

  useEffect(() => {
    let live = true;
    window.mosaic.charts(genre)
      .then((list) => live && setCharts({ genre, list }))
      .catch(() => live && setCharts({ genre, list: [] }));
    return () => { live = false; };
  }, [genre]);

  useEffect(() => {
    if (!spotify.enabled) { setSpSaved([]); return; }
    let live = true;
    window.mosaic.spLibrary().then((lib) => live && setSpSaved(lib.saved)).catch(() => {});
    return () => { live = false; };
  }, [spotify.enabled]);

  useEffect(() => {
    if (!youtube.enabled) { setYtLiked([]); return; }
    let live = true;
    window.mosaic.ytLibrary().then((lib) => live && setYtLiked(lib.liked)).catch(() => {});
    return () => { live = false; };
  }, [youtube.enabled]);

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

      <SearchBar value={query} onChange={setQuery} onSubmit={run} busy={busy} inputRef={inputRef} placeholder="Search songs, artists…" />

      {searching ? (
        <>
          {deviceHits.length > 0 && (
            <>
              <h3>On your device</h3>
              <ol className="rows">{deviceHits.map((t, i) => <TrackRow key={t.id} t={t} i={i} list={deviceHits} showIndex={false} />)}</ol>
            </>
          )}

          {spResults.artists.length > 0 && (<><h3>Spotify artists <span className="beta-tag">beta</span></h3><ArtistCards artists={spResults.artists} go={spotify.go} /></>)}
          {spResults.albums.length > 0 && (<><h3>Spotify albums</h3><AlbumCards albums={spResults.albums} go={spotify.go} /></>)}
          {spResults.tracks.length > 0 && (
            <>
              <h3>Spotify songs</h3>
              <div className="shelf">{spResults.tracks.map((t) => <TrackTile key={t.id} t={t} list={spResults.tracks} />)}</div>
            </>
          )}

          {ytResults.length > 0 && (
            <>
              <h3>On YouTube <span className="beta-tag">beta</span></h3>
              <div className="shelf">{ytResults.map((t) => <TrackTile key={t.id} t={t} list={ytResults} />)}</div>
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
          <motion.div className="grid" animate={{ opacity: busy && results.length ? 0.45 : 1 }} transition={{ duration: 0.15 }} style={{ pointerEvents: busy ? 'none' : undefined }}>
            {results.map((t) => <TrackTile key={t.id} t={t} list={queueable(results)} />)}
          </motion.div>
        </>
      ) : (
        <>
          {recent.length > 0 && (
            <>
              <h3>Jump back in</h3>
              <div className="shelf">{recent.map((t) => <TrackTile key={t.id} t={t} list={recent} />)}</div>
            </>
          )}

          {spSaved.length > 0 && (
            <>
              <h3>Your Spotify likes <span className="beta-tag">beta</span></h3>
              <div className="shelf">{spSaved.map((t) => <TrackTile key={t.id} t={t} list={spSaved} />)}</div>
            </>
          )}

          {ytLiked.length > 0 && (
            <>
              <h3>Liked on YouTube <span className="beta-tag">beta</span></h3>
              <div className="shelf">{ytLiked.map((t) => <TrackTile key={t.id} t={t} list={ytLiked} />)}</div>
            </>
          )}

          <div className="feed-head">
            <h3>{GENRES.find(([g]) => g === genre)[1]}</h3>
            <div className="chips">
              {GENRES.map(([g, label]) => <button key={g} className={g === genre ? 'on' : ''} onClick={() => setGenre(g)}>{label}</button>)}
            </div>
          </div>
          {charts.genre !== genre ? (
            <Skeletons n={8} />
          ) : charts.list.length === 0 ? (
            <p className="muted">Couldn’t load this right now.</p>
          ) : (
            <motion.div className="shelf" key={genre} initial="hidden" animate="show" variants={gridVariants}>
              {charts.list.map((t) => <TrackTile key={t.id} t={t} list={queueable(charts.list)} />)}
            </motion.div>
          )}

          {more.seed && (more.busy || more.list.length > 0) && (
            <>
              <h3>Because you played “{more.seed.title}”</h3>
              {more.busy && more.list.length === 0 ? <Skeletons /> : (
                <div className="shelf">{more.list.map((t) => <TrackTile key={t.id} t={t} list={more.list} />)}</div>
              )}
            </>
          )}
        </>
      )}
    </div>
  );
}
