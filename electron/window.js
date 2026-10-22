const { BrowserWindow, ipcMain, shell } = require('electron');
const { APP_ICON, PRELOAD, DEV, DEV_URL, APP_URL } = require('./core/constants');

const IS_MAC = process.platform === 'darwin';

let mainWindow = null;
let normalBounds = null;

const getMainWindow = () => mainWindow;

function openExternalHttps(url) {
  if (/^https:\/\//.test(url)) shell.openExternal(url);
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1280,
    height: 820,
    minWidth: 980,
    minHeight: 640,
    title: 'Mosaic',
    backgroundColor: '#141218',
    icon: APP_ICON,
    autoHideMenuBar: true,
    titleBarStyle: 'hidden',
    ...(IS_MAC
      ? { trafficLightPosition: { x: 18, y: 12 } }
      : { titleBarOverlay: { color: '#00000000', symbolColor: '#cac4d0', height: 40 } }),
    webPreferences: {
      preload: PRELOAD,
      contextIsolation: true,
      nodeIntegration: false,
      backgroundThrottling: true,
    },
  });

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    openExternalHttps(url);
    return { action: 'deny' };
  });
  mainWindow.webContents.on('before-input-event', (event, input) => {
    const combo = input.type === 'keyDown' && (input.key === 'F12' || (input.key.toLowerCase() === 'i' && input.shift && (input.control || (IS_MAC && input.meta))) || (IS_MAC && input.alt && input.meta && input.key.toLowerCase() === 'i'));
    if (combo) { mainWindow.webContents.toggleDevTools(); event.preventDefault(); }
  });
  mainWindow.loadURL(DEV ? DEV_URL : APP_URL);
  mainWindow.on('close', (event) => {
    const { closeToTray, isQuitting } = require('./tray');
    if (isQuitting() || !closeToTray()) return;
    event.preventDefault();
    mainWindow.hide();
  });
  mainWindow.on('closed', () => { mainWindow = null; });
  return mainWindow;
}

function focusMainWindow() {
  if (!mainWindow) return;
  if (!mainWindow.isVisible()) mainWindow.show();
  if (mainWindow.isMinimized()) mainWindow.restore();
  mainWindow.focus();
}

function showMainWindow() {
  if (!mainWindow) return createWindow();
  mainWindow.show();
  focusMainWindow();
}

function registerWindowIpc() {
  ipcMain.handle('win:mini', (event, on) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win) return;
    if (on) {
      normalBounds = win.getBounds();
      win.setMinimumSize(340, 440);
      win.setSize(380, 660, true);
      win.setAlwaysOnTop(true, 'floating');
    } else {
      win.setAlwaysOnTop(false);
      win.setMinimumSize(980, 640);
      if (normalBounds) win.setBounds(normalBounds, true);
    }
  });
}

module.exports = { createWindow, getMainWindow, focusMainWindow, showMainWindow, registerWindowIpc, openExternalHttps };
