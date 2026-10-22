const { app, ipcMain } = require('electron');
const { readConfig, writeConfig, userFile, readJson, writeJson } = require('../core/storage');
const { isLowMemory } = require('../core/flags');
const { closeToTray, setCloseToTray } = require('../tray');
const { openExternalHttps } = require('../window');

function registerAppIpc() {
  ipcMain.handle('config:get', () => {
    const { name, onboarded } = readConfig();
    return { onboarded: !!onboarded, name: name ?? '' };
  });

  ipcMain.handle('config:save', (_event, { name }) => {
    writeConfig({ ...readConfig(), name: (name ?? '').trim(), onboarded: true });
    return { ok: true };
  });

  ipcMain.handle('config:reset', () => {
    const { clientId, lowMemory, closeToTray: tray, genius, beta, spotifyClientId, youtubeClientId, youtubeClientSecret } = readConfig();
    writeConfig({ ...(clientId ? { clientId } : {}), ...(lowMemory === false ? { lowMemory: false } : {}), ...(tray === false ? { closeToTray: false } : {}), ...(genius ? { genius: true } : {}), ...(beta ? { beta: true } : {}), ...(spotifyClientId ? { spotifyClientId } : {}), ...(youtubeClientId ? { youtubeClientId } : {}), ...(youtubeClientSecret ? { youtubeClientSecret } : {}) });
  });

  ipcMain.handle('perf:get', () => ({ lowMemory: isLowMemory() }));

  ipcMain.handle('perf:set', (_event, on) => {
    writeConfig({ ...readConfig(), lowMemory: !!on });
    return { lowMemory: !!on };
  });

  ipcMain.handle('beta:get', () => !!readConfig().beta);
  ipcMain.handle('beta:set', (_event, on) => { writeConfig({ ...readConfig(), beta: !!on }); return !!on; });
  ipcMain.handle('genius:get', () => !!readConfig().genius);
  ipcMain.handle('genius:set', (_event, on) => { writeConfig({ ...readConfig(), genius: !!on }); return !!on; });

  ipcMain.handle('tray:get', () => closeToTray());
  ipcMain.handle('tray:set', (_event, on) => { setCloseToTray(on); return !!on; });

  ipcMain.handle('app:relaunch', () => {
    app.relaunch();
    app.exit(0);
  });

  ipcMain.handle('app:openExternal', (_event, url) => openExternalHttps(url));

  ipcMain.handle('store:load', () => readJson(userFile('mosaic-data.json'), {}));

  ipcMain.handle('store:save', (_event, data) => {
    writeJson(userFile('mosaic-data.json'), data);
    return true;
  });
}

module.exports = { registerAppIpc };
