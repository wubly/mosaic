const { app, ipcMain } = require('electron');
const { getMainWindow } = require('../window');
const { buildActivity } = require('./activity');

const DISCORD_CLIENT_ID = '1562187683592147045';
const RETRY_MS = 15000;
const MIN_PUSH_GAP_MS = 4000;

let enabled = false;
let client = null;
let user = null;
let pending;
let retryTimer = null;
let pushTimer = null;
let lastPush = 0;
let lastError = null;

const status = () => ({ enabled, connected: !!user, user: user?.username || null, error: lastError });
const notify = () => getMainWindow()?.webContents.send('presence:status', status());

function scheduleRetry() {
  if (!enabled || retryTimer) return;
  retryTimer = setTimeout(() => { retryTimer = null; connect(); }, RETRY_MS);
}

function schedulePush(now = false) {
  clearTimeout(pushTimer);
  const wait = now ? 0 : Math.max(0, MIN_PUSH_GAP_MS - (Date.now() - lastPush));
  pushTimer = setTimeout(push, wait);
}

async function push() {
  if (!user || pending === undefined) return;

  const next = pending;
  pending = undefined;
  lastPush = Date.now();

  try {
    if (next === null) await user.clearActivity();
    else await user.setActivity(next);
    if (lastError) { lastError = null; notify(); }
  } catch (err) {
    pending = next;
    lastError = String(err?.message || err).slice(0, 140);
    notify();
  }
}

async function connect() {
  if (!enabled || client) return;

  let Client;
  try { ({ Client } = require('@xhayper/discord-rpc')); } catch { return; }

  const rpc = new Client({ clientId: DISCORD_CLIENT_ID });
  client = rpc;

  rpc.on('ready', () => { user = rpc.user; notify(); schedulePush(true); });
  rpc.on('disconnected', () => { user = null; client = null; notify(); scheduleRetry(); });

  try {
    await rpc.login();
  } catch {
    try { rpc.destroy(); } catch {}
    client = null;
    user = null;
    notify();
    scheduleRetry();
  }
}

async function disconnect() {
  enabled = false;
  clearTimeout(retryTimer);
  retryTimer = null;
  clearTimeout(pushTimer);

  try { await user?.clearActivity(); } catch {}
  try { client?.destroy(); } catch {}

  client = null;
  user = null;
  pending = undefined;
  notify();
}

function registerDiscordIpc() {
  ipcMain.handle('presence:enable', async (_event, on) => {
    if (on && !enabled) { enabled = true; connect(); }
    else if (!on && enabled) await disconnect();
    return status();
  });

  ipcMain.handle('presence:status', () => status());

  ipcMain.handle('presence:update', (_event, payload) => {
    if (!enabled) return;
    pending = payload ? buildActivity(payload) : null;
    schedulePush();
  });

  app.on('before-quit', () => { disconnect(); });
}

module.exports = { registerDiscordIpc };
