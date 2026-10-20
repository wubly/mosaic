export const fmt = (ms) => {
  const seconds = Math.floor((ms || 0) / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
};

export const artworkOf = (track) => track.thumb || track.artwork || '';

export const artStyle = (track) => {
  const src = artworkOf(track);
  return src ? { backgroundImage: `url(${src})` } : undefined;
};
