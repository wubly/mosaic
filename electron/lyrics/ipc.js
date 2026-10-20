const { ipcMain } = require('electron');
const { getLyrics, getCandidates, selectLyrics } = require('./service');

function registerLyricsIpc() {
  ipcMain.handle('lyrics:get', (_event, args) => getLyrics(args));
  ipcMain.handle('lyrics:candidates', (_event, args) => getCandidates(args));
  ipcMain.handle('lyrics:select', (_event, args) => selectLyrics(args));
}

module.exports = { registerLyricsIpc };
