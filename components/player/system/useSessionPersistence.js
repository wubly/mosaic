'use client';
import { useCallback, useEffect, useRef } from 'react';
import { useData } from '@/components/data/DataProvider';

const WINDOW_BEFORE = 20;
const WINDOW_SIZE = 300;
const POSITION_EVERY_MS = 4000;

export default function useSessionPersistence({ queue, restore }) {
  const { session, saveSession } = useData();
  const lastPositionSave = useRef(0);
  const restored = useRef(false);

  useEffect(() => {
    if (!queue.list.length) return;
    const from = Math.max(0, queue.index - WINDOW_BEFORE);
    saveSession({ queue: queue.list.slice(from, from + WINDOW_SIZE), index: queue.index - from });
  }, [queue]);

  useEffect(() => {
    if (restored.current) return;
    restored.current = true;
    if (!session?.queue?.length) return;

    const index = Math.min(session.index ?? 0, session.queue.length - 1);
    restore({ list: session.queue, index, position: session.position || 0 });
  }, []);

  return useCallback((position) => {
    if (Date.now() - lastPositionSave.current < POSITION_EVERY_MS) return;
    lastPositionSave.current = Date.now();
    saveSession({ position });
  }, [saveSession]);
}
