const { parseTitle } = require('./parse');

const ENTITIES = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };
const UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36';

const ascii = (s) => s.normalize('NFKD').replace(/[̀-ͯ]/g, '');

function slug(artist, title) {
  const line = `${artist}-${title}`
    .replace(/[,._@!#%^*+:;'()[\]?]/g, '')
    .replace(/[\s/]/g, '-')
    .replace(/-?&-?/g, '-and-')
    .replace(/-+/g, '-');
  return encodeURIComponent(ascii(line)).replace(/%2F/gi, '-');
}

function decode(text) {
  return text.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (m, e) => {
    if (e[0] === '#') return String.fromCodePoint(e[1].toLowerCase() === 'x' ? parseInt(e.slice(2), 16) : parseInt(e.slice(1), 10));
    return ENTITIES[e.toLowerCase()] ?? m;
  });
}

function dropBlocks(html, openRe) {
  let out = html;
  let m;
  while ((m = openRe.exec(out))) {
    let depth = 1;
    const tags = /<(\/?)div\b[^>]*>/g;
    tags.lastIndex = m.index + m[0].length;
    let t;
    while (depth && (t = tags.exec(out))) depth += t[1] ? -1 : 1;
    out = out.slice(0, m.index) + out.slice(t ? tags.lastIndex : out.length);
    openRe.lastIndex = 0;
  }
  return out;
}

// each lyrics block is a div with nested divs, so walk to the matching close tag
function containers(html) {
  const out = [];
  const open = /<div[^>]*data-lyrics-container="true"[^>]*>/g;
  let m;
  while ((m = open.exec(html))) {
    let depth = 1;
    const tags = /<(\/?)div\b[^>]*>/g;
    tags.lastIndex = open.lastIndex;
    let t;
    while (depth && (t = tags.exec(html))) depth += t[1] ? -1 : 1;
    out.push(dropBlocks(html.slice(open.lastIndex, t ? t.index : html.length), /<div[^>]*data-exclude-from-selection="true"[^>]*>/g));
    open.lastIndex = t ? tags.lastIndex : html.length;
  }
  return out;
}

const JUNK = /^(\d+\s+contributors?|.*\blyrics|embed|more on genius|you might also like)$/i;

function clean(chunks) {
  const text = decode(chunks.map((c) => c.replace(/<br\s*\/?>/gi, '\n').replace(/<[^>]+>/g, '')).join('\n'))
    .replace(/\[[^\]]*\]/g, '');

  const lines = text.split('\n').map((l) => l.trim()).filter((l) => l && !JUNK.test(l) && !(l.length < 2 && !/[a-z]/i.test(l)));
  while (lines.length && lines[0].length < 3) lines.shift();
  while (lines.length && lines[lines.length - 1].length < 3) lines.pop();
  return lines.join('\n');
}

async function fetchPage(who, song) {
  try {
    const res = await fetch(`https://genius.com/${slug(who, song)}-lyrics`, { signal: AbortSignal.timeout(10000), headers: { 'User-Agent': UA } });
    if (!res.ok) return null;
    const lyrics = clean(containers(await res.text()));
    return lyrics.length > 10 ? lyrics : null;
  } catch {
    return null;
  }
}

async function geniusLyrics(title, artist) {
  const { tracks, artists } = parseTitle(title, artist);
  const song = (tracks[0] || title).split(/\(?feat\./i)[0].trim();
  if (!song) return null;

  for (const name of artists) {
    const who = name.split(/feat[.:]/i)[0].trim();
    const found = who && await fetchPage(who, song);
    if (found) return found;
  }
  return null;
}

module.exports = { geniusLyrics };
