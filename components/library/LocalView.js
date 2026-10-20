'use client';
import { useMemo, useState } from 'react';
import { MdCreateNewFolder, MdAudioFile, MdClose, MdRefresh, MdSearch, MdLibraryMusic, MdFolder, MdPlayArrow, MdShuffle } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { usePlayer } from '@/components/player/PlayerProvider';
import TrackRow from '@/components/tracks/TrackRow';
import PageHeader from '@/components/ui/PageHeader';
import VirtualList from '@/components/ui/VirtualList';

const SORTS = { artist: 'Artist', folder: 'Folder', title: 'Title', album: 'Album', length: 'Length' };
const UNKNOWN = 'Unknown artist';
const text = (a, b) => a.localeCompare(b, undefined, { sensitivity: 'base', numeric: true });
const base = (p) => p.split(/[\\/]/).filter(Boolean).pop() || p;

const comparators = {
  artist: (a, b) => (a.artist === UNKNOWN) - (b.artist === UNKNOWN) || text(a.artist, b.artist) || text(a.album, b.album) || text(a.title, b.title),
  folder: (a, b) => text(a.folder || '', b.folder || '') || text(a.artist, b.artist) || text(a.album, b.album) || text(a.title, b.title),
  title: (a, b) => text(a.title, b.title),
  album: (a, b) => text(a.album, b.album) || text(a.title, b.title),
  length: (a, b) => b.duration - a.duration,
};

const groupKey = { artist: (t) => t.artist, folder: (t) => t.folder || 'Other' };

function GroupHeader({ row, onPlay }) {
  const cover = row.list.find((t) => t.thumb || t.artwork);
  const src = cover && (cover.thumb || cover.artwork);
  return (
    <div className="grp-head">
      <span className="grp-av" style={{ backgroundImage: src ? `url(${src})` : undefined }}>{!src && row.name.charAt(0).toUpperCase()}</span>
      <span className="grp-txt">
        <b>{row.name}</b>
        <em>{row.list.length} {row.list.length === 1 ? 'track' : 'tracks'}</em>
      </span>
      <button className="icon" title={`Play ${row.name}`} onClick={() => onPlay(row.list)}><MdPlayArrow size={22} /></button>
    </div>
  );
}

export default function LocalView() {
  const lib = useData();
  const { play, toggleShuffle, shuffle } = usePlayer();
  const [filter, setFilter] = useState('');
  const [sort, setSort] = useState('artist');

  const tracks = useMemo(() => {
    const q = filter.trim().toLowerCase();
    const list = lib.local.tracks.filter((t) => !q || `${t.title} ${t.artist} ${t.album} ${t.folder || ''}`.toLowerCase().includes(q));
    return [...list].sort(comparators[sort]);
  }, [lib.local.tracks, filter, sort]);

  const rows = useMemo(() => {
    const keyOf = groupKey[sort];
    if (!keyOf) return tracks.map((t, i) => ({ id: t.id, track: t, n: i }));
    const out = [];
    let current = null;
    for (const t of tracks) {
      const name = keyOf(t);
      if (!current || current.name !== name) {
        current = { id: `group:${name}`, header: true, name, list: [] };
        out.push(current);
      }
      current.list.push(t);
      out.push({ id: t.id, track: t, n: current.list.length - 1 });
    }
    return out;
  }, [tracks, sort]);

  const playGroup = (list) => play(list[0], tracks);

  const addFolder = async () => { const p = await window.mosaic.pickFolder(); if (p.length) lib.addSources(p); };
  const addFiles = async () => { const p = await window.mosaic.pickFiles(); if (p.length) lib.addSources(p); };

  return (
    <div className="library">
      <PageHeader title="On your device" subtitle={lib.local.scanning ? 'Scanning…' : `${lib.local.tracks.length} tracks from ${lib.data.sources.length} sources`}>
        <button className="primary" onClick={addFolder}><MdCreateNewFolder size={20} /> Add folder</button>
        <button className="tonal" onClick={addFiles}><MdAudioFile size={20} /> Add files</button>
        {lib.data.sources.length > 0 && <button className="icon" title="Rescan" onClick={lib.rescan}><MdRefresh size={22} className={lib.local.scanning ? 'spin' : ''} /></button>}
      </PageHeader>

      {lib.data.sources.length > 0 && (
        <div className="sources">
          {lib.data.sources.map((s) => (
            <span key={s} className="src-chip" title={s}><MdFolder size={16} />{base(s)}<button onClick={() => lib.removeSource(s)} aria-label="Remove"><MdClose size={14} /></button></span>
          ))}
        </div>
      )}

      {lib.local.tracks.length === 0 && !lib.local.scanning ? (
        <div className="dropzone">
          <MdLibraryMusic size={44} />
          <b>Bring your own music</b>
          <p className="muted">Add a folder, pick files, or just drop them anywhere in this window.<br />MP3, FLAC, M4A, WAV, OGG, OPUS · covers and tags are read automatically · a matching .lrc file gives you your own synced lyrics.</p>
        </div>
      ) : (
        <>
          <div className="toolbar">
            <label className="search small">
              <MdSearch size={20} />
              <input placeholder="Filter your music" value={filter} onChange={(e) => setFilter(e.target.value)} />
            </label>
            <span className="grow" />
            <button className="tonal" onClick={() => tracks.length && play(tracks[0], tracks)}><MdPlayArrow size={20} /> Play</button>
            <button className="tonal" onClick={() => { if (!shuffle) toggleShuffle(); tracks.length && play(tracks[Math.floor(Math.random() * tracks.length)], tracks); }}><MdShuffle size={20} /> Shuffle</button>
          </div>
          <div className="chip-row sortbar">
            <span className="muted small">Sort by</span>
            {Object.entries(SORTS).map(([k, v]) => <button key={k} className={`chip ${sort === k ? 'on' : ''}`} onClick={() => setSort(k)}>{v}</button>)}
          </div>
          <VirtualList items={rows}>
            {(r) => (r.header ? <GroupHeader row={r} onPlay={playGroup} /> : <TrackRow t={r.track} i={r.n} list={tracks} />)}
          </VirtualList>
        </>
      )}
    </div>
  );
}
