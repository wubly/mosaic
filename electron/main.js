const { app, BrowserWindow } = require('electron');
const { applyFlags } = require('./core/flags');

applyFlags();

const { registerSchemes, registerHandlers } = require('./core/protocols');
const { createWindow, focusMainWindow, registerWindowIpc } = require('./window');
const { registerMediaKeys, unregisterMediaKeys } = require('./media-keys');
const { registerAppIpc } = require('./ipc/app');
const { registerSoundCloudIpc } = require('./soundcloud/ipc');
const { registerLyricsIpc } = require('./lyrics/ipc');
const { registerLibraryIpc } = require('./library/ipc');
const { registerSpotifyIpc } = require('./spotify/ipc');
const { createTray } = require('./tray');
const { registerDiscordIpc } = require('./discord/presence');

registerSchemes();

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', focusMainWindow);

  app.whenReady().then(() => {
    registerHandlers();
    registerAppIpc();
    registerWindowIpc();
    registerSoundCloudIpc();
    registerLyricsIpc();
    registerLibraryIpc();
    registerSpotifyIpc();
    registerDiscordIpc();

    createWindow();
    registerMediaKeys();
    createTray();

    app.on('activate', () => {
      if (BrowserWindow.getAllWindows().length === 0) createWindow();
    });
  });

  app.on('will-quit', unregisterMediaKeys);
  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });
}
