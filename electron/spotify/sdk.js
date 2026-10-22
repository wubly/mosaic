const { ipcMain } = require('electron');
const auth = require('./auth');

const image = (list, i = 0) => list?.[Math.min(i, (list?.length || 1) - 1)]?.url || '';

const slim = (t) => ({
  id: `sp:${t.id}`,
  source: 'spotify',
  uri: t.uri,
  title: t.name,
  artist: (t.artists || []).map((a) => a.name).join(', ') || 'Unknown',
  artistId: t.artists?.[0]?.id,
  album: t.album?.name || '',
  albumId: t.album?.id,
  artwork: image(t.album?.images, 0),
  thumb: image(t.album?.images, 1),
  duration: t.duration_ms,
  permalink: t.external_urls?.spotify,
  streamable: t.is_playable !== false,
});

const albumCard = (a) => ({
  id: a.id,
  name: a.name,
  artist: (a.artists || []).map((x) => x.name).join(', '),
  artistId: a.artists?.[0]?.id,
  image: image(a.images, 1),
  year: (a.release_date || '').slice(0, 4),
  kind: a.album_type,
});

const artistCard = (a) => ({ id: a.id, name: a.name, image: image(a.images, 1), followers: a.followers?.total, genres: a.genres || [] });

const usable = (list) => (list || []).filter((t) => t && t.id && t.uri).map(slim).filter((t) => t.streamable);
const settled = (r, fallback) => (r.status === 'fulfilled' ? r.value : fallback);

async function allAlbums(id) {
  const json = await auth.api(`/artists/${id}/albums`, { query: { include_groups: 'album,single', limit: 50, market: auth.country() } });
  const seen = new Set();
  return (json.items || []).filter((a) => { const k = a.name.toLowerCase(); return !seen.has(k) && seen.add(k); }).map(albumCard);
}

function registerSpotifySdkIpc() {
  ipcMain.handle('sp:status', () => auth.status());
  ipcMain.handle('sp:clientId', (_e, id) => { auth.setClientId(id); return auth.status(); });
  ipcMain.handle('sp:connect', async () => { await auth.connect(); return auth.status(); });
  ipcMain.handle('sp:disconnect', () => { auth.disconnect(); return auth.status(); });
  ipcMain.handle('sp:token', () => auth.getToken());

  ipcMain.handle('sp:search', async (_e, q, limit = 20) => {
    const json = await auth.api('/search', { query: { q, type: 'track,artist,album', limit } });
    return {
      tracks: usable(json.tracks?.items),
      artists: (json.artists?.items || []).filter(Boolean).slice(0, 10).map(artistCard),
      albums: (json.albums?.items || []).filter(Boolean).slice(0, 10).map(albumCard),
    };
  });

  ipcMain.handle('sp:library', async () => {
    const [saved, albums, artists] = await Promise.allSettled([
      auth.api('/me/tracks', { query: { limit: 30 } }),
      auth.api('/me/albums', { query: { limit: 30 } }),
      auth.api('/me/following', { query: { type: 'artist', limit: 30 } }),
    ]);
    return {
      saved: usable(settled(saved, { items: [] }).items.map((i) => i.track)),
      albums: settled(albums, { items: [] }).items.map((i) => albumCard(i.album)),
      artists: (settled(artists, { artists: { items: [] } }).artists?.items || []).map(artistCard),
    };
  });

  ipcMain.handle('sp:artist', async (_e, id) => {
    const [artist, top, albums] = await Promise.all([
      auth.api(`/artists/${id}`),
      auth.api(`/artists/${id}/top-tracks`, { query: { market: auth.country() } }).catch(() => ({ tracks: [] })),
      allAlbums(id).catch(() => []),
    ]);
    return { artist: artistCard(artist), top: usable(top.tracks), albums };
  });

  ipcMain.handle('sp:album', async (_e, id) => {
    const album = await auth.api(`/albums/${id}`, { query: { market: auth.country() } });
    const tracks = usable((album.tracks?.items || []).map((t) => ({ ...t, album })));
    return { album: { ...albumCard(album), image: image(album.images, 0), total: album.total_tracks }, tracks };
  });

  ipcMain.handle('sp:play', async (_e, { deviceId, uri, positionMs = 0 }) => {
    const play = () => auth.api('/me/player/play', { method: 'PUT', query: { device_id: deviceId }, body: { uris: [uri], position_ms: Math.round(positionMs) } });
    try {
      return await play();
    } catch (first) {
      await auth.api('/me/player', { method: 'PUT', body: { device_ids: [deviceId], play: false } }).catch(() => {});
      await new Promise((r) => setTimeout(r, 700));
      try { return await play(); } catch { throw first; }
    }
  });
}

module.exports = { registerSpotifySdkIpc };
