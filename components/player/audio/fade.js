const running = new WeakMap();

export function cancelFade(audio) {
  const id = running.get(audio);
  if (id) cancelAnimationFrame(id);
  running.delete(audio);
}

export function rampVolume(audio, from, to, ms, done) {
  cancelFade(audio);
  const start = performance.now();
  audio.volume = from;
  const frame = (now) => {
    const t = Math.min(1, (now - start) / ms);
    audio.volume = Math.min(1, Math.max(0, from + (to - from) * t));
    if (t < 1) {
      running.set(audio, requestAnimationFrame(frame));
      return;
    }
    running.delete(audio);
    done?.();
  };
  running.set(audio, requestAnimationFrame(frame));
}
