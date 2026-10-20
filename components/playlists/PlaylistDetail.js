'use client';
import { useState } from 'react';
import { motion, Reorder } from 'framer-motion';
import { MdDelete, MdArrowBack, MdEdit, MdPlaylistPlay, MdQueueMusic } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import Empty from '@/components/ui/Empty';
import VirtualList from '@/components/ui/VirtualList';
import TrackRow from '@/components/tracks/TrackRow';
import PlayAll from '@/components/library/PlayAll';
import Collage from '@/components/playlists/Collage';
import PlaylistItem from '@/components/playlists/PlaylistItem';
import { hm } from '@/components/library/time';

const MAX_REORDERABLE = 250;

export default function PlaylistDetail({ id, onBack }) {
  const lib = useData();
  const player = usePlayer();
  const toast = useToast();
  const playlist = lib.data.playlists.find((p) => p.id === id);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(playlist?.name ?? '');
  if (!playlist) return null;

  const rename = () => { lib.renamePlaylist(id, name); setEditing(false); };
  const remove = () => { lib.deletePlaylist(id); toast('Playlist deleted'); onBack(); };
  const total = playlist.tracks.reduce((sum, t) => sum + (t.duration || 0), 0) / 1000;
  const windowed = playlist.tracks.length > MAX_REORDERABLE;
  const removeAction = (t) => [{ label: 'Remove from playlist', icon: <MdDelete size={20} />, run: () => lib.removeFromPlaylist(id, t.id) }];

  return (
    <>
      <button className="ghost back" onClick={onBack}><MdArrowBack size={20} /> Playlists</button>

      <div className="pl-head">
        <Collage tracks={playlist.tracks} />
        <div className="pl-info">
          {editing ? (
            <input className="pl-name" autoFocus value={name} onChange={(e) => setName(e.target.value)} onBlur={rename} onKeyDown={(e) => e.key === 'Enter' && rename()} />
          ) : (
            <h2>{playlist.name}</h2>
          )}
          <p className="muted">{playlist.tracks.length} tracks{total > 0 ? ` · ${hm(total)}` : ''}</p>
          <div className="page-actions left">
            <PlayAll list={playlist.tracks} />
            {playlist.tracks.length > 0 && <button className="tonal" onClick={() => player.enqueueMany(playlist.tracks)}><MdQueueMusic size={20} /> Add to queue</button>}
            <button className="icon" title="Rename" onClick={() => { setName(playlist.name); setEditing(true); }}><MdEdit size={20} /></button>
            <button className="icon" title="Delete playlist" onClick={remove}><MdDelete size={20} /></button>
          </div>
        </div>
      </div>

      {playlist.tracks.length === 0 ? (
        <Empty icon={<MdPlaylistPlay size={32} />} title="This playlist is empty" hint="Right-click any track and choose “Add to playlist”, or import one from Spotify." />
      ) : windowed ? (
        <VirtualList items={playlist.tracks}>{(t, i) => <TrackRow t={t} i={i} list={playlist.tracks} extra={removeAction(t)} />}</VirtualList>
      ) : (
        <Reorder.Group axis="y" values={playlist.tracks} onReorder={(next) => lib.reorderPlaylist(id, next)} className="rows">
          {playlist.tracks.map((t, i) => <PlaylistItem key={t.id} t={t} i={i} list={playlist.tracks} plId={id} />)}
        </Reorder.Group>
      )}
    </>
  );
}
