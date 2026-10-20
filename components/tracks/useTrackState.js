'use client';
import { usePlayer } from '@/components/player/PlayerProvider';

export default function useTrackState(track) {
  const { track: current, playing, play, toggle } = usePlayer();
  const active = current?.id === track.id;

  return {
    active,
    playing: active && playing,
    click: (list) => (active ? toggle() : play(track, list)),
  };
}
