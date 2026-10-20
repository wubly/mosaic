'use client';
import { motion, AnimatePresence } from 'framer-motion';
import { MdVolumeUp, MdVolumeDown, MdVolumeOff, MdOutlineLyrics, MdLyrics, MdFavorite, MdFavoriteBorder, MdOpenInFull, MdPictureInPictureAlt, MdMusicNote, MdSkipPrevious, MdSkipNext, MdPlayArrow } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import { useTrackMenu } from '@/components/tracks/TrackMenu';
import Controls from '@/components/player/Controls';
import WavySlider from '@/components/player/WavySlider';
import TunePopover from '@/components/player/TunePopover';

export default function PlayerBar({ lyricsOpen, onToggleLyrics, onOpenNowPlaying, onToggleMini }) {
  const { track, error, volume, setVolume } = usePlayer();
  const lib = useData();
  const toast = useToast();
  const openMenu = useTrackMenu();

  if (!track) {
    return (
      <footer className="glass bar">
        <div className="now">
          <div className="thumb idle-thumb"><MdMusicNote size={24} /></div>
          <div className="meta">
            <div className="title">Nothing playing</div>
            <div className="artist">Pick a track to get started</div>
          </div>
        </div>
        <div className="center idle-center">
          <div className="transport">
            <button className="icon skip" disabled><MdSkipPrevious size={30} /></button>
            <button className="play" disabled><MdPlayArrow size={32} /></button>
            <button className="icon skip" disabled><MdSkipNext size={30} /></button>
          </div>
          <div className="seek"><span>0:00</span><WavySlider label="Seek" wavy={false} value={0} max={1} onChange={() => {}} /><span>0:00</span></div>
        </div>
        <div className="tools">
          <div className="volume">
            <button className="icon" title={volume === 0 ? 'Unmute' : 'Mute'} onClick={() => setVolume(volume === 0 ? 0.8 : 0)}>{volume === 0 ? <MdVolumeOff size={22} /> : <MdVolumeUp size={22} />}</button>
            <WavySlider label="Volume" wavy={false} value={volume} max={1} onChange={setVolume} />
          </div>
        </div>
      </footer>
    );
  }

  const liked = lib.isLiked(track.id);
  const VolIcon = volume === 0 ? MdVolumeOff : volume < 0.5 ? MdVolumeDown : MdVolumeUp;

  return (
    <motion.footer className="glass bar" initial={{ y: 40, opacity: 0 }} animate={{ y: 0, opacity: 1 }} transition={{ type: 'spring', stiffness: 260, damping: 26 }}>
      <div className="now" onContextMenu={(e) => { e.preventDefault(); openMenu(track, e.clientX, e.clientY - 280); }}>
        <button className="thumb-btn" title="Open Now Playing (F)" onClick={onOpenNowPlaying}>
          <AnimatePresence mode="popLayout" initial={false}>
            <motion.div key={track.id} className="thumb" initial={{ scale: 0.85, opacity: 0 }} animate={{ scale: 1, opacity: 1 }} exit={{ scale: 0.85, opacity: 0 }} transition={{ duration: 0.2 }} style={{ backgroundImage: (track.thumb || track.artwork) ? `url(${track.thumb || track.artwork})` : undefined }} />
          </AnimatePresence>
          <span className="thumb-hover"><MdOpenInFull size={20} /></span>
        </button>
        <div className="meta">
          <div className="title">{track.title}</div>
          <div className="artist">{error ? <span className="err">{error}</span> : track.artist}</div>
        </div>
        <button className={`icon like ${liked ? 'on-like' : ''}`} title={liked ? 'Unlike (L)' : 'Like (L)'} onClick={() => toast(lib.toggleLike(track) ? 'Added to Liked songs' : 'Removed from Liked songs')}>
          {liked ? <MdFavorite size={22} /> : <MdFavoriteBorder size={22} />}
        </button>
      </div>

      <Controls />

      <div className="tools">
        <TunePopover />
        <button className="icon" title="Mini player (I)" onClick={onToggleMini}><MdPictureInPictureAlt size={20} /></button>
        <button className={`icon ${lyricsOpen ? 'on' : ''}`} title="Lyrics (Y)" onClick={onToggleLyrics}>
          {lyricsOpen ? <MdLyrics size={22} /> : <MdOutlineLyrics size={22} />}
        </button>
        <div className="volume">
          <button className="icon" title={volume === 0 ? 'Unmute (M)' : 'Mute (M)'} onClick={() => setVolume(volume === 0 ? 0.8 : 0)}><VolIcon size={22} /></button>
          <WavySlider label="Volume" wavy={false} value={volume} max={1} onChange={setVolume} />
        </div>
      </div>
    </motion.footer>
  );
}
