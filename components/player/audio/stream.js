import Hls from 'hls.js';

export async function resolveSource(track) {
  if (track.source === 'local') return { url: `media://file/${encodeURIComponent(track.id)}`, type: 'file' };
  return window.mosaic.getStream(track.id);
}

export function attachStream({ audio, hlsRef, url, type, onReady, onFatal }) {
  if (type === 'hls' && Hls.isSupported()) {
    const hls = new Hls({ maxBufferLength: 20, maxMaxBufferLength: 30, backBufferLength: 10, enableWorker: false });
    hlsRef.current = hls;
    hls.loadSource(url);
    hls.attachMedia(audio);
    hls.on(Hls.Events.MANIFEST_PARSED, onReady);
    hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) onFatal(); });
    return;
  }
  audio.src = url;
  audio.addEventListener('loadedmetadata', onReady, { once: true });
}

export function detachStream(hlsRef) {
  hlsRef.current?.destroy();
  hlsRef.current = null;
}
