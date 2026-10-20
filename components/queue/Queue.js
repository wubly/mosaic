'use client';
import { Reorder, useDragControls } from 'framer-motion';
import { MdDragIndicator, MdClose, MdClearAll, MdPlayArrow, MdPause } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import Bars from '@/components/tracks/Bars';
import PageHeader from '@/components/ui/PageHeader';
import { fmt } from '@/components/tracks/format';

function Item({ t, i, active, playing, onPlay, onRemove }) {
  const controls = useDragControls();
  return (
    <Reorder.Item value={t} dragListener={false} dragControls={controls} className={`row-item q ${active ? 'active' : ''}`} whileDrag={{ scale: 1.02, boxShadow: '0 8px 24px #0006', zIndex: 5 }}>
      <span className="grip" onPointerDown={(e) => controls.start(e)} title="Drag to reorder"><MdDragIndicator size={22} /></span>
      <button className="row-main" onClick={onPlay}>
        <span className="idx">
          <span className="num">{active ? <Bars playing={playing} /> : i + 1}</span>
          <span className="go">{playing ? <MdPause size={20} /> : <MdPlayArrow size={20} />}</span>
        </span>
        <span className="thumb sm" style={{ backgroundImage: (t.thumb || t.artwork) ? `url(${t.thumb || t.artwork})` : undefined }} />
        <span className="txt"><span className="t">{t.title}</span><span className="a">{t.artist}</span></span>
      </button>
      <span className="d">{fmt(t.duration)}</span>
      {active ? <span className="icon ph" /> : <button className="icon" title="Remove from queue" onClick={onRemove}><MdClose size={20} /></button>}
    </Reorder.Item>
  );
}

export default function Queue() {
  const { queue, playing, playIndex, toggle, removeFromQueue, reorderQueue, clearQueue } = usePlayer();
  const upcoming = Math.max(0, queue.list.length - queue.index - 1);

  const WINDOW = 150;
  const begin = Math.max(0, Math.min(queue.index - 8, queue.list.length - WINDOW));
  const shown = queue.list.slice(begin, begin + WINDOW);
  const after = Math.max(0, queue.list.length - begin - WINDOW);
  const onReorder = (next) => reorderQueue([...queue.list.slice(0, begin), ...next, ...queue.list.slice(begin + WINDOW)]);

  return (
    <div className="library">
      <PageHeader title="Queue" subtitle={queue.list.length ? `${queue.list.length} tracks · ${upcoming} up next` : undefined}>
        {queue.list.length > 1 && <button className="tonal" onClick={clearQueue}><MdClearAll size={20} /> Clear queue</button>}
      </PageHeader>

      {queue.list.length === 0 && <p className="muted">Nothing queued. Play a track, or right-click one and choose “Add to queue”.</p>}

      {begin > 0 && <p className="muted queue-more">{begin} earlier tracks not shown</p>}
      <Reorder.Group axis="y" values={shown} onReorder={onReorder} className="rows">
        {shown.map((t, k) => {
          const i = begin + k;
          return <Item key={t.id} t={t} i={i} active={i === queue.index} playing={i === queue.index && playing} onPlay={() => (i === queue.index ? toggle() : playIndex(i))} onRemove={() => removeFromQueue(i)} />;
        })}
      </Reorder.Group>
      {after > 0 && <p className="muted queue-more">{after} more tracks after these</p>}
    </div>
  );
}
