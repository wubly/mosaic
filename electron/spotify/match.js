const { scGet } = require('../soundcloud/client');
const { playable } = require('../soundcloud/tracks');
const { norm } = require('../lyrics/parse');

const MIN_SCORE = 0.6;
const GOOD_ENOUGH = 0.8;
const VARIANTS = /\b(remix|rmx|slowed|sped|speed|fast|nightcore|cover|instrumental|reverb|8d|bass boosted|karaoke|mashup|edit|flip|bootleg|vip|acapella|a cappella|lofi|lo-fi|pitched|tiktok)\b/i;

const tokens = (text) => new Set(norm(text || '').split(' ').filter((w) => w.length > 1));

function jaccard(a, b) {
  if (!a.size || !b.size) return 0;
  let shared = 0;
  for (const word of a) if (b.has(word)) shared++;
  return shared / (a.size + b.size - shared);
}

const stripNoise = (text) =>
  text
    .replace(/\[[^\]]*\]|\([^)]*\)/g, ' ')
    .replace(/\s+-\s+(remaster(ed)?|single|radio edit|album version|live|mono|stereo).*$/i, '')
    .replace(/\s+/g, ' ')
    .trim();

function scoreCandidate(spotify, soundcloud) {
  const spotifyTitle = stripNoise(spotify.title);
  const artists = spotify.artist.split(/,|&| x |feat\.?/i).map((a) => a.trim()).filter(Boolean);

  const parts = soundcloud.title.split(/\s+[-–—]\s+/);
  const soundcloudTitle = stripNoise(parts.length > 1 ? parts.slice(1).join(' ') : soundcloud.title);

  const titleSim = Math.max(
    jaccard(tokens(spotifyTitle), tokens(soundcloudTitle)),
    jaccard(tokens(spotifyTitle), tokens(soundcloud.title)),
  );

  const haystack = tokens(`${soundcloud.title} ${soundcloud.artist}`);
  const artistSim = artists.length
    ? artists.filter((a) => [...tokens(a)].every((w) => haystack.has(w))).length / artists.length
    : 0;

  const seconds = Math.abs((soundcloud.duration || 0) - (spotify.duration || 0)) / 1000;
  const durationSim = !spotify.duration ? 0.5 : seconds <= 3 ? 1 : seconds <= 10 ? 0.6 : seconds <= 30 ? 0.25 : 0;

  let score = 0.5 * titleSim + 0.3 * artistSim + 0.2 * durationSim;
  if (VARIANTS.test(soundcloud.title) && !VARIANTS.test(spotify.title)) score -= 0.35;
  return score;
}

async function findMatch(spotify) {
  const artist = (spotify.artist || '').split(',')[0].trim();
  const title = stripNoise(spotify.title || '');
  let best = null;

  for (const q of [`${artist} ${title}`.trim(), title]) {
    try {
      const json = await scGet('https://api-v2.soundcloud.com/search/tracks', { q, limit: 12 });
      for (const track of playable(json.collection)) {
        const score = scoreCandidate(spotify, track);
        if (!best || score > best.score) best = { track, score };
      }
    } catch {}
    if (best && best.score >= GOOD_ENOUGH) break;
  }

  return best && best.score >= MIN_SCORE ? { track: best.track, score: +best.score.toFixed(2) } : null;
}

module.exports = { findMatch, scoreCandidate, stripNoise };
