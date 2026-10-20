'use client';
import { motion } from 'framer-motion';
import {
  MdExplore, MdOutlineExplore, MdQueueMusic, MdOutlineQueueMusic, MdOutlineSettings, MdSettings,
  MdLibraryMusic, MdOutlineLibraryMusic, MdFolder, MdFolderOpen, MdPlaylistPlay, MdOutlinePlaylistPlay,
} from 'react-icons/md';

const TOP = [
  { id: 'discover', label: 'Discover', icon: MdOutlineExplore, activeIcon: MdExplore },
  { id: 'library', label: 'Library', icon: MdOutlineLibraryMusic, activeIcon: MdLibraryMusic },
  { id: 'playlists', label: 'Playlists', icon: MdOutlinePlaylistPlay, activeIcon: MdPlaylistPlay },
  { id: 'local', label: 'Device', icon: MdFolder, activeIcon: MdFolderOpen },
  { id: 'queue', label: 'Queue', icon: MdOutlineQueueMusic, activeIcon: MdQueueMusic },
];
const BOTTOM = [{ id: 'settings', label: 'Settings', icon: MdOutlineSettings, activeIcon: MdSettings }];

function RailItem({ item, active, badge, onClick }) {
  const Glyph = active ? item.activeIcon : item.icon;

  return (
    <button className={`rail-item ${active ? 'active' : ''}`} onClick={onClick} aria-current={active ? 'page' : undefined}>
      <span className="pill">
        {active && <motion.span layoutId="rail-pill" className="pill-bg" transition={{ type: 'spring', stiffness: 500, damping: 36 }} />}
        <Glyph size={24} />
        {badge > 0 && <span className="badge">{badge > 99 ? '99+' : badge}</span>}
      </span>
      <span className="lbl">{item.label}</span>
    </button>
  );
}

export default function Rail({ view, onChange, queueCount }) {
  const render = (item) => (
    <RailItem key={item.id} item={item} active={view === item.id} badge={item.id === 'queue' ? queueCount : 0} onClick={() => onChange(item.id)} />
  );

  return (
    <nav className="rail">
      {TOP.map(render)}
      <span className="grow" />
      {BOTTOM.map(render)}
    </nav>
  );
}
