const LISTENING = 2;
const DISPLAY_STATE = 1;

const clip = (value, fallback) => {
  const text = String(value || fallback || '').replace(/\s+/g, ' ').trim().slice(0, 120);
  return text.length >= 2 ? text : `${text || fallback || '—'}  `;
};

function buildActivity(payload) {
  const activity = {
    type: LISTENING,
    statusDisplayType: DISPLAY_STATE,
    details: clip(payload.title, 'Unknown track'),
    state: clip(payload.artist, 'Unknown artist'),
    largeImageText: clip(payload.album || payload.title, 'Mosaic'),
    instance: false,
  };

  if (/^https:\/\//.test(payload.artwork || '')) activity.largeImageKey = payload.artwork;

  if (payload.durationMs > 0 && payload.positionMs >= 0) {
    const start = Date.now() - payload.positionMs;
    activity.startTimestamp = Math.round(start);
    activity.endTimestamp = Math.round(start + payload.durationMs);
  }

  if (/^https:\/\/soundcloud\.com\//.test(payload.url || '')) {
    activity.buttons = [{ label: 'Listen on SoundCloud', url: payload.url }];
  }
  return activity;
}

module.exports = { buildActivity };
