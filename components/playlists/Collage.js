import { MdPlaylistPlay } from 'react-icons/md';

export default function Collage({ tracks }) {
  const covers = tracks.map((t) => t.thumb || t.artwork).filter(Boolean).slice(0, 4);

  return (
    <div className={`collage c${Math.max(1, covers.length)}`}>
      {covers.length === 0 && <MdPlaylistPlay size={40} />}
      {covers.map((src, i) => <span key={i} style={{ backgroundImage: `url(${src})` }} />)}
    </div>
  );
}
