const fullStreams = (transcodings = []) =>
  transcodings
    .filter((t) => !t.snipped && ['hls', 'progressive'].includes(t.format.protocol))
    .sort((a, b) => (a.format.protocol === 'hls' ? -1 : 0) - (b.format.protocol === 'hls' ? -1 : 0));

const slimTrack = (t) => {
  const art = t.artwork_url || t.user?.avatar_url || '';
  return {
    id: t.id,
    source: 'soundcloud',
    title: t.title,
    artist: t.user?.username ?? 'Unknown',
    artwork: art.replace('-large', '-t500x500'),
    thumb: art.replace('-large', '-t300x300'),
    duration: t.duration,
    permalink: t.permalink_url,
    streamable: t.streamable !== false && t.policy !== 'BLOCK' && fullStreams(t.media?.transcodings).length > 0,
  };
};

const playable = (list) => (list ?? []).filter((t) => t && t.id && t.title).map(slimTrack).filter((t) => t.streamable);

module.exports = { fullStreams, slimTrack, playable };
