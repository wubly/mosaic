'use client';
import { motion } from 'framer-motion';
import { MdHistory, MdDelete } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Empty from '@/components/ui/Empty';
import TrackRow from '@/components/tracks/TrackRow';
import { listVariants } from '@/components/tracks/variants';
import { ago } from '@/components/library/time';

export default function HistoryList() {
  const lib = useData();
  const history = lib.data.history;
  const tracks = history.map((entry) => entry.track);

  if (history.length === 0) {
    return <Empty icon={<MdHistory size={32} />} title="Nothing played yet" hint="Tracks you listen to for a few seconds show up here." />;
  }

  return (
    <>
      <div className="page-actions left tight">
        <button className="tonal" onClick={lib.clearHistory}><MdDelete size={20} /> Clear history</button>
      </div>
      <motion.ol className="rows" initial="hidden" animate="show" variants={listVariants}>
        {history.map((entry, i) => (
          <div className="hist" key={entry.at + entry.track.id}>
            <TrackRow t={entry.track} i={i} list={tracks} showIndex={false} />
            <span className="when">{ago(entry.at)}</span>
          </div>
        ))}
      </motion.ol>
    </>
  );
}
