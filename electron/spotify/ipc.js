const { ipcMain } = require('electron');
const { parseSpotifyUrl, canonicalUrl } = require('./url');
const { readEmbed } = require('./embed');
const { readWeb, openLogin } = require('./web');
const { findMatch } = require('./match');

const EMBED_LIMIT = 100;

async function importPlaylist(input, options = {}) {
  const ref = parseSpotifyUrl(input);
  if (!ref) return { error: 'badurl' };

  let result = null;
  if (!options.web) {
    try { result = await readEmbed(ref); } catch {}
  }

  if (!result || result.tracks.length >= EMBED_LIMIT) {
    const web = await readWeb(canonicalUrl(input, ref));
    if (web.tracks?.length && (!result || web.tracks.length > result.tracks.length)) {
      result = { ...web, cover: result?.cover || web.cover, owner: result?.owner || web.owner };
    } else if (!result) {
      return web.needsLogin ? { needsLogin: true } : { error: web.error || 'unavailable' };
    }
  }
  return result;
}

function registerSpotifyIpc() {
  ipcMain.handle('spotify:playlist', (_event, input, options) => importPlaylist(input, options));
  ipcMain.handle('spotify:login', () => openLogin());
  ipcMain.handle('sc:match', (_event, track) => findMatch(track));
}

module.exports = { registerSpotifyIpc };
