const { ipcMain } = require('electron');
const { scGet } = require('./client');
const { fullStreams, playable } = require('./tracks');
const { waveformSilence } = require('./silence');

const API = 'https://api-v2.soundcloud.com';

function registerSoundCloudIpc() {
  ipcMain.handle('sc:search', async (_event, query, limit = 30) => {
    const json = await scGet(`${API}/search/tracks`, { q: query, limit });
    return playable(json.collection);
  });

  ipcMain.handle('sc:charts', async (_event, genre = 'all-music', limit = 30) => {
    if (genre === 'all-music') {
      const json = await scGet(`${API}/charts`, { kind: 'trending', genre: 'soundcloud:genres:all-music', limit });
      return playable((json.collection ?? []).map((entry) => entry.track));
    }
    const json = await scGet(`${API}/search/tracks`, {
      q: '',
      'filter.genre_or_tag': genre,
      'filter.created_at': 'last_month',
      sort: 'popular',
      limit,
    });
    return playable(json.collection);
  });

  ipcMain.handle('sc:related', async (_event, id, limit = 20) => {
    const json = await scGet(`${API}/tracks/${id}/related`, { limit });
    return playable(json.collection);
  });

  ipcMain.handle('sc:stream', async (_event, id) => {
    const track = await scGet(`${API}/tracks/${id}`);
    const silence = waveformSilence(track);
    const params = track.track_authorization ? { track_authorization: track.track_authorization } : {};

    for (const transcoding of fullStreams(track.media?.transcodings)) {
      try {
        const { url } = await scGet(transcoding.url, params);
        return { type: transcoding.format.protocol === 'hls' ? 'hls' : 'mp3', url, silence: await silence };
      } catch {}
    }
    throw new Error('No playable stream for this track');
  });
}

module.exports = { registerSoundCloudIpc };
