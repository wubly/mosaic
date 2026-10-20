'use client';
import { motion } from 'framer-motion';
import { MdPlayArrow, MdPause, MdMoreVert, MdFavorite, MdFavoriteBorder, MdFolder } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { useTrackMenu } from '@/components/tracks/TrackMenu';
import { useToast } from '@/components/ui/Toast';
import useTrackState from '@/components/tracks/useTrackState';
import Bars from '@/components/tracks/Bars';
import { fmt, artStyle } from '@/components/tracks/format';

const hidden = { hidden: { opacity: 0, x: -8 }, show: { opacity: 1, x: 0 } };

export default function TrackRow({ t, i, list, extra, showIndex = true }) {
  const { active, playing, click } = useTrackState(t);
  const openMenu = useTrackMenu();
  const lib = useData();
  const toast = useToast();
  const liked = lib.isLiked(t.id);

  const openAt = (e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    openMenu(t, rect.left - 180, rect.bottom, extra);
  };

  const toggleLike = () => toast(lib.toggleLike(t) ? 'Added to Liked songs' : 'Removed from Liked songs');

  return (
    <motion.li
      variants={hidden}
      className={`row-item ${active ? 'active' : ''}`}
      onContextMenu={(e) => { e.preventDefault(); openMenu(t, e.clientX, e.clientY, extra); }}
    >
      <button className="row-main" onClick={() => click(list)}>
        <span className="idx">
          <span className="num">{showIndex ? (active ? <Bars playing={playing} /> : i + 1) : null}</span>
          <span className="go">{playing ? <MdPause size={20} /> : <MdPlayArrow size={20} />}</span>
        </span>
        <span className="thumb sm" style={artStyle(t)} />
        <span className="txt">
          <span className="t">{t.title}</span>
          <span className="a">
            {t.source === 'local' && <MdFolder size={12} className="src" />}
            {t.artist}{t.album ? ` · ${t.album}` : ''}
          </span>
        </span>
      </button>

      <button className={`icon like ${liked ? 'on-like' : ''}`} title={liked ? 'Unlike' : 'Like'} onClick={toggleLike}>
        {liked ? <MdFavorite size={20} /> : <MdFavoriteBorder size={20} />}
      </button>
      <span className="d">{fmt(t.duration)}</span>
      <button className="icon" title="More" onClick={openAt}><MdMoreVert size={20} /></button>
    </motion.li>
  );
}
