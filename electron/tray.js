const { app, Tray, Menu, nativeImage } = require('electron');
const { APP_ICON } = require('./core/constants');
const { readConfig, writeConfig } = require('./core/storage');
const { getMainWindow, showMainWindow } = require('./window');

let tray = null;
let quitting = false;

const closeToTray = () => readConfig().closeToTray !== false;
const setCloseToTray = (on) => writeConfig({ ...readConfig(), closeToTray: !!on });
const isQuitting = () => quitting;

function send(command) {
  getMainWindow()?.webContents.send('media-key', command);
}

function createTray() {
  if (tray) return;
  tray = new Tray(nativeImage.createFromPath(APP_ICON).resize({ width: 16, height: 16 }));
  tray.setToolTip('Mosaic');
  tray.setContextMenu(Menu.buildFromTemplate([
    { label: 'Open Mosaic', click: showMainWindow },
    { type: 'separator' },
    { label: 'Play / pause', click: () => send('toggle') },
    { label: 'Next', click: () => send('next') },
    { label: 'Previous', click: () => send('prev') },
    { type: 'separator' },
    { label: 'Quit', click: () => app.quit() },
  ]));
  tray.on('click', showMainWindow);
  app.on('before-quit', () => { quitting = true; });
}

module.exports = { createTray, closeToTray, setCloseToTray, isQuitting };
