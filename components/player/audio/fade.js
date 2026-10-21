const running = new WeakMap();

export function cancelFade(audio) {
  const id = running.get(audio);
  if (id) cancelAnimationFrame(id);
  running.delete(audio);
}

const EASE = {
  in: (t) => Math.sin(t * Math.PI / 2),
  out: (t) => 1 - Math.cos(t * Math.PI / 2),
};

export function rampVolume(audio, from, to, ms, done, curve) {
  const ease = EASE[curve] || ((t) => t);
  cancelFade(audio);
  const start = performance.now();
  audio.volume = from;
  const frame = (now) => {
    const t = Math.min(1, (now - start) / ms);
    audio.volume = Math.min(1, Math.max(0, from + (to - from) * ease(t)));
    if (t < 1) {
      running.set(audio, requestAnimationFrame(frame));
      return;
    }
    running.delete(audio);
    done?.();
  };
  running.set(audio, requestAnimationFrame(frame));
}
