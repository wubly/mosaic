const MIN_START = 8;
const MIN_END = 5;
const RUN = 3;

async function waveformSilence(track) {
  const url = track.waveform_url?.replace(/\.png$/, '.json');
  const length = (track.duration || 0) / 1000;
  if (!url || !length) return null;

  try {
    const res = await fetch(url, { signal: AbortSignal.timeout(8000) });
    if (!res.ok) return null;
    const { samples } = await res.json();
    if (!Array.isArray(samples) || samples.length < 10) return null;

    const limit = Math.max(2, Math.max(...samples) * 0.03);
    const loud = (i) => samples[i] > limit;
    const solid = (i) => Array.from({ length: RUN }, (_, k) => i + k).every((j) => j < samples.length && loud(j));
    const secondsPer = length / samples.length;

    let first = samples.findIndex((_, i) => solid(i));
    if (first < 0) return null;

    let last = samples.length - 1;
    while (last > first && !loud(last)) last--;

    const start = first * secondsPer;
    const end = (last + 1) * secondsPer;
    return {
      start: start >= MIN_START ? Math.max(0, start - 0.3) : 0,
      end: length - end >= MIN_END ? end + 0.3 : null,
    };
  } catch {
    return null;
  }
}

module.exports = { waveformSilence };
