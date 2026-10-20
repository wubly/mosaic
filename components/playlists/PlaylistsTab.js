'use client';
import { MdAdd, MdPlaylistAdd, MdCloudDownload } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Empty from '@/components/ui/Empty';
import Collage from '@/components/playlists/Collage';
import PlaylistDetail from '@/components/playlists/PlaylistDetail';

export default function PlaylistsTab({ open, setOpen, onImport }) {
  const lib = useData();
  const create = () => setOpen(lib.createPlaylist('New playlist'));

  if (open) return <PlaylistDetail id={open} onBack={() => setOpen(null)} />;

  if (lib.data.playlists.length === 0) {
    return (
      <Empty icon={<MdPlaylistAdd size={32} />} title="No playlists yet" hint="Create one, add tracks from any track’s menu, or bring a playlist over from Spotify.">
        <div className="page-actions" style={{ marginTop: 16 }}>
          <button className="tonal" onClick={onImport}><MdCloudDownload size={20} /> Import from Spotify</button>
          <button className="primary" onClick={create}><MdAdd size={20} /> New playlist</button>
        </div>
      </Empty>
    );
  }

  return (
    <div className="pl-grid">
      {lib.data.playlists.map((p) => (
        <button key={p.id} className="pl-card" onClick={() => setOpen(p.id)}>
          <Collage tracks={p.tracks} />
          <b>{p.name}</b>
          <span className="muted">{p.tracks.length} tracks</span>
        </button>
      ))}
    </div>
  );
}
