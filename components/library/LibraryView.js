'use client';
import { useEffect, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { MdAdd, MdCloudDownload } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import PageHeader from '@/components/ui/PageHeader';
import LikedList from '@/components/library/LikedList';
import HistoryList from '@/components/library/HistoryList';
import Stats from '@/components/library/Stats';
import PlaylistsTab from '@/components/playlists/PlaylistsTab';

const TABS = [['liked', 'Liked'], ['playlists', 'Playlists'], ['history', 'History'], ['stats', 'Stats']];

function Tabs({ tab, onChange }) {
  return (
    <div className="tabs wide">
      {TABS.map(([id, label]) => (
        <button key={id} className={tab === id ? 'on' : ''} onClick={() => onChange(id)}>
          <span className="tab-label">
            {label}
            {tab === id && <motion.span layoutId="lib-tab" className="tab-ind" />}
          </span>
        </button>
      ))}
    </div>
  );
}

export default function LibraryView({ initialTab = 'liked', target, onImport }) {
  const lib = useData();
  const [tab, setTab] = useState(initialTab);
  const [open, setOpen] = useState(null);
  const playlistsOnly = initialTab === 'playlists';
  const count = lib.data.playlists.length;

  useEffect(() => {
    if (target) {
      setTab('playlists');
      setOpen(target.open || null);
    }
  }, [target]);

  const subtitle = playlistsOnly ? `${count} playlists` : `${lib.data.likes.length} liked · ${count} playlists`;

  return (
    <div className="library">
      <PageHeader title={playlistsOnly ? 'Playlists' : 'Your library'} subtitle={subtitle}>
        {playlistsOnly && !open && (
          <>
            <button className="tonal" onClick={onImport}><MdCloudDownload size={20} /> Import from Spotify</button>
            <button className="primary" onClick={() => setOpen(lib.createPlaylist('New playlist'))}><MdAdd size={20} /> New playlist</button>
          </>
        )}
      </PageHeader>

      {!playlistsOnly && <Tabs tab={tab} onChange={(id) => { setTab(id); setOpen(null); }} />}

      <AnimatePresence mode="wait" initial={false}>
        <motion.div key={tab + (open ?? '')} initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0 }} transition={{ duration: 0.15 }}>
          {tab === 'liked' && <LikedList />}
          {tab === 'playlists' && <PlaylistsTab open={open} setOpen={setOpen} onImport={onImport} />}
          {tab === 'history' && <HistoryList />}
          {tab === 'stats' && <Stats />}
        </motion.div>
      </AnimatePresence>
    </div>
  );
}
