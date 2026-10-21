'use client';
import { useEffect, useMemo, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { MdSubtitles, MdExpandMore, MdLyrics } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import LyricsPicker, { KIND_LABEL } from '@/components/lyrics/LyricsPicker';
import { buildTimings, clamp01 } from '@/components/lyrics/timing';

export default function LyricsPanel({ large = false, bare = false }) {
  const { track, audioRef, seek } = usePlayer();
  const [lyrics, setLyrics] = useState(null);
  const [active, setActive] = useState(-1);
  const boxRef = useRef(null);
  const lineRefs = useRef([]);
  const wordRefs = useRef([]);
  const activeRef = useRef(-1);
  const [picker, setPicker] = useState(false);

  const applyLyrics = (l) => {
    activeRef.current = -1;
    setActive(-1);
    setLyrics(l);
    boxRef.current?.scrollTo({ top: 0 });
  };

  const fetchLyrics = (live = () => true) => {
    setLyrics(null);
    activeRef.current = -1;
    setActive(-1);
    window.mosaic
      .getLyrics({ id: track.id, title: track.title, artist: track.artist, duration: track.duration, lrcPath: track.lrcPath })
      .then((l) => live() && setLyrics(l))
      .catch(() => live() && setLyrics({ synced: [], plain: '', meta: { kind: 'none' } }));
  };

  useEffect(() => {
    if (!track) return;
    let alive = true;
    setPicker(false);
    fetchLyrics(() => alive);
    return () => { alive = false; };
  }, [track?.id]);

  const synced = lyrics?.synced ?? [];
  const timings = useMemo(() => buildTimings(synced), [lyrics]);

  useEffect(() => {
    if (!synced.length) return;
    wordRefs.current.length = synced.length;
    let raf;
    let lastT = -1;
    const paintLine = (i, mode, t) => {
      const spans = wordRefs.current[i];
      if (!spans) return;
      for (let k = 0; k < spans.length; k++) {
        const el = spans[k];
        if (!el) continue;
        const p = mode === 'live' ? clamp01((t - timings[i][k].s) / Math.max(0.05, timings[i][k].e - timings[i][k].s)) : mode;
        el.style.setProperty('--p', p.toFixed(3));
      }
    };
    const frame = () => {
      raf = requestAnimationFrame(frame);
      if (document.hidden) return;
      const audio = audioRef.current;
      const now = audio?.currentTime ?? 0;
      if (audio?.paused && now === lastT) return;
      lastT = now;
      const t = now + 0.04;
      let i = activeRef.current;
      while (i + 1 < synced.length && synced[i + 1].t <= t) i++;
      while (i >= 0 && synced[i].t > t) i--;
      if (i !== activeRef.current) {
        activeRef.current = i;
        setActive(i);
        for (let j = 0; j < synced.length; j++) if (j !== i) paintLine(j, j < i ? 1 : 0);
      }
      if (i >= 0) paintLine(i, 'live', t);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [lyrics, timings]);

  useEffect(() => {
    const box = boxRef.current;
    const el = lineRefs.current[active];
    if (!box || !el) return;
    box.scrollTo({ top: el.offsetTop - box.clientHeight / 2 + el.clientHeight / 2, behavior: 'smooth' });
  }, [active]);

  useEffect(() => { boxRef.current?.scrollTo({ top: 0 }); }, [track?.id]);

  if (!track) {
    return (
      <div className="lyr-wrap">
        <div className="side-empty">
          <div className="empty-ico"><MdLyrics size={32} /></div>
          <b>No lyrics yet</b>
          <p className="muted">Play a song and its lyrics show up here, line by line.</p>
        </div>
      </div>
    );
  }

  const kind = lyrics === null ? null : lyrics.meta?.kind || (synced.length ? 'synced' : lyrics.plain ? 'plain' : 'none');
  const empty = lyrics !== null && synced.length === 0 && !lyrics.plain;
  const sourcePill = bare ? null : (
    <button className={`lyr-source ${kind || 'loading'}`} onClick={() => setPicker(true)} title="Choose another version of the lyrics">
      <MdSubtitles size={16} />
      <span>{kind ? KIND_LABEL[kind] : 'Finding lyrics…'}</span>
      <MdExpandMore size={18} />
    </button>
  );

  return (
    <div className={`lyr-wrap ${large ? 'large' : ''} ${bare ? 'bare' : ''} ${empty ? 'empty' : ''}`}>
      {!large && !bare && (
        <header className="lyr-head">
          <motion.div key={track.id} className="lyr-art" initial={{ opacity: 0, scale: 0.94 }} animate={{ opacity: 1, scale: 1 }} style={{ backgroundImage: (track.thumb || track.artwork) ? `url(${track.thumb || track.artwork})` : undefined }} />
          <h3>{track.title}</h3>
          <p>{track.artist}</p>
          {sourcePill}
        </header>
      )}
      {large && !empty && <div className="lyr-pill-float">{sourcePill}</div>}

      <div className="lyrics" ref={boxRef}>
        {lyrics === null && <p className="lyr-empty">Finding lyrics…</p>}

        {lyrics && synced.map((l, i) => {
          const d = Math.abs(i - active);
          return (
            <motion.p
              key={`${track.id}-${i}`}
              ref={(el) => { lineRefs.current[i] = el; }}
              className="line"
              onClick={() => seek(l.t)}
              animate={{ opacity: d === 0 ? 1 : d < 3 ? 0.55 : 0.28, scale: d === 0 ? 1 : 0.96, filter: `blur(${d === 0 ? 0 : Math.min(d, 4) * 0.5}px)` }}
              transition={{ duration: 0.35 }}
            >
              {timings[i]?.map((w, k) => (
                <span key={k}>
                  <span className="w" ref={(el) => { (wordRefs.current[i] ||= [])[k] = el; }}>{w.w}</span>{' '}
                </span>
              ))}
            </motion.p>
          );
        })}

        {lyrics && synced.length === 0 && lyrics.plain && <p className="plain">{lyrics.plain}</p>}
        {empty && large && <div className="lyr-empty-center">{sourcePill}</div>}
      </div>

      <AnimatePresence>
        {picker && <LyricsPicker key="picker" track={track} current={lyrics?.meta} onClose={() => setPicker(false)} onPicked={applyLyrics} />}
      </AnimatePresence>
    </div>
  );
}
