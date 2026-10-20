'use client';
import { createContext, useCallback, useContext, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import {
  MdPlayArrow, MdQueueMusic, MdFavorite, MdFavoriteBorder, MdPlaylistAdd, MdSensors, MdOpenInNew, MdPlaylistPlay, MdAdd, MdChevronRight, MdLink,
} from 'react-icons/md';
import { usePlayer } from '@/components/player/PlayerProvider';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';

const Ctx = createContext(() => {});
export const useTrackMenu = () => useContext(Ctx);

export default function TrackMenuProvider({ children }) {
  const [menu, setMenu] = useState(null);
  const [sub, setSub] = useState(false);
  const [pos, setPos] = useState({ left: 0, top: 0 });
  const ref = useRef(null);
  const player = usePlayer();
  const lib = useData();
  const toast = useToast();

  const open = useCallback((track, x, y, extra = []) => {
    setSub(false);
    setMenu({ track, x, y, extra });
  }, []);
  const close = () => setMenu(null);

  useLayoutEffect(() => {
    if (!menu || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      left: Math.max(8, Math.min(menu.x, window.innerWidth - r.width - 8)),
      top: Math.max(8, Math.min(menu.y, window.innerHeight - r.height - 8)),
    });
  }, [menu, sub]);

  useEffect(() => {
    if (!menu) return;
    const onDown = (e) => { if (!ref.current?.contains(e.target)) close(); };
    const onKey = (e) => e.key === 'Escape' && close();
    document.addEventListener('pointerdown', onDown, true);
    document.addEventListener('keydown', onKey);
    window.addEventListener('blur', close);
    return () => {
      document.removeEventListener('pointerdown', onDown, true);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', close);
    };
  }, [menu]);

  const t = menu?.track;
  const liked = t && lib.isLiked(t.id);
  const act = (fn) => () => { fn(); close(); };

  return (
    <Ctx.Provider value={open}>
      {children}
      <AnimatePresence>
        {menu && (
          <motion.div
            ref={ref}
            className="menu"
            style={{ left: pos.left, top: pos.top }}
            initial={{ opacity: 0, scale: 0.94 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.96 }}
            transition={{ duration: 0.12 }}
            role="menu"
            onContextMenu={(e) => e.preventDefault()}
          >
            {!sub ? (
              <>
                <div className="menu-head"><b>{t.title}</b><span>{t.artist}</span></div>
                <button onClick={act(() => player.play(t))}><MdPlayArrow size={20} /> Play</button>
                <button onClick={act(() => player.playNext(t))}><MdPlaylistPlay size={20} /> Play next</button>
                <button onClick={act(() => player.addToQueue(t))}><MdQueueMusic size={20} /> Add to queue</button>
                <hr />
                <button onClick={act(() => toast(lib.toggleLike(t) ? 'Added to Liked songs' : 'Removed from Liked songs'))}>
                  {liked ? <MdFavorite size={20} className="liked" /> : <MdFavoriteBorder size={20} />} {liked ? 'Unlike' : 'Like'}
                </button>
                <button onClick={() => setSub(true)}><MdPlaylistAdd size={20} /> Add to playlist <MdChevronRight size={20} className="end" /></button>
                {t.source !== 'local' && <button onClick={act(() => player.startRadio(t))}><MdSensors size={20} /> Start radio</button>}
                {t.permalink && <hr />}
                {t.permalink && <button onClick={act(() => window.mosaic.openExternal(t.permalink))}><MdOpenInNew size={20} /> Open on SoundCloud</button>}
                {t.permalink && <button onClick={act(() => { navigator.clipboard.writeText(t.permalink); toast('Link copied'); })}><MdLink size={20} /> Copy link</button>}
                {menu.extra.length > 0 && <hr />}
                {menu.extra.map((x) => (
                  <button key={x.label} onClick={act(x.run)}>{x.icon} {x.label}</button>
                ))}
              </>
            ) : (
              <>
                <div className="menu-head"><b>Add to playlist</b></div>
                {lib.data.playlists.map((p) => (
                  <button key={p.id} onClick={act(() => toast(lib.addToPlaylist(p.id, t) ? `Added to ${p.name}` : `Already in ${p.name}`))}>
                    <MdPlaylistPlay size={20} /> {p.name}
                  </button>
                ))}
                <button onClick={act(() => { lib.createPlaylist('New playlist', [t]); toast('Created “New playlist”'); })}><MdAdd size={20} /> New playlist</button>
              </>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </Ctx.Provider>
  );
}
