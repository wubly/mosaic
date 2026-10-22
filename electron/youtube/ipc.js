const { ipcMain, session } = require('electron');
const auth = require('./auth');

const seconds = (iso = '') => {
  const m = /^P(?:(\d+)D)?T?(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?$/.exec(iso) || [];
  return ((+m[1] || 0) * 86400 + (+m[2] || 0) * 3600 + (+m[3] || 0) * 60 + (+m[4] || 0));
};

const decode = (t = '') => t.replace(/&(amp|quot|#39|lt|gt);/g, (_m, e) => ({ amp: '&', quot: '"', '#39': "'", lt: '<', gt: '>' })[e]);

function slim(v) {
  const thumbs = v.snippet.thumbnails || {};
  const best = thumbs.maxres || thumbs.standard || thumbs.high || thumbs.medium || thumbs.default;
  return {
    id: `yt:${v.id}`,
    source: 'youtube',
    videoId: v.id,
    title: decode(v.snippet.title),
    artist: decode(v.snippet.channelTitle || '').replace(/\s*-\s*Topic$/i, '') || 'YouTube',
    artwork: best?.url || '',
    thumb: (thumbs.medium || best)?.url || '',
    duration: seconds(v.contentDetails?.duration) * 1000,
    permalink: `https://www.youtube.com/watch?v=${v.id}`,
    streamable: v.status?.embeddable !== false && v.snippet.liveBroadcastContent !== 'live',
  };
}

async function details(ids) {
  if (!ids.length) return [];
  const json = await auth.api('/videos', { part: 'snippet,contentDetails,status', id: ids.join(','), maxResults: 50 });
  const byId = new Map((json.items || []).map((v) => [v.id, v]));
  return ids.map((id) => byId.get(id)).filter(Boolean).map(slim).filter((t) => t.streamable && t.duration > 0);
}

function registerYoutubeIpc() {
  // youtube's embed refuses custom-scheme origins (error 153), so present a normal https referer
  session.defaultSession.webRequest.onBeforeSendHeaders({ urls: ['*://*.youtube.com/*', '*://*.youtube-nocookie.com/*'] }, (d, cb) => {
    d.requestHeaders.Referer = 'https://mosaic.app/';
    cb({ requestHeaders: d.requestHeaders });
  });

  ipcMain.handle('yt:status', () => auth.status());
  ipcMain.handle('yt:credentials', (_e, id, secret) => { auth.setCredentials(id, secret); return auth.status(); });
  ipcMain.handle('yt:connect', async () => { await auth.connect(); return auth.status(); });
  ipcMain.handle('yt:disconnect', () => { auth.disconnect(); return auth.status(); });

  ipcMain.handle('yt:search', async (_e, q) => {
    const json = await auth.api('/search', { part: 'snippet', type: 'video', videoCategoryId: '10', videoEmbeddable: 'true', maxResults: 20, q });
    return details((json.items || []).map((i) => i.id?.videoId).filter(Boolean));
  });

  ipcMain.handle('yt:library', async () => {
    const [liked, lists] = await Promise.allSettled([
      auth.api('/videos', { part: 'snippet,contentDetails,status', myRating: 'like', maxResults: 30 }),
      auth.api('/playlists', { part: 'snippet,contentDetails', mine: 'true', maxResults: 25 }),
    ]);
    return {
      liked: liked.status === 'fulfilled' ? (liked.value.items || []).map(slim).filter((t) => t.streamable && t.duration > 0) : [],
      playlists: lists.status === 'fulfilled'
        ? (lists.value.items || []).map((p) => ({ id: p.id, name: p.snippet.title, count: p.contentDetails?.itemCount || 0, image: p.snippet.thumbnails?.medium?.url || '' }))
        : [],
    };
  });

  ipcMain.handle('yt:playlist', async (_e, id) => {
    const json = await auth.api('/playlistItems', { part: 'contentDetails', playlistId: id, maxResults: 50 });
    return details((json.items || []).map((i) => i.contentDetails?.videoId).filter(Boolean));
  });
}

module.exports = { registerYoutubeIpc };
