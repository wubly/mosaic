const { app, nativeImage } = require('electron');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { userFile, readJson, writeJson } = require('../core/storage');

const AUDIO_EXT = new Set(['.mp3', '.flac', '.m4a', '.aac', '.wav', '.ogg', '.opus', '.wma', '.webm']);
const CACHE_VERSION = 4;
const SCAN_WORKERS = 4;

const localPaths = new Map();
let metaCache = null;

const coverDir = () => path.join(app.getPath('userData'), 'covers');
const idForPath = (file) => 'local:' + crypto.createHash('sha1').update(file.toLowerCase()).digest('hex').slice(0, 16);
const isAudio = (file) => AUDIO_EXT.has(path.extname(file).toLowerCase());

function loadMetaCache() {
  if (metaCache) return metaCache;
  metaCache = readJson(userFile('local-cache.json'), {});
  if (metaCache.__v !== CACHE_VERSION) {
    metaCache = { __v: CACHE_VERSION };
    try { fs.rmSync(coverDir(), { recursive: true, force: true }); } catch {}
  }
  return metaCache;
}

const saveMetaCache = () => writeJson(userFile('local-cache.json'), metaCache);

async function walk(dir, out) {
  let entries;
  try { entries = await fs.promises.readdir(dir, { withFileTypes: true }); } catch { return; }

  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) await walk(full, out);
    else if (isAudio(entry.name)) out.push(full);
  }
}

function saveCover(id, data) {
  const image = nativeImage.createFromBuffer(Buffer.from(data));
  if (image.isEmpty()) return null;

  fs.mkdirSync(coverDir(), { recursive: true });
  const key = id.slice(6);
  const width = image.getSize().width;

  fs.writeFileSync(path.join(coverDir(), `${key}.jpg`), image.resize({ width: Math.min(400, width), quality: 'good' }).toJPEG(82));
  fs.writeFileSync(path.join(coverDir(), `${key}-s.jpg`), image.resize({ width: Math.min(120, width), quality: 'good' }).toJPEG(80));

  return { artwork: `media://art/${key}.jpg`, thumb: `media://art/${key}-s.jpg` };
}

function fallbackTrack(file, id) {
  const base = path.basename(file, path.extname(file));
  const named = /^(.+?)\s+-\s+(.+)$/.exec(base);

  return {
    id,
    source: 'local',
    title: named ? named[2] : base,
    artist: named ? named[1] : 'Unknown artist',
    album: '',
    folder: path.basename(path.dirname(file)),
    artwork: '',
    duration: 0,
    streamable: true,
  };
}

async function readLocalTrack(file) {
  const cache = loadMetaCache();
  const stat = await fs.promises.stat(file);
  const id = idForPath(file);
  const lrc = file.replace(/\.[^.]+$/, '.lrc');
  const lrcPath = fs.existsSync(lrc) ? lrc : null;

  const hit = cache[file];
  if (hit && hit.mtime === stat.mtimeMs) {
    localPaths.set(id, file);
    return { ...hit.track, lrcPath };
  }

  const track = fallbackTrack(file, id);
  try {
    const mm = await import('music-metadata');
    const meta = await mm.parseFile(file, { skipPostHeaders: true });

    track.title = meta.common.title || track.title;
    track.artist = meta.common.artist || meta.common.albumartist || track.artist;
    track.album = meta.common.album || '';
    track.duration = Math.round((meta.format.duration || 0) * 1000);

    const picture = meta.common.picture?.[0];
    if (picture) Object.assign(track, saveCover(id, picture.data) || {});
  } catch {}

  cache[file] = { mtime: stat.mtimeMs, track };
  localPaths.set(id, file);
  return { ...track, lrcPath };
}

async function collectFiles(paths) {
  const files = [];
  for (const target of paths) {
    try {
      const stat = await fs.promises.stat(target);
      if (stat.isDirectory()) await walk(target, files);
      else if (isAudio(target)) files.push(target);
    } catch {}
  }
  return [...new Set(files)];
}

async function scanPaths(paths) {
  const files = await collectFiles(paths);
  const tracks = [];
  let next = 0;

  const worker = async () => {
    while (next < files.length) {
      const file = files[next++];
      try { tracks.push(await readLocalTrack(file)); } catch {}
    }
  };
  await Promise.all(Array.from({ length: SCAN_WORKERS }, worker));

  saveMetaCache();
  return tracks.sort((a, b) => a.artist.localeCompare(b.artist) || a.title.localeCompare(b.title));
}

module.exports = { AUDIO_EXT, localPaths, coverDir, scanPaths };
