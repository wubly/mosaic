'use client';
import { useCallback } from 'react';

export default function useQueueOps({ queueRef, setQueue, play, toast }) {
  const addToQueue = useCallback((track) => {
    const queue = queueRef.current;
    if (!queue.list.length) return play(track);
    if (queue.list.some((t) => t.id === track.id)) return toast('Already in your queue');
    setQueue({ ...queue, list: [...queue.list, track] });
    toast('Added to queue');
  }, [queueRef, setQueue, play, toast]);

  const enqueueMany = useCallback((tracks) => {
    const queue = queueRef.current;
    if (!tracks.length) return;
    if (!queue.list.length) return play(tracks[0], tracks);

    const have = new Set(queue.list.map((t) => t.id));
    const fresh = tracks.filter((t) => !have.has(t.id));
    if (!fresh.length) return toast('Already in your queue');

    setQueue({ ...queue, list: [...queue.list, ...fresh] });
    toast(`Added ${fresh.length} track${fresh.length > 1 ? 's' : ''} to the queue`);
  }, [queueRef, setQueue, play, toast]);

  const playNext = useCallback((track) => {
    const queue = queueRef.current;
    if (!queue.list.length) return play(track);

    const without = queue.list.filter((t) => t.id !== track.id);
    const current = queue.list[queue.index];
    const insertAt = Math.max(0, without.findIndex((t) => t.id === current?.id)) + 1;
    without.splice(insertAt, 0, track);

    setQueue({ list: without, index: without.findIndex((t) => t.id === current?.id) });
    toast('Playing next');
  }, [queueRef, setQueue, play, toast]);

  const removeFromQueue = useCallback((index) => {
    const queue = queueRef.current;
    if (index === queue.index) return;
    setQueue({ list: queue.list.filter((_, i) => i !== index), index: index < queue.index ? queue.index - 1 : queue.index });
  }, [queueRef, setQueue]);

  const reorderQueue = useCallback((list) => {
    const queue = queueRef.current;
    const current = queue.list[queue.index];
    setQueue({ list, index: Math.max(0, list.indexOf(current)) });
  }, [queueRef, setQueue]);

  const clearQueue = useCallback(() => {
    const queue = queueRef.current;
    const current = queue.list[queue.index];
    setQueue({ list: current ? [current] : [], index: current ? 0 : -1 });
  }, [queueRef, setQueue]);

  const startRadio = useCallback(async (track) => {
    toast(`Starting radio from “${track.title}”`);
    try {
      const related = track.source !== 'soundcloud' ? [] : await window.mosaic.related(track.id);
      play(track, [track, ...related.filter((t) => t.id !== track.id)]);
      if (!related.length) toast('No similar tracks found');
    } catch {
      toast('Could not start radio');
    }
  }, [play, toast]);

  return { addToQueue, enqueueMany, playNext, removeFromQueue, reorderQueue, clearQueue, startRadio };
}
