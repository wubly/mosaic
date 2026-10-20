'use client';
import { useCallback, useRef, useState } from 'react';

const EMPTY = { list: [], index: -1 };

export default function useQueueState() {
  const queueRef = useRef(EMPTY);
  const [queue, setQueueState] = useState(EMPTY);

  const setQueue = useCallback((next) => {
    queueRef.current = next;
    setQueueState(next);
  }, []);

  return { queue, queueRef, setQueue };
}
