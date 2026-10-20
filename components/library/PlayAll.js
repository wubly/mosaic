'use client';
import { MdPlayArrow, MdShuffle } from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';

export default function PlayAll({ list }) {
  const { play, toggleShuffle, shuffle } = usePlayer();
  if (!list.length) return null;

  const playRandom = () => {
    if (!shuffle) toggleShuffle();
    play(list[Math.floor(Math.random() * list.length)], list);
  };

  return (
    <>
      <button className="primary" onClick={() => play(list[0], list)}><MdPlayArrow size={20} /> Play</button>
      <button className="tonal" onClick={playRandom}><MdShuffle size={20} /> Shuffle</button>
    </>
  );
}
