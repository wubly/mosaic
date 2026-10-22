const { app, components } = require('electron');
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
const { setupMac } = require('./mac');
const { registerUpdateIpc } = require('./update');
const { registerSpotifySdkIpc } = require('./spotify/sdk');
const { registerYoutubeIpc } = require('./youtube/ipc');
const { registerDiscordIpc } = require('./discord/presence');

registerSchemes();

if (!app.requestSingleInstanceLock()) {
  app.quit();
} else {
  app.on('second-instance', focusMainWindow);

  app.whenReady().then(async () => {
    try { await components?.whenReady(); } catch {}
    registerHandlers();
    registerAppIpc();
    registerWindowIpc();
    registerSoundCloudIpc();
    registerLyricsIpc();
    registerLibraryIpc();
    registerSpotifyIpc();
    registerDiscordIpc();
    registerUpdateIpc();
    registerSpotifySdkIpc();
    registerYoutubeIpc();

    createWindow();
    registerMediaKeys();
    createTray();
    setupMac();

  });

  app.on('will-quit', unregisterMediaKeys);
  app.on('window-all-closed', () => {
    if (process.platform !== 'darwin') app.quit();
  });
}
