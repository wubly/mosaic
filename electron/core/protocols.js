const { protocol, net } = require('electron');
const path = require('path');
const { pathToFileURL } = require('url');
const { OUT_DIR } = require('./constants');
const { localPaths, coverDir } = require('../library/scanner');
const { serveFile } = require('../library/serve');

function registerSchemes() {
  protocol.registerSchemesAsPrivileged([
    { scheme: 'app', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true } },
    { scheme: 'media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true, bypassCSP: true } },
  ]);
}

function handleApp(req) {
  let requested = decodeURIComponent(new URL(req.url).pathname);
  if (requested === '/' || requested === '') requested = '/index.html';
  const file = path.normalize(path.join(OUT_DIR, requested));
  if (!file.startsWith(OUT_DIR)) return new Response('Forbidden', { status: 403 });
  return net.fetch(pathToFileURL(file).toString());
}

function handleMedia(req) {
  const url = new URL(req.url);
  const key = decodeURIComponent(url.pathname.replace(/^\//, ''));

  if (url.hostname === 'file') {
    const file = localPaths.get(key);
    return file ? serveFile(file, req) : new Response('Unknown track', { status: 404 });
  }
  if (url.hostname === 'art') return serveFile(path.join(coverDir(), path.basename(key)), req);
  return new Response('Not found', { status: 404 });
}

function registerHandlers() {
  protocol.handle('app', handleApp);
  protocol.handle('media', handleMedia);
}

module.exports = { registerSchemes, registerHandlers };
