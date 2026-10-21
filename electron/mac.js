const { app, Menu } = require('electron');
const { getMainWindow, showMainWindow } = require('./window');

const send = (command) => getMainWindow()?.webContents.send('media-key', command);

function setupMac() {
  if (process.platform !== 'darwin') return;

  app.dock.setMenu(Menu.buildFromTemplate([
    { label: 'Play / pause', click: () => send('toggle') },
    { label: 'Next', click: () => send('next') },
    { label: 'Previous', click: () => send('prev') },
  ]));

  Menu.setApplicationMenu(Menu.buildFromTemplate([
    { role: 'appMenu' },
    { role: 'editMenu' },
    {
      label: 'Playback',
      submenu: [
        { label: 'Play / pause', accelerator: 'Space', registerAccelerator: false, click: () => send('toggle') },
        { label: 'Next', accelerator: 'CmdOrCtrl+Right', click: () => send('next') },
        { label: 'Previous', accelerator: 'CmdOrCtrl+Left', click: () => send('prev') },
      ],
    },
    { role: 'windowMenu' },
  ]));

  app.on('activate', showMainWindow);
}

module.exports = { setupMac };
