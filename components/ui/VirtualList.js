'use client';
import { useEffect, useRef, useState } from 'react';

export default function VirtualList({ items, rowHeight = 54, overscan = 8, children }) {
  const host = useRef(null);
  const [win, setWin] = useState({ start: 0, end: 24 });

  useEffect(() => {
    const el = host.current;
    const scroller = el?.closest('.library');
    if (!el || !scroller) return;
    let raf = 0;
    const calc = () => {
      raf = 0;
      const view = scroller.getBoundingClientRect();
      const past = view.top - el.getBoundingClientRect().top;
      const start = Math.max(0, Math.floor(past / rowHeight) - overscan);
      const end = Math.min(items.length, Math.ceil((past + view.height) / rowHeight) + overscan);
      setWin((w) => (w.start === start && w.end === end ? w : { start, end }));
    };
    const schedule = () => { if (!raf) raf = requestAnimationFrame(calc); };
    calc();
    scroller.addEventListener('scroll', schedule, { passive: true });
    const ro = new ResizeObserver(schedule);
    ro.observe(scroller);
    return () => {
      scroller.removeEventListener('scroll', schedule);
      ro.disconnect();
      cancelAnimationFrame(raf);
    };
  }, [items.length, rowHeight, overscan]);

  return (
    <div ref={host} className="vlist" style={{ height: items.length * rowHeight }}>
      {items.slice(win.start, win.end).map((item, k) => (
        <div key={item.id ?? win.start + k} className="vrow" style={{ top: (win.start + k) * rowHeight, height: rowHeight }}>
          {children(item, win.start + k)}
        </div>
      ))}
    </div>
  );
}
