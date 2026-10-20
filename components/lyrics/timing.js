export const clamp01 = (n) => Math.min(1, Math.max(0, n));

export function buildTimings(lines) {
  return lines.map((line, i) => {
    const next = lines[i + 1]?.t ?? line.t + 6;

    if (line.words?.length) {
      return line.words.map((word, k) => ({
        w: word.w,
        s: word.t,
        e: line.words[k + 1]?.t ?? Math.min(next, word.t + 0.8),
      }));
    }

    const words = line.text.split(' ');
    const singingTime = Math.min(Math.max(next - line.t - 0.15, 0.4), Math.max(1, line.text.length / 9));
    const weight = words.reduce((sum, w) => sum + w.length + 1, 0);

    let at = line.t;
    return words.map((w) => {
      const span = ((w.length + 1) / weight) * singingTime;
      const timing = { w, s: at, e: at + span };
      at += span;
      return timing;
    });
  });
}
