const { contextBridge, ipcRenderer, webUtils } = require('electron');

if (process.platform === 'darwin') {
  window.addEventListener('DOMContentLoaded', () => { document.documentElement.dataset.platform = 'mac'; });
}

contextBridge.exposeInMainWorld('mosaic', {
  platform: process.platform,
  getConfig: () => ipcRenderer.invoke('config:get'),
  saveConfig: (cfg) => ipcRenderer.invoke('config:save', cfg),
  resetConfig: () => ipcRenderer.invoke('config:reset'),

  loadStore: () => ipcRenderer.invoke('store:load'),
  saveStore: (data) => ipcRenderer.invoke('store:save', data),

  search: (q) => ipcRenderer.invoke('sc:search', q),
  charts: (genre) => ipcRenderer.invoke('sc:charts', genre),
  related: (id, limit) => ipcRenderer.invoke('sc:related', id, limit),
  getStream: (id) => ipcRenderer.invoke('sc:stream', id),
  getLyrics: (track) => ipcRenderer.invoke('lyrics:get', track),
  lyricCandidates: (args) => ipcRenderer.invoke('lyrics:candidates', args),
  selectLyrics: (args) => ipcRenderer.invoke('lyrics:select', args),
  spotifyPlaylist: (url, opts) => ipcRenderer.invoke('spotify:playlist', url, opts),
  spotifyLogin: () => ipcRenderer.invoke('spotify:login'),
  matchTrack: (sp) => ipcRenderer.invoke('sc:match', sp),
  setPresence: (on) => ipcRenderer.invoke('presence:enable', on),
  presenceStatus: () => ipcRenderer.invoke('presence:status'),
  updatePresence: (payload) => ipcRenderer.invoke('presence:update', payload),
  onPresenceStatus: (cb) => {
    const handler = (_e, st) => cb(st);
    ipcRenderer.on('presence:status', handler);
    return () => ipcRenderer.removeListener('presence:status', handler);
  },
  getPerf: () => ipcRenderer.invoke('perf:get'),
  setLowMemory: (on) => ipcRenderer.invoke('perf:set', on),
  getUpdate: () => ipcRenderer.invoke('update:get'),
  checkUpdate: () => ipcRenderer.invoke('update:check'),
  openUpdate: () => ipcRenderer.invoke('update:open'),
  spStatus: () => ipcRenderer.invoke('sp:status'),
  spSetClientId: (id) => ipcRenderer.invoke('sp:clientId', id),
  spConnect: () => ipcRenderer.invoke('sp:connect'),
  spDisconnect: () => ipcRenderer.invoke('sp:disconnect'),
  spToken: () => ipcRenderer.invoke('sp:token'),
  spSearch: (q) => ipcRenderer.invoke('sp:search', q),
  spLibrary: () => ipcRenderer.invoke('sp:library'),
  spArtist: (id) => ipcRenderer.invoke('sp:artist', id),
  spAlbum: (id) => ipcRenderer.invoke('sp:album', id),
  spPlay: (args) => ipcRenderer.invoke('sp:play', args),
  ytStatus: () => ipcRenderer.invoke('yt:status'),
  ytSetCredentials: (id, secret) => ipcRenderer.invoke('yt:credentials', id, secret),
  ytConnect: () => ipcRenderer.invoke('yt:connect'),
  ytDisconnect: () => ipcRenderer.invoke('yt:disconnect'),
  ytSearch: (q) => ipcRenderer.invoke('yt:search', q),
  ytLibrary: () => ipcRenderer.invoke('yt:library'),
  ytPlaylist: (id) => ipcRenderer.invoke('yt:playlist', id),
  getBeta: () => ipcRenderer.invoke('beta:get'),
  setBeta: (on) => ipcRenderer.invoke('beta:set', on),
  getGenius: () => ipcRenderer.invoke('genius:get'),
  setGenius: (on) => ipcRenderer.invoke('genius:set', on),
  getCloseToTray: () => ipcRenderer.invoke('tray:get'),
  setCloseToTray: (on) => ipcRenderer.invoke('tray:set', on),
  relaunch: () => ipcRenderer.invoke('app:relaunch'),
  setMini: (on) => ipcRenderer.invoke('win:mini', on),
  openExternal: (url) => ipcRenderer.invoke('app:openExternal', url),

  pickFolder: () => ipcRenderer.invoke('local:pickFolder'),
  pickFiles: () => ipcRenderer.invoke('local:pickFiles'),
  scanLocal: (paths) => ipcRenderer.invoke('local:scan', paths),
  pathsForFiles: (files) => [...files].map((f) => webUtils.getPathForFile(f)).filter(Boolean),

  onMediaKey: (cb) => {
    const handler = (_e, cmd) => cb(cmd);
    ipcRenderer.on('media-key', handler);
    return () => ipcRenderer.removeListener('media-key', handler);
  },
});
