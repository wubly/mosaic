const http = require('http');
const crypto = require('crypto');
const fs = require('fs');
const { shell } = require('electron');
const { userFile, readJson, writeJson, readConfig, writeConfig } = require('../core/storage');

const PORT = 8899;
const REDIRECT = `http://127.0.0.1:${PORT}/callback`;
const SCOPE = 'https://www.googleapis.com/auth/youtube.readonly';
const authFile = () => userFile('youtube-auth.json');
const b64url = (buf) => buf.toString('base64url');

let tokens = readJson(authFile(), null);
let channel = null;
let pendingServer = null;

const creds = () => {
  const { youtubeClientId = '', youtubeClientSecret = '' } = readConfig();
  return { id: youtubeClientId.trim(), secret: youtubeClientSecret.trim() };
};

async function tokenRequest(params) {
  const { id, secret } = creds();
  const res = await fetch('https://oauth2.googleapis.com/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ client_id: id, ...(secret ? { client_secret: secret } : {}), ...params }),
    signal: AbortSignal.timeout(15000),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(json.error_description || json.error || `Google said ${res.status}`);
  return json;
}

function store(json) {
  tokens = { access: json.access_token, refresh: json.refresh_token || tokens?.refresh, expires: Date.now() + (json.expires_in - 60) * 1000 };
  writeJson(authFile(), tokens);
}

async function getToken() {
  if (!tokens?.refresh) throw new Error('Not connected to YouTube');
  if (Date.now() >= tokens.expires) store(await tokenRequest({ grant_type: 'refresh_token', refresh_token: tokens.refresh }));
  return tokens.access;
}

async function api(path, query = {}) {
  const url = new URL(`https://www.googleapis.com/youtube/v3${path}`);
  for (const [k, v] of Object.entries(query)) url.searchParams.set(k, v);
  const res = await fetch(url, { headers: { Authorization: `Bearer ${await getToken()}` }, signal: AbortSignal.timeout(15000) });
  const json = await res.json().catch(() => null);
  if (!res.ok) {
    const reason = json?.error?.errors?.[0]?.reason;
    throw new Error(reason === 'quotaExceeded' ? 'YouTube’s daily quota is used up. Try again tomorrow.' : json?.error?.message || `YouTube said ${res.status}`);
  }
  return json;
}

async function loadChannel() {
  try { channel = (await api('/channels', { part: 'snippet', mine: 'true' })).items?.[0] || null; } catch { channel = null; }
  return channel;
}

function connect() {
  const { id } = creds();
  if (!id) return Promise.reject(new Error('Add your Google client ID first'));
  pendingServer?.close();

  const verifier = b64url(crypto.randomBytes(48));
  const challenge = b64url(crypto.createHash('sha256').update(verifier).digest());
  const state = b64url(crypto.randomBytes(12));

  return new Promise((resolve, reject) => {
    const server = http.createServer(async (req, res) => {
      const url = new URL(req.url, REDIRECT);
      if (url.pathname !== '/callback') { res.writeHead(404).end(); return; }

      const done = (message, error) => {
        res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
        res.end(`<body style="font:16px system-ui;background:#141218;color:#e6e0e9;display:grid;place-items:center;height:100vh;margin:0"><p>${message}</p></body>`);
        server.close();
        pendingServer = null;
        error ? reject(error) : resolve();
      };

      if (url.searchParams.get('state') !== state) return done('Something went wrong. Close this tab and try again.', new Error('State mismatch'));
      if (url.searchParams.get('error')) return done('Google sign-in was cancelled.', new Error('Sign-in cancelled'));

      try {
        store(await tokenRequest({ grant_type: 'authorization_code', code: url.searchParams.get('code'), redirect_uri: REDIRECT, code_verifier: verifier }));
        await loadChannel();
        done('Connected. You can close this tab and go back to Mosaic.');
      } catch (e) {
        done('Could not finish signing in. Close this tab and try again.', e);
      }
    });

    server.on('error', (e) => reject(new Error(e.code === 'EADDRINUSE' ? `Port ${PORT} is busy` : e.message)));
    server.listen(PORT, '127.0.0.1', () => {
      pendingServer = server;
      const auth = new URL('https://accounts.google.com/o/oauth2/v2/auth');
      auth.search = new URLSearchParams({
        client_id: id, response_type: 'code', redirect_uri: REDIRECT, scope: SCOPE,
        code_challenge_method: 'S256', code_challenge: challenge, state, access_type: 'offline', prompt: 'consent',
      }).toString();
      shell.openExternal(auth.toString());
    });
    setTimeout(() => { if (pendingServer === server) { server.close(); pendingServer = null; reject(new Error('Sign-in timed out')); } }, 5 * 60 * 1000);
  });
}

function disconnect() {
  tokens = null;
  channel = null;
  try { fs.rmSync(authFile(), { force: true }); } catch {}
}

async function status() {
  if (tokens && !channel) await loadChannel();
  return { clientId: creds().id, hasSecret: !!creds().secret, redirect: REDIRECT, connected: !!tokens && !!channel, user: channel?.snippet?.title || null };
}

const setCredentials = (id, secret) => {
  const config = readConfig();
  writeConfig({ ...config, youtubeClientId: String(id || '').trim(), youtubeClientSecret: String(secret || '').trim() || config.youtubeClientSecret || '' });
};

module.exports = { api, connect, disconnect, status, setCredentials };
