const TALK = /\b(podcast|episode|ep\.?\s?\d+|interview|full (album|mixtape|show|set)|radio show|live ?stream|audiobook|asmr|sermon|mixtape)\b/i;
const MIN = 45_000;
const MAX = 12 * 60_000;

const clean = (t) => (t || '').toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();

export function fitsAlong(candidate, seed) {
  if (candidate.source === 'local') return true;
  const length = candidate.duration || 0;
  const max = Math.max(MAX, (seed?.duration || 0) * 1.5);

  if (length && (length < MIN || length > max)) return false;
  if (TALK.test(candidate.title) && !TALK.test(seed?.title || '')) return false;
  return !seed || clean(candidate.title) !== clean(seed.title) || candidate.id === seed.id;
}

export const filterAlong = (list, seed) => list.filter((t) => fitsAlong(t, seed));
