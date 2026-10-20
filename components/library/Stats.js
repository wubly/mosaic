'use client';
import { useMemo } from 'react';
import { motion } from 'framer-motion';
import { MdHistory, MdLocalFireDepartment } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Empty from '@/components/ui/Empty';
import { hm } from '@/components/library/time';
import { artStyle } from '@/components/tracks/format';

const DAYS_SHOWN = 14;
const WEEKDAY = ['S', 'M', 'T', 'W', 'T', 'F', 'S'];

const dayKey = (date) => `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`;

function summarize(plays, days) {
  const entries = Object.values(plays);
  const artists = {};
  for (const p of entries) {
    const a = artists[p.track.artist] || (artists[p.track.artist] = { name: p.track.artist, count: 0, seconds: 0, top: p });
    a.count += p.count;
    a.seconds += p.seconds;
    if (p.count > a.top.count) a.top = p;
  }

  const recent = [];
  const today = new Date();
  for (let i = DAYS_SHOWN - 1; i >= 0; i--) {
    const date = new Date(today.getFullYear(), today.getMonth(), today.getDate() - i);
    recent.push({ date, seconds: days[dayKey(date)] || 0 });
  }

  let streak = 0;
  for (let i = recent.length - 1; i >= 0; i--) {
    if (recent[i].seconds > 0) streak++;
    else if (i !== recent.length - 1) break;
  }

  return {
    seconds: entries.reduce((n, p) => n + p.seconds, 0),
    plays: entries.reduce((n, p) => n + p.count, 0),
    tracks: [...entries].sort((a, b) => b.count - a.count || b.seconds - a.seconds).slice(0, 6),
    artists: Object.values(artists).sort((a, b) => b.count - a.count || b.seconds - a.seconds).slice(0, 6),
    artistCount: Object.keys(artists).length,
    recent,
    week: recent.slice(-7).reduce((n, d) => n + d.seconds, 0),
    streak,
  };
}

function Activity({ recent, week }) {
  const peak = Math.max(...recent.map((d) => d.seconds), 1);
  return (
    <section className="stat-block">
      <div className="stat-head">
        <h3>Last 2 weeks</h3>
        <span className="muted">{week ? `${hm(week)} this week` : 'Nothing this week yet'}</span>
      </div>
      <div className="activity">
        {recent.map((d, i) => (
          <div
            className={`day ${i === recent.length - 1 ? 'today' : ''}`}
            key={i}
            title={`${d.date.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' })} · ${d.seconds ? hm(d.seconds) : 'no listening'}`}
          >
            <span className="col">
              <motion.i
                className={d.seconds ? '' : 'none'}
                initial={{ height: 0 }}
                animate={{ height: `${Math.max(d.seconds ? 8 : 3, (d.seconds / peak) * 100)}%` }}
                transition={{ type: 'spring', stiffness: 140, damping: 20, delay: i * 0.02 }}
              />
            </span>
            <em>{WEEKDAY[d.date.getDay()]}</em>
          </div>
        ))}
      </div>
    </section>
  );
}

export default function Stats() {
  const { data } = useData();
  const totals = useMemo(() => summarize(data.plays, data.days || {}), [data.plays, data.days]);

  if (!totals.plays) {
    return <Empty icon={<MdHistory size={32} />} title="No listening data yet" hint="Play a few tracks and your stats will show up here." />;
  }

  const lead = totals.tracks[0];

  return (
    <div className="stats">
      <div className="stat-hero">
        <div className="stat-hero-art" style={artStyle(lead.track)} />
        <div className="stat-hero-txt">
          <span className="eyebrow">Most played</span>
          <b>{lead.track.title}</b>
          <span className="muted">{lead.track.artist}</span>
          <span className="stat-hero-meta">{plural(lead.count, 'play')}{lead.seconds >= 60 ? ` · ${hm(lead.seconds)} listened` : ''}</span>
        </div>
      </div>

      <div className="stat-cards">
        <div><b>{hm(totals.seconds)}</b><span>listened</span></div>
        <div><b>{totals.plays}</b><span>plays</span></div>
        <div><b>{totals.artistCount}</b><span>artists</span></div>
        <div><b>{totals.streak}{totals.streak > 1 && <MdLocalFireDepartment size={22} />}</b><span>day streak</span></div>
      </div>

      <Activity recent={totals.recent} week={totals.week} />

      <div className="stat-cols">
        <section>
          <h3>Top tracks</h3>
          {totals.tracks.map((p, i) => (
            <div className="rank-row" key={p.track.id}>
              <span className="rank">{i + 1}</span>
              <span className="thumb sm" style={artStyle(p.track)} />
              <span className="bl">{p.track.title}<em>{p.track.artist}</em></span>
              <span className="bn">{p.count}×</span>
            </div>
          ))}
        </section>

        <section>
          <h3>Top artists</h3>
          {totals.artists.map((a, i) => (
            <div className="rank-row" key={a.name}>
              <span className="rank">{i + 1}</span>
              <span className="thumb sm round" style={artStyle(a.top.track)} />
              <span className="bl">{a.name}<em>{a.seconds >= 60 ? `${hm(a.seconds)} listened` : plural(a.count, 'play')}</em></span>
              <span className="bn">{a.count}×</span>
            </div>
          ))}
        </section>
      </div>
    </div>
  );
}
