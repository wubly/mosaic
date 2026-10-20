const { BrowserWindow } = require('electron');
const { USER_AGENT } = require('../core/constants');
const { sleep, withTimeout } = require('../core/util');
const { getMainWindow } = require('../window');
const { READY_CHECK, SCRAPE } = require('./scrape-script');

const PARTITION = 'persist:spotify';
const LOGIN_URL = 'https://accounts.spotify.com/en/login?continue=' + encodeURIComponent('https://open.spotify.com/');

async function readWeb(url) {
  const win = new BrowserWindow({
    show: false,
    width: 1280,
    height: 900,
    webPreferences: { partition: PARTITION, backgroundThrottling: false, contextIsolation: true, nodeIntegration: false },
  });

  try {
    win.loadURL(url, { userAgent: USER_AGENT }).catch(() => {});
    const contents = win.webContents;
    const started = Date.now();

    for (;;) {
      if (win.isDestroyed()) return { error: 'closed' };
      if (/accounts\.spotify\.com/.test(contents.getURL())) return { needsLogin: true };

      const state = await withTimeout(contents.executeJavaScript(READY_CHECK), 3000).catch(() => undefined);
      if (state?.rows) break;

      const waited = Date.now() - started;
      if (waited > 10000 && state && /log in|sign up|sign in/i.test(state.text)) return { needsLogin: true };
      if (waited > 25000) return { error: 'timeout' };
      await sleep(500);
    }

    const data = await withTimeout(contents.executeJavaScript(SCRAPE), 120000);
    if (!data?.tracks?.length) return { error: 'empty' };
    return { via: 'web', name: data.name, owner: '', cover: '', tracks: data.tracks };
  } finally {
    if (!win.isDestroyed()) win.destroy();
  }
}

function openLogin() {
  return new Promise((resolve) => {
    const win = new BrowserWindow({
      width: 480,
      height: 740,
      title: 'Sign in to Spotify',
      autoHideMenuBar: true,
      parent: getMainWindow() || undefined,
      webPreferences: { partition: PARTITION, contextIsolation: true, nodeIntegration: false },
    });

    let finished = false;
    const finish = (signedIn) => {
      if (finished) return;
      finished = true;
      resolve(signedIn);
      if (!win.isDestroyed()) win.close();
    };

    win.loadURL(LOGIN_URL, { userAgent: USER_AGENT });
    win.webContents.on('did-navigate', (_event, url) => {
      if (/^https:\/\/open\.spotify\.com\//.test(url)) finish(true);
    });
    win.on('closed', () => finish(false));
  });
}

module.exports = { readWeb, openLogin };
