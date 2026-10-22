'use client';
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from 'react';

const Ctx = createContext({ beta: false, setBeta: () => {}, status: null, refresh: async () => {}, enabled: false, route: null, stack: [], go: () => {}, back: () => {}, registerNav: () => {} });
export const useSpotify = () => useContext(Ctx);

const HOME = { type: 'home' };

export default function SpotifyProvider({ children }) {
  const [beta, setBetaState] = useState(false);
  const [status, setStatus] = useState(null);
  const [stack, setStack] = useState([HOME]);
  const nav = useRef(null);

  const refresh = useCallback(async () => {
    const next = await window.mosaic.spStatus();
    setStatus(next);
    return next;
  }, []);

  useEffect(() => {
    window.mosaic.getBeta().then((on) => { setBetaState(on); if (on) refresh().catch(() => {}); });
  }, [refresh]);

  const setBeta = useCallback(async (on) => {
    setBetaState(await window.mosaic.setBeta(on));
    if (on) refresh().catch(() => {});
  }, [refresh]);

  const go = useCallback((route) => {
    setStack((s) => (route.type === 'home' ? [HOME] : [...s, route]));
    nav.current?.();
  }, []);
  const back = useCallback(() => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s)), []);
  const registerNav = useCallback((fn) => { nav.current = fn; }, []);

  const value = useMemo(
    () => ({ beta, setBeta, status, refresh, enabled: beta && !!status?.connected && status?.premium, route: stack[stack.length - 1], stack, go, back, registerNav }),
    [beta, setBeta, status, refresh, stack, go, back, registerNav],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
