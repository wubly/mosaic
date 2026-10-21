const fs = require('fs');
const path = require('path');
const { userFile, readJson, writeJson } = require('../core/storage');
const { sleep } = require('../core/util');
const { readConfig } = require('../core/storage');
const { geniusLyrics } = require('./genius');
const { lrclib, failureCount } = require('./lrclib');
const { localPaths } = require('../library/scanner');
const { norm, parseTitle, parseLrc, hasWordStamps, kindOf, shapeLyrics, emptyLyrics } = require('./parse');

const autoCache = new Map();
const candidateStore = new Map();

const choicesFile = () => userFile('lyrics-choices.json');
const readChoices = () => readJson(choicesFile(), {});

const hasLyrics = (record) => record && (record.syncedLyrics || record.plainLyrics);
const query = (params) => new URLSearchParams(params).toString();

function sidecar(lrcPath) {
  if (!lrcPath) return null;
  try {
    const lines = parseLrc(fs.readFileSync(lrcPath, 'utf8'));
    return lines.length ? lines : null;
  } catch {
    return null;
  }
}

async function embedded(id) {
  const file = typeof id === 'string' && id.startsWith('local:') && localPaths.get(id);
  if (!file) return null;

  try {
    const mm = await import('music-metadata');
    const meta = await mm.parseFile(file, { skipPostHeaders: true });
    const tags = meta.common.lyrics || [];

    const timed = tags.find((t) => t.syncText?.length && t.timeStampFormat === 2);
    if (timed) {
      const synced = timed.syncText.filter((l) => l.text?.trim()).map((l) => ({ t: (l.timestamp ?? 0) / 1000, text: l.text.trim() }));
      if (synced.length) return { synced, plain: '', kind: 'synced' };
    }

    const text = tags.map((t) => t.text).find((t) => t?.trim());
    if (!text) return null;

    const lines = parseLrc(text);
    if (lines.length) return { synced: lines, plain: '', kind: lines.some((l) => l.words) ? 'word' : 'synced' };
    return { synced: [], plain: text.trim(), kind: 'plain' };
  } catch {
    return null;
  }
}

function lyricsPlan(title, artist, duration) {
  const dur = Math.round(duration / 1000);
  const { tracks, artists } = parseTitle(title, artist);

  const pairs = tracks.flatMap((track) => artists.map((who) => [who, track])).slice(0, 8);
  const exact = pairs.map(([who, track]) => `get?${query({ artist_name: who, track_name: track, duration: dur })}`);

  const fuzzy = [
    ...artists.slice(0, 2).map((who) => `search?${query({ track_name: tracks[0], artist_name: who })}`),
    `search?${query({ track_name: tracks[0] })}`,
    `search?${query({ q: `${tracks[0]} ${artists[0] || ''}`.trim() })}`,
    ...(tracks[1] ? [`search?${query({ q: `${tracks[1]} ${artists[0] || ''}`.trim() })}`] : []),
  ];

  return { dur, tracks, artists, exact, fuzzy };
}

async function autoLyrics({ title, artist, duration }) {
  const { dur, tracks, artists, exact, fuzzy } = lyricsPlan(title, artist, duration);
  const diff = (record) => Math.abs((record.duration ?? 0) - dur);

  const pick = (records) => {
    const wantTrack = new Set(tracks.map(norm));
    const wantArtist = artists.map(norm);

    const acceptable = records.filter(hasLyrics).filter((record) => {
      const sameSong = wantTrack.has(norm(record.trackName || ''));
      const sameArtist = wantArtist.some((w) => w && (norm(record.artistName || '').includes(w) || w.includes(norm(record.artistName || ''))));
      return diff(record) <= 8 || (sameSong && sameArtist && diff(record) <= 45);
    });

    acceptable.sort((a, b) => (b.syncedLyrics ? 1 : 0) - (a.syncedLyrics ? 1 : 0) || diff(a) - diff(b));
    return acceptable[0] || null;
  };

  let found = pick(await Promise.all(exact.map((q) => lrclib(q, 2))));
  if (!found) {
    const lists = await Promise.all(fuzzy.map((q) => lrclib(q, 2)));
    found = pick(lists.flatMap((list) => (Array.isArray(list) ? list : [])));
  }
  return found;
}

async function getLyrics(args) {
  const { id, lrcPath } = args;

  const chosen = readChoices()[id];
  if (chosen) return shapeLyrics(chosen, 'custom');

  const own = sidecar(lrcPath);
  if (own) {
    return {
      synced: own,
      plain: '',
      meta: { origin: 'file', id: 'file', title: path.basename(lrcPath), artist: '', album: '', kind: own.some((l) => l.words) ? 'word' : 'synced' },
    };
  }

  if (autoCache.has(id)) return autoCache.get(id);

  const tagged = await embedded(id);
  if (tagged) {
    const result = { synced: tagged.synced, plain: tagged.plain, meta: { origin: 'file', id: 'embedded', title: 'Embedded in file', artist: '', album: '', kind: tagged.kind } };
    autoCache.set(id, result);
    return result;
  }

  const failuresBefore = failureCount();
  let found = await autoLyrics(args);
  if (!found && failureCount() > failuresBefore) {
    await sleep(1500);
    found = await autoLyrics(args);
  }

  if (!found && readConfig().genius) {
    const plain = await geniusLyrics(args.title, args.artist);
    if (plain) {
      const result = { synced: [], plain, meta: { origin: 'genius', id: 'genius', title: args.title, artist: args.artist, album: '', kind: 'plain' } };
      autoCache.set(id, result);
      return result;
    }
  }

  if (!found) return emptyLyrics();
  const result = shapeLyrics(found);
  autoCache.set(id, result);
  return result;
}

async function getCandidates({ id, title, artist, duration, query: typedQuery, lrcPath }) {
  const { dur, tracks, artists, exact, fuzzy } = lyricsPlan(title, artist, duration);
  const typed = (typedQuery || '').trim();

  const requests = typed
    ? [`search?${query({ q: typed })}`, `search?${query({ track_name: typed })}`]
    : [...exact, ...fuzzy];

  const ask = async () =>
    (await Promise.all(requests.map((q) => lrclib(q, 2)))).flatMap((r) => (Array.isArray(r) ? r : r ? [r] : []));

  const failuresBefore = failureCount();
  let results = await ask();
  if (!results.length && failureCount() > failuresBefore) {
    await sleep(1500);
    results = await ask();
  }

  const unique = new Map();
  for (const record of results) {
    if (record?.id && (record.syncedLyrics || record.plainLyrics || record.instrumental) && !unique.has(record.id)) unique.set(record.id, record);
  }

  const score = (r) => Math.min(Math.abs((r.duration ?? 0) - dur), 60) - (r.syncedLyrics ? 12 : 0) - (hasWordStamps(r.syncedLyrics) ? 6 : 0);
  const ranked = [...unique.values()].sort((a, b) => score(a) - score(b)).slice(0, 30);
  for (const record of ranked) candidateStore.set(record.id, record);

  const list = ranked.map((r) => ({
    id: r.id,
    title: r.trackName || '',
    artist: r.artistName || '',
    album: r.albumName || '',
    duration: Math.round(r.duration ?? 0),
    diff: Math.round((r.duration ?? 0) - dur),
    kind: kindOf(r),
  }));

  const own = !typed ? sidecar(lrcPath) : null;
  if (own) {
    list.unshift({ id: 'file', title: path.basename(lrcPath), artist: 'Next to your file', album: '', duration: 0, diff: 0, kind: own.some((l) => l.words) ? 'word' : 'synced' });
  }

  return {
    list,
    chosen: readChoices()[id]?.id ?? null,
    query: `${artists[0] ? artists[0] + ' ' : ''}${tracks[0] || ''}`.trim(),
  };
}

async function selectLyrics(args) {
  const { id, candId } = args;
  const choices = readChoices();

  if (candId && candId !== 'file' && candidateStore.has(candId)) {
    const r = candidateStore.get(candId);
    choices[id] = {
      id: r.id,
      trackName: r.trackName,
      artistName: r.artistName,
      albumName: r.albumName,
      duration: r.duration,
      instrumental: r.instrumental,
      syncedLyrics: r.syncedLyrics,
      plainLyrics: r.plainLyrics,
    };
    writeJson(choicesFile(), choices);
    return shapeLyrics(choices[id], 'custom');
  }

  delete choices[id];
  writeJson(choicesFile(), choices);
  return getLyrics(args);
}

module.exports = { getLyrics, getCandidates, selectLyrics };
