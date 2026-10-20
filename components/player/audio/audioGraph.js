const EQ_BANDS = [60, 230, 910, 3600, 14000];
const EQ_TYPES = ['lowshelf', 'peaking', 'peaking', 'peaking', 'highshelf'];

export function createAudioGraph(audio) {
  const AudioContextClass = window.AudioContext || window.webkitAudioContext;
  const ctx = new AudioContextClass();
  const source = ctx.createMediaElementSource(audio);

  const filters = EQ_BANDS.map((frequency, i) => {
    const filter = ctx.createBiquadFilter();
    filter.type = EQ_TYPES[i];
    filter.frequency.value = frequency;
    filter.Q.value = 1;
    filter.gain.value = 0;
    return filter;
  });

  const analyser = ctx.createAnalyser();
  analyser.fftSize = 512;
  analyser.smoothingTimeConstant = 0.82;

  const leveler = ctx.createDynamicsCompressor();
  leveler.threshold.value = 0;
  leveler.knee.value = 0;
  leveler.ratio.value = 1;
  leveler.attack.value = 0.01;
  leveler.release.value = 0.25;

  let node = source;
  for (const filter of filters) {
    node.connect(filter);
    node = filter;
  }
  node.connect(leveler);
  leveler.connect(analyser);
  analyser.connect(ctx.destination);

  return { ctx, filters, analyser, leveler };
}
