const { app, ipcMain } = require('electron');
const { openExternalHttps } = require('./window');

const REPO = 'wubly/mosaic';
const RECHECK_MS = 6 * 60 * 60 * 1000;

let state = { current: app.getVersion(), latest: null, available: false, url: null, notes: '', checkedAt: 0, error: null };

const parts = (v) => String(v).replace(/^v/i, '').split('.').map((n) => parseInt(n, 10) || 0);

function newer(a, b) {
  const x = parts(a);
  const y = parts(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0);
  }
  return false;
}

async function check() {
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, {
      signal: AbortSignal.timeout(10000),
      headers: { 'User-Agent': 'Mosaic', Accept: 'application/vnd.github+json' },
    });
    if (!res.ok) throw new Error(res.status === 404 ? 'No releases yet' : `GitHub said ${res.status}`);
    const release = await res.json();
    const latest = String(release.tag_name || '').replace(/^v/i, '');
    state = {
      current: app.getVersion(),
      latest,
      available: !!latest && newer(latest, app.getVersion()),
      url: release.html_url,
      notes: release.body || '',
      checkedAt: Date.now(),
      error: null,
    };
  } catch (e) {
    state = { ...state, checkedAt: Date.now(), error: e.message };
  }
  return state;
}

function registerUpdateIpc() {
  ipcMain.handle('update:get', () => state);
  ipcMain.handle('update:check', check);
  ipcMain.handle('update:open', () => { if (state.url) openExternalHttps(state.url); });

  setTimeout(check, 5000);
  setInterval(check, RECHECK_MS);
}

module.exports = { registerUpdateIpc };
