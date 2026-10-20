const uniq = (list) => [...new Set(list.map((x) => x.trim()).filter(Boolean))];

const norm = (x) => x.toLowerCase().normalize('NFKD').replace(/[̀-ͯ]/g, '').replace(/[^a-z0-9]+/g, ' ').trim();

function parseTitle(title, uploader) {
  const clean = (x) =>
    x
      .replace(/\[[^\]]*\]|\([^)]*\)|\{[^}]*\}|\|[^|]*\|/g, ' ')
      .replace(/\s+(ft|feat|featuring|with)\.?\s.*$/i, '')
      .replace(/\s+(official|lyric|lyrics|audio|video|visualizer|music video)\b.*$/i, '')
      .replace(/\s+/g, ' ')
      .trim();

  const split = title.split(/\s+[-–—]\s+/);
  const tracks = [];
  const artists = [];

  if (split.length > 1) {
    const prefix = clean(split[0]);
    artists.push(prefix, ...prefix.split(/\s*(?:,|&|\bx\b|\band\b)\s*/i));
    tracks.push(clean(split.slice(1).join(' - ')), clean(split[1]));
  }
  tracks.push(clean(title));
  artists.push(clean(uploader || ''));

  return { tracks: uniq(tracks).slice(0, 3), artists: uniq(artists).slice(0, 3) };
}

const stamp = (m) => +m[1] * 60 + parseFloat(m[2]);

function parseLrc(lrc) {
  const lines = [];

  for (const raw of lrc.split('\n')) {
    const stamps = [...raw.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    if (!stamps.length) continue;

    const body = raw.replace(/\[\d+:\d+(?:\.\d+)?\]/g, '').trim();
    const text = body.replace(/<\d+:\d+(?:\.\d+)?>/g, '').replace(/\s+/g, ' ').trim();
    if (!text) continue;

    let words = null;
    const wordStamps = [...body.matchAll(/<(\d+):(\d+(?:\.\d+)?)>\s*([^<]*)/g)];
    if (wordStamps.length > 1) {
      words = wordStamps.map((m) => ({ t: stamp(m), w: m[3].trim() })).filter((w) => w.w);
    }
    for (const m of stamps) lines.push({ t: stamp(m), text, ...(words ? { words } : {}) });
  }
  return lines.sort((a, b) => a.t - b.t);
}

const hasWordStamps = (text) => /<\d+:\d+(?:\.\d+)?>/.test(text || '');

const kindOf = (record) => {
  if (record.instrumental) return 'instrumental';
  if (record.syncedLyrics) return hasWordStamps(record.syncedLyrics) ? 'word' : 'synced';
  return record.plainLyrics ? 'plain' : 'none';
};

const emptyLyrics = () => ({ synced: [], plain: '', meta: { origin: 'none', id: null, title: '', artist: '', album: '', kind: 'none' } });

const shapeLyrics = (record, origin = 'lrclib') => ({
  synced: record.syncedLyrics ? parseLrc(record.syncedLyrics) : [],
  plain: record.plainLyrics || '',
  meta: {
    origin,
    id: record.id ?? null,
    title: record.trackName || '',
    artist: record.artistName || '',
    album: record.albumName || '',
    kind: kindOf(record),
  },
});

module.exports = { uniq, norm, parseTitle, parseLrc, hasWordStamps, kindOf, shapeLyrics, emptyLyrics };
