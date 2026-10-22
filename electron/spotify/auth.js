const http = require('http');
const crypto = require('crypto');
const { shell } = require('electron');
const { userFile, readJson, writeJson, readConfig, writeConfig } = require('../core/storage');

const PORT = 8898;
const REDIRECT = `http://127.0.0.1:${PORT}/callback`;
const SCOPES = [
  'streaming', 'user-read-email', 'user-read-private',
  'user-library-read', 'user-follow-read', 'playlist-read-private', 'playlist-read-collaborative',
  'user-modify-playback-state', 'user-read-playback-state',
].join(' ');

const authFile = () => userFile('spotify-auth.json');
const b64url = (buf) => buf.toString('base64url');

let tokens = readJson(authFile(), null);
let profile = null;
let pendingServer = null;

const clientId = () => (readConfig().spotifyClientId || '').trim();

async function tokenRequest(params) {
  const res = await fetch('https://accounts.spotify.com/api/token', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ client_id: clientId(), ...params }),
    signal: AbortSignal.timeout(15000),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(json.error_description || json.error || `Spotify said ${res.status}`);
  return json;
}

function store(json) {
  tokens = {
    access: json.access_token,
    refresh: json.refresh_token || tokens?.refresh,
    expires: Date.now() + (json.expires_in - 60) * 1000,
  };
  writeJson(authFile(), tokens);
}

async function getToken() {
  if (!tokens?.refresh) throw new Error('Not connected to Spotify');
  if (Date.now() >= tokens.expires) store(await tokenRequest({ grant_type: 'refresh_token', refresh_token: tokens.refresh }));
  return tokens.access;
}

async function api(path, { method = 'GET', body, query } = {}) {
  const url = new URL(path.startsWith('http') ? path : `https://api.spotify.com/v1${path}`);
  for (const [k, v] of Object.entries(query || {})) url.searchParams.set(k, v);

  const res = await fetch(url, {
    method,
    headers: { Authorization: `Bearer ${await getToken()}`, ...(body ? { 'Content-Type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
    signal: AbortSignal.timeout(15000),
  });
  if (res.status === 204) return null;
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(json?.error?.message || `Spotify said ${res.status}`);
  return json;
}

async function loadProfile() {
  try { profile = await api('/me'); } catch { profile = null; }
  return profile;
}

function connect() {
  if (!clientId()) return Promise.reject(new Error('Add your Spotify client ID first'));
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
      if (url.searchParams.get('error')) return done('Spotify sign-in was cancelled.', new Error('Sign-in cancelled'));

      try {
        store(await tokenRequest({ grant_type: 'authorization_code', code: url.searchParams.get('code'), redirect_uri: REDIRECT, code_verifier: verifier }));
        await loadProfile();
        done('Connected. You can close this tab and go back to Mosaic.');
      } catch (e) {
        done('Could not finish signing in. Close this tab and try again.', e);
      }
    });

    server.on('error', (e) => reject(new Error(e.code === 'EADDRINUSE' ? `Port ${PORT} is busy` : e.message)));
    server.listen(PORT, '127.0.0.1', () => {
      pendingServer = server;
      const auth = new URL('https://accounts.spotify.com/authorize');
      auth.search = new URLSearchParams({
        client_id: clientId(), response_type: 'code', redirect_uri: REDIRECT, scope: SCOPES,
        code_challenge_method: 'S256', code_challenge: challenge, state,
      }).toString();
      shell.openExternal(auth.toString());
    });
    setTimeout(() => { if (pendingServer === server) { server.close(); pendingServer = null; reject(new Error('Sign-in timed out')); } }, 5 * 60 * 1000);
  });
}

function disconnect() {
  tokens = null;
  profile = null;
  try { require('fs').rmSync(authFile(), { force: true }); } catch {}
}

async function status() {
  if (tokens && !profile) await loadProfile();
  return {
    clientId: clientId(),
    redirect: REDIRECT,
    connected: !!tokens && !!profile,
    user: profile?.display_name || profile?.id || null,
    premium: profile?.product === 'premium',
  };
}

const setClientId = (id) => writeConfig({ ...readConfig(), spotifyClientId: String(id || '').trim() });

const country = () => profile?.country || 'US';

module.exports = { country, api, getToken, connect, disconnect, status, setClientId };
