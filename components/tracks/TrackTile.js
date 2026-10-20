'use client';
import { motion } from 'framer-motion';
import { MdPlayArrow, MdPause, MdMoreVert, MdFavorite } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { useTrackMenu } from '@/components/tracks/TrackMenu';
import useTrackState from '@/components/tracks/useTrackState';
import { fmt, artStyle } from '@/components/tracks/format';

const hidden = { hidden: { opacity: 0, y: 16 }, show: { opacity: 1, y: 0 } };

export default function TrackTile({ t, list, extra }) {
  const { active, playing, click } = useTrackState(t);
  const openMenu = useTrackMenu();
  const lib = useData();

  const openAt = (e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    openMenu(t, rect.left, rect.bottom, extra);
  };

  return (
    <motion.div
      variants={hidden}
      whileHover={{ y: -3 }}
      className={`tile ${active ? 'active' : ''}`}
      onContextMenu={(e) => { e.preventDefault(); openMenu(t, e.clientX, e.clientY, extra); }}
    >
      <button className="tile-main" onClick={() => click(list)}>
        <div className="art" style={artStyle(t)}>
          <span className="overlay">{playing ? <MdPause size={30} /> : <MdPlayArrow size={30} />}</span>
          <span className="dur">{fmt(t.duration)}</span>
        </div>
        <div className="title">{t.title}</div>
        <div className="artist">{t.artist}</div>
      </button>

      <button className="tile-more icon" title="More" onClick={openAt}><MdMoreVert size={20} /></button>
      {lib.isLiked(t.id) && <MdFavorite className="tile-heart" size={16} />}
    </motion.div>
  );
}
