import {
  MdExplore, MdQueueMusic, MdSettings, MdLibraryMusic, MdFolder, MdOutlineLyrics, MdShuffle, MdFullscreen,
  MdFavoriteBorder, MdBedtime, MdGraphicEq, MdPictureInPictureAlt, MdPlaylistPlay, MdCloudDownload,
} from 'react-icons/md';

const icon = (Glyph) => <Glyph size={20} />;

export default function buildPaletteActions({ go, lib, player, track, toast, lyricsOpen, actions }) {
  return [
    { id: 'g-discover', label: 'Go to Discover', icon: icon(MdExplore), run: () => go('discover') },
    { id: 'g-library', label: 'Go to Library', icon: icon(MdLibraryMusic), run: () => go('library') },
    { id: 'g-playlists', label: 'Go to Playlists', icon: icon(MdPlaylistPlay), run: () => go('playlists') },
    { id: 'import', label: 'Import a Spotify playlist', icon: icon(MdCloudDownload), run: actions.importPlaylist },
    { id: 'g-local', label: 'Go to On your device', icon: icon(MdFolder), run: () => go('local') },
    { id: 'g-queue', label: 'Go to Queue', icon: icon(MdQueueMusic), run: () => go('queue') },
    { id: 'g-settings', label: 'Open Settings', icon: icon(MdSettings), run: () => go('settings') },
    { id: 'np', label: 'Open immersive Now Playing', icon: icon(MdFullscreen), run: () => track && actions.openNowPlaying() },
    { id: 'mini', label: 'Toggle mini player', icon: icon(MdPictureInPictureAlt), run: actions.toggleMini },
    { id: 'lyrics', label: 'Toggle lyrics panel', icon: icon(MdOutlineLyrics), run: () => lib.setSetting('lyricsOpen', !lyricsOpen) },
    { id: 'shuffle', label: 'Toggle shuffle', icon: icon(MdShuffle), run: () => player.toggleShuffle() },
    { id: 'like', label: 'Like current track', icon: icon(MdFavoriteBorder), run: () => track && toast(lib.toggleLike(track) ? 'Added to Liked songs' : 'Removed from Liked songs') },
    { id: 'eq', label: lib.settings.eqOn ? 'Turn equalizer off' : 'Turn equalizer on', icon: icon(MdGraphicEq), run: () => lib.setSetting('eqOn', !lib.settings.eqOn) },
    { id: 'sleep', label: 'Sleep timer: 30 minutes', icon: icon(MdBedtime), run: () => player.setSleep('timer', 30) },
  ];
}
