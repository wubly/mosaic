'use client';
import { MdFavorite } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Empty from '@/components/ui/Empty';
import VirtualList from '@/components/ui/VirtualList';
import TrackRow from '@/components/tracks/TrackRow';
import PlayAll from '@/components/library/PlayAll';

export default function LikedList() {
  const { data } = useData();
  const likes = data.likes;

  if (likes.length === 0) {
    return <Empty icon={<MdFavorite size={32} />} title="No liked songs yet" hint="Tap the heart on any track and it will land here." />;
  }

  return (
    <>
      <div className="page-actions left tight"><PlayAll list={likes} /></div>
      <VirtualList items={likes}>{(t, i) => <TrackRow t={t} i={i} list={likes} />}</VirtualList>
    </>
  );
}
