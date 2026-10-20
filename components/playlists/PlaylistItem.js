'use client';
import { Reorder, useDragControls } from 'framer-motion';
import { MdPlayArrow, MdPause, MdDelete, MdDragIndicator, MdMoreVert } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import { useTrackMenu } from '@/components/tracks/TrackMenu';
import Bars from '@/components/tracks/Bars';
import { fmt, artStyle } from '@/components/tracks/format';

export default function PlaylistItem({ t, i, list, plId }) {
  const controls = useDragControls();
  const openMenu = useTrackMenu();
  const lib = useData();
  const { track, playing, play, toggle } = usePlayer();
  const active = track?.id === t.id;

  const extra = [{ label: 'Remove from playlist', icon: <MdDelete size={20} />, run: () => lib.removeFromPlaylist(plId, t.id) }];

  const openAt = (e) => {
    const rect = e.currentTarget.getBoundingClientRect();
    openMenu(t, rect.left - 200, rect.bottom, extra);
  };

  return (
    <Reorder.Item
      value={t}
      dragListener={false}
      dragControls={controls}
      className={`row-item q ${active ? 'active' : ''}`}
      whileDrag={{ scale: 1.02, boxShadow: '0 8px 24px #0006', zIndex: 5 }}
      onContextMenu={(e) => { e.preventDefault(); openMenu(t, e.clientX, e.clientY, extra); }}
    >
      <span className="grip" onPointerDown={(e) => controls.start(e)} title="Drag to reorder"><MdDragIndicator size={22} /></span>
      <button className="row-main" onClick={() => (active ? toggle() : play(t, list))}>
        <span className="idx">
          <span className="num">{active ? <Bars playing={playing} /> : i + 1}</span>
          <span className="go">{active && playing ? <MdPause size={20} /> : <MdPlayArrow size={20} />}</span>
        </span>
        <span className="thumb sm" style={artStyle(t)} />
        <span className="txt"><span className="t">{t.title}</span><span className="a">{t.artist}</span></span>
      </button>
      <span className="d">{fmt(t.duration)}</span>
      <button className="icon" title="More" onClick={openAt}><MdMoreVert size={20} /></button>
    </Reorder.Item>
  );
}
