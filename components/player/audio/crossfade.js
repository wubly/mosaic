import { resolveSource, attachStream, detachStream } from '@/components/player/audio/stream';
import { rampVolume } from '@/components/player/audio/fade';

export async function fadeOutGhost({ track, at, volume, speed, seconds }) {
  const ghost = new Audio();
  ghost.crossOrigin = 'anonymous';
  ghost.volume = 0;
  ghost.playbackRate = speed;
  const hlsRef = { current: null };
  const finish = () => {
    ghost.pause();
    detachStream(hlsRef);
    ghost.removeAttribute('src');
    ghost.load();
  };
  try {
    const { url, type } = await resolveSource(track);
    attachStream({
      audio: ghost,
      hlsRef,
      url,
      type,
      onReady: () => {
        ghost.currentTime = at;
        ghost.play().then(() => rampVolume(ghost, volume, 0, Math.max(300, (seconds - 0.2) * 1000), finish)).catch(finish);
      },
      onFatal: finish,
    });
  } catch {
    finish();
  }
}
