const { USER_AGENT } = require('../core/constants');
const { readConfig, writeConfig } = require('../core/storage');

let clientId;

async function discoverClientId() {
  const html = await (await fetch('https://soundcloud.com', { headers: { 'User-Agent': USER_AGENT } })).text();
  const scripts = [...html.matchAll(/<script[^>]+src="(https:\/\/a-v2\.sndcdn\.com\/assets\/[^"]+\.js)"/g)].map((m) => m[1]);

  for (const src of scripts.reverse()) {
    const js = await (await fetch(src, { headers: { 'User-Agent': USER_AGENT } })).text();
    const match = js.match(/client_id\s*[:=]\s*"([a-zA-Z0-9]{32})"/);
    if (match) {
      clientId = match[1];
      writeConfig({ ...readConfig(), clientId });
      return clientId;
    }
  }
  throw new Error('Could not reach SoundCloud. Check your connection.');
}

async function scGet(url, params = {}, retry = true) {
  if (clientId === undefined) clientId = readConfig().clientId || null;
  if (!clientId) await discoverClientId();

  const target = new URL(url);
  for (const [key, value] of Object.entries(params)) target.searchParams.set(key, value);
  target.searchParams.set('client_id', clientId);

  const res = await fetch(target, {
    headers: { 'User-Agent': USER_AGENT, Accept: 'application/json' },
    signal: AbortSignal.timeout(15000),
  });

  if ((res.status === 401 || res.status === 403) && retry) {
    await discoverClientId();
    return scGet(url, params, false);
  }
  if (!res.ok) throw new Error(`SoundCloud request failed (${res.status})`);
  return res.json();
}

module.exports = { scGet };
