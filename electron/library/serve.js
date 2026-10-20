const fs = require('fs');
const path = require('path');
const { Readable } = require('stream');

const MIME = {
  '.mp3': 'audio/mpeg',
  '.flac': 'audio/flac',
  '.m4a': 'audio/mp4',
  '.aac': 'audio/aac',
  '.wav': 'audio/wav',
  '.ogg': 'audio/ogg',
  '.opus': 'audio/ogg',
  '.wma': 'audio/x-ms-wma',
  '.webm': 'audio/webm',
};

const contentType = (file) => MIME[path.extname(file).toLowerCase()] || (/\.png$/i.test(file) ? 'image/png' : 'image/jpeg');

function serveFile(file, req) {
  let stat;
  try { stat = fs.statSync(file); } catch { return new Response('Not found', { status: 404 }); }

  const headers = { 'Content-Type': contentType(file), 'Accept-Ranges': 'bytes', 'Access-Control-Allow-Origin': '*' };
  const range = /bytes=(\d*)-(\d*)/.exec(req.headers.get('range') || '');

  if (range && (range[1] || range[2])) {
    const start = range[1] ? parseInt(range[1], 10) : Math.max(0, stat.size - parseInt(range[2], 10));
    const end = range[1] && range[2] ? Math.min(parseInt(range[2], 10), stat.size - 1) : stat.size - 1;

    if (start >= stat.size || start > end) {
      return new Response('Bad range', { status: 416, headers: { 'Content-Range': `bytes */${stat.size}` } });
    }
    return new Response(Readable.toWeb(fs.createReadStream(file, { start, end })), {
      status: 206,
      headers: { ...headers, 'Content-Range': `bytes ${start}-${end}/${stat.size}`, 'Content-Length': String(end - start + 1) },
    });
  }

  return new Response(Readable.toWeb(fs.createReadStream(file)), {
    status: 200,
    headers: { ...headers, 'Content-Length': String(stat.size) },
  });
}

module.exports = { serveFile };
