'use client';
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import { useSpotify } from '@/components/spotify/SpotifyProvider';

const Ctx = createContext({ status: null, refresh: async () => {}, enabled: false });
export const useYoutube = () => useContext(Ctx);

export default function YoutubeProvider({ children }) {
  const { beta } = useSpotify();
  const [status, setStatus] = useState(null);

  const refresh = useCallback(async () => {
    const next = await window.mosaic.ytStatus();
    setStatus(next);
    return next;
  }, []);

  useEffect(() => { if (beta) refresh().catch(() => {}); }, [beta, refresh]);

  const value = useMemo(() => ({ status, refresh, enabled: beta && !!status?.connected }), [beta, status, refresh]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
