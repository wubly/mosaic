'use client';
import { useEffect, useState } from 'react';

const hasFiles = (e) => [...(e.dataTransfer?.types || [])].includes('Files');

export default function useFileDrop(onPaths) {
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    let depth = 0;

    const enter = (e) => {
      if (!hasFiles(e)) return;
      depth++;
      setDragging(true);
    };
    const leave = (e) => {
      if (hasFiles(e) && --depth <= 0) {
        depth = 0;
        setDragging(false);
      }
    };
    const over = (e) => hasFiles(e) && e.preventDefault();
    const drop = (e) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      depth = 0;
      setDragging(false);
      const paths = window.mosaic.pathsForFiles(Array.from(e.dataTransfer.files));
      if (paths.length) onPaths(paths);
    };

    window.addEventListener('dragenter', enter);
    window.addEventListener('dragleave', leave);
    window.addEventListener('dragover', over);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragenter', enter);
      window.removeEventListener('dragleave', leave);
      window.removeEventListener('dragover', over);
      window.removeEventListener('drop', drop);
    };
  });

  return dragging;
}
