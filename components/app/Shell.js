'use client';
import { useEffect, useMemo, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import Logo from '@/components/ui/Logo';
import useDynamicTheme from '@/components/theme/useDynamicTheme';
import Discover from '@/components/library/Discover';
import LibraryView from '@/components/library/LibraryView';
import LocalView from '@/components/library/LocalView';
import SettingsView from '@/components/settings/SettingsView';
import Queue from '@/components/queue/Queue';
import LyricsPanel from '@/components/lyrics/LyricsPanel';
import PlayerBar from '@/components/player/PlayerBar';
import NowPlaying from '@/components/player/NowPlaying';
import MiniPlayer from '@/components/player/MiniPlayer';
import ImportDialog from '@/components/playlists/ImportDialog';
import CommandPalette from '@/components/app/CommandPalette';
import Ambient from '@/components/app/Ambient';
import SpotifyView from '@/components/spotify/SpotifyView';
import { useSpotify } from '@/components/spotify/SpotifyProvider';
import Rail from '@/components/app/Rail';
import Resizer from '@/components/app/Resizer';
import DropVeil from '@/components/app/DropVeil';
import usePanelSizes from '@/components/app/usePanelSizes';
import useShortcuts from '@/components/app/useShortcuts';
import useFileDrop from '@/components/app/useFileDrop';
import buildPaletteActions from '@/components/app/paletteActions';

export const macInset = typeof window !== 'undefined' && window.mosaic?.platform === 'darwin' ? { paddingLeft: 92 } : undefined;

const Titlebar = () => <div className="titlebar" style={macInset}><Logo size={16} className="tb" /></div>;

export default function Shell({ name, onReset }) {
  const player = usePlayer();
  const { queue, track } = player;
  const lib = useData();
  const toast = useToast();
  const spotify = useSpotify();

  const [view, setView] = useState('discover');
  const [nowPlaying, setNowPlaying] = useState(false);
  const [palette, setPalette] = useState(false);
  const [searchRequest, setSearchRequest] = useState(null);
  const [mini, setMini] = useState(false);
  const [importing, setImporting] = useState(false);
  const [libraryTarget, setLibraryTarget] = useState(null);
  const lyricsOpen = lib.settings.lyricsOpen;

  useDynamicTheme(track?.thumb || track?.artwork);
  const sizes = usePanelSizes();

  useEffect(() => { window.mosaic.setMini(mini); }, [mini]);
  useEffect(() => { spotify.registerNav(() => { setView('spotify'); setNowPlaying(false); }); }, [spotify.registerNav]);
  useEffect(() => { if (view === 'spotify' && !spotify.enabled) setView('discover'); }, [view, spotify.enabled]);

  const toggleMini = () => setMini((value) => !value);

  const search = (q = '') => {
    setView('discover');
    setNowPlaying(false);
    setSearchRequest({ q, n: Date.now() });
  };

  const actions = {
    togglePalette: () => setPalette((value) => !value),
    toggleMini,
    toggleNowPlaying: () => setNowPlaying((value) => !value),
    openNowPlaying: () => setNowPlaying(true),
    importPlaylist: () => setImporting(true),
    search,
  };

  useShortcuts({ player, lib, toast, track, lyricsOpen, paletteOpen: palette, actions });

  const dragging = useFileDrop((paths) => {
    lib.addSources(paths);
    setView('local');
    toast(`Adding ${paths.length} item${paths.length > 1 ? 's' : ''} to your library`);
  });

  const paletteActions = useMemo(
    () => buildPaletteActions({ go: setView, lib, player, track, toast, lyricsOpen, actions }),
    [track, lyricsOpen, lib.settings.eqOn],
  );

  const searchable = useMemo(
    () => [...queue.list, ...lib.data.likes, ...lib.data.history.map((h) => h.track), ...lib.local.tracks],
    [queue.list, lib.data.likes, lib.data.history, lib.local.tracks],
  );

  if (mini) {
    return (
      <div className="app-mini">
        <Ambient />
        <Titlebar />
        <MiniPlayer onExpand={toggleMini} />
      </div>
    );
  }

  return (
    <div className="app">
      <Ambient />
      <Titlebar />
      <Rail view={view} onChange={setView} queueCount={queue.list.length} spotify={spotify.enabled} />

      <main className="glass main">
        <Discover name={name} hidden={view !== 'discover'} request={searchRequest} />
        {view === 'library' && <LibraryView key="library" initialTab="liked" onImport={actions.importPlaylist} />}
        {view === 'playlists' && <LibraryView key="playlists" initialTab="playlists" target={libraryTarget} onImport={actions.importPlaylist} />}
        {view === 'local' && <LocalView />}
        {view === 'queue' && <Queue />}
        {view === 'spotify' && <SpotifyView />}
        {view === 'settings' && <SettingsView onReset={onReset} />}
      </main>

      <AnimatePresence initial={false}>
        {lyricsOpen && (
          <motion.aside
            key="lyrics"
            className="glass side"
            initial={{ opacity: 0, x: 32 }}
            animate={{ opacity: 1, x: 0 }}
            exit={{ opacity: 0, x: 32 }}
            transition={{ type: 'spring', stiffness: 260, damping: 30 }}
          >
            <LyricsPanel />
          </motion.aside>
        )}
      </AnimatePresence>

      {lyricsOpen && <Resizer className="rz-v" label="Resize lyrics panel" onDrag={sizes.dragSide} onCommit={sizes.commit} onReset={sizes.resetSide} />}
      <Resizer className="rz-h" label="Resize player bar" onDrag={sizes.dragBar} onCommit={sizes.commit} onReset={sizes.resetBar} />

      <PlayerBar
        lyricsOpen={lyricsOpen}
        onToggleLyrics={() => lib.setSetting('lyricsOpen', !lyricsOpen)}
        onOpenNowPlaying={actions.openNowPlaying}
        onToggleMini={toggleMini}
      />

      <AnimatePresence>{nowPlaying && track && <NowPlaying onClose={() => setNowPlaying(false)} />}</AnimatePresence>

      <AnimatePresence>
        {importing && (
          <ImportDialog
            onClose={() => setImporting(false)}
            onCreated={(id) => { setLibraryTarget({ open: id, n: Date.now() }); setView('playlists'); }}
          />
        )}
      </AnimatePresence>

      <CommandPalette
        open={palette}
        onClose={() => setPalette(false)}
        actions={paletteActions}
        tracks={searchable}
        onSearch={search}
        onPlay={(t) => player.play(t)}
      />

      <DropVeil visible={dragging} />
    </div>
  );
}
