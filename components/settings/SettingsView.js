'use client';
import { useEffect, useState } from 'react';
import { MdRefresh, MdDelete, MdRestartAlt } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import PageHeader from '@/components/ui/PageHeader';
import Switch from '@/components/ui/Switch';
import WavySlider from '@/components/player/WavySlider';

export const SHORTCUTS = [
  ['Space', 'Play / pause'],
  ['← →', 'Seek 5 seconds'],
  ['Shift + ← →', 'Previous / next track'],
  ['↑ ↓', 'Volume'],
  ['M', 'Mute'],
  ['L', 'Like current track'],
  ['S / R', 'Shuffle / repeat'],
  ['Y', 'Toggle lyrics panel'],
  ['F', 'Immersive Now Playing'],
  ['I', 'Mini player (always on top)'],
  ['/', 'Focus search'],
  ['Ctrl + K', 'Command palette'],
  ['Media keys', 'Work system-wide, even when minimised'],
];

export default function SettingsView({ onReset }) {
  const lib = useData();
  const toast = useToast();
  const s = lib.settings;
  const [rpc, setRpc] = useState({ enabled: false, connected: false, user: null });
  const [perf, setPerf] = useState({ lowMemory: true, startedWith: true });
  const [tray, setTray] = useState(true);
  useEffect(() => { window.mosaic.getCloseToTray().then(setTray); }, []);
  useEffect(() => { window.mosaic.getPerf().then((p) => setPerf({ lowMemory: p.lowMemory, startedWith: p.lowMemory })); }, []);
  useEffect(() => {
    window.mosaic.presenceStatus().then(setRpc);
    return window.mosaic.onPresenceStatus(setRpc);
  }, []);

  return (
    <div className="library">
      <PageHeader title="Settings" />

      <section className="settings">
        <h3>Playback</h3>
        <label className="switch-row">
          <span><b>Autoplay similar tracks</b><em>When your queue ends, keep going with tracks SoundCloud thinks you’ll like.</em></span>
          <Switch on={s.autoplay} onChange={(v) => lib.setSetting('autoplay', v)} />
        </label>
        <div className="switch-row static">
          <span>
            <b>Crossfade</b>
            <em>{s.crossfade ? `Overlap the end of one track with the start of the next for ${s.crossfade} seconds.` : 'Off. Tracks play back to back.'}</em>
          </span>
          <div className="setting-slider">
            <WavySlider label="Crossfade seconds" wavy={false} value={s.crossfade || 0} max={12} step={1} onChange={(v) => lib.setSetting('crossfade', Math.round(v))} />
          </div>
        </div>
        <label className="switch-row">
          <span><b>Fade on pause and resume</b><em>Ease the volume instead of cutting it off.</em></span>
          <Switch on={s.fadePause} onChange={(v) => lib.setSetting('fadePause', v)} />
        </label>
        <label className="switch-row">
          <span><b>Level loudness</b><em>Gently compress loud peaks so quiet and loud tracks sit closer together.</em></span>
          <Switch on={s.leveling} onChange={(v) => lib.setSetting('leveling', v)} />
        </label>
        <label className="switch-row">
          <span><b>Show lyrics panel</b><em>Synced, word-by-word lyrics from LRCLIB beside your music.</em></span>
          <Switch on={s.lyricsOpen} onChange={(v) => lib.setSetting('lyricsOpen', v)} />
        </label>

        <label className="switch-row">
          <span><b>Keep playing in the tray</b><em>Closing the window hides Mosaic in the system tray so your music keeps going. Quit from the tray icon.</em></span>
          <Switch on={tray} onChange={(v) => { setTray(v); window.mosaic.setCloseToTray(v); }} />
        </label>

        <h3>Discord</h3>
        <label className="switch-row">
          <span>
            <b>Rich Presence</b>
            <em>
              Show what you are listening to on your Discord profile.{' '}
              {s.presence
                ? rpc.connected
                  ? <span className="ok">Connected{rpc.user ? ` as ${rpc.user}` : ''}.{rpc.error ? <span className="warn"> Last update failed: {rpc.error}</span> : null}</span>
                  : <span className="warn">Discord isn’t running — Mosaic keeps trying and connects automatically.</span>
                : null}
            </em>
          </span>
          <Switch on={s.presence} onChange={(v) => lib.setSetting('presence', v)} />
        </label>

        <h3>Performance</h3>
        <label className="switch-row">
          <span>
            <b>Low memory mode</b>
            <em>Software rendering and Chromium’s low-end mode: roughly 200 MB less memory with no extra CPU use here. Turn it off if animations ever feel choppy.</em>
          </span>
          <Switch on={perf.lowMemory} onChange={(v) => { setPerf((p) => ({ ...p, lowMemory: v })); window.mosaic.setLowMemory(v); }} />
        </label>
        {perf.lowMemory !== perf.startedWith && (
          <div className="btn-row" style={{ marginBottom: 8 }}>
            <button className="primary" onClick={() => window.mosaic.relaunch()}>Restart to apply</button>
          </div>
        )}

        <h3>Library</h3>
        <div className="btn-row">
          <button className="tonal" onClick={() => { lib.rescan(); toast('Rescanning your music…'); }}><MdRefresh size={20} /> Rescan local music</button>
          <button className="tonal" onClick={() => { lib.clearHistory(); toast('History and stats cleared'); }}><MdDelete size={20} /> Clear history & stats</button>
          <button className="tonal" onClick={onReset}><MdRestartAlt size={20} /> Restart onboarding</button>
        </div>

        <h3>Keyboard shortcuts</h3>
        <dl className="keys">
          {SHORTCUTS.map(([k, d]) => (
            <div key={k}><dt>{k.split(' + ').map((x, i) => <span key={i}><kbd>{x}</kbd>{i < k.split(' + ').length - 1 ? ' + ' : ''}</span>)}</dt><dd>{d}</dd></div>
          ))}
        </dl>

        <h3>About</h3>
        <p className="muted">Mosaic 0.2 · lyrics by <a href="https://lrclib.net" onClick={(e) => { e.preventDefault(); window.mosaic.openExternal('https://lrclib.net'); }}>LRCLIB</a> · colour from your artwork via Material You.</p>
      </section>
    </div>
  );
}
