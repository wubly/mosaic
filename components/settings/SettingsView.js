'use client';
import { useEffect, useState } from 'react';
import { MdRefresh, MdDelete, MdRestartAlt } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import { useToast } from '@/components/ui/Toast';
import PageHeader from '@/components/ui/PageHeader';
import Switch from '@/components/ui/Switch';
import { useSpotify } from '@/components/spotify/SpotifyProvider';
import { useYoutube } from '@/components/youtube/YoutubeProvider';
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
  const [genius, setGenius] = useState(false);
  const sp = useSpotify();
  const [clientId, setClientId] = useState('');
  const [spBusy, setSpBusy] = useState(false);
  useEffect(() => { if (sp.status) setClientId(sp.status.clientId || ''); }, [sp.status?.clientId]);
  const spConnect = async () => {
    setSpBusy(true);
    try {
      if (clientId.trim() !== (sp.status?.clientId || '')) await window.mosaic.spSetClientId(clientId);
      await window.mosaic.spConnect();
      toast('Spotify connected');
    } catch (e) { toast(e.message.replace(/^Error invoking remote method '[^']+': (Error: )?/, '')); }
    await sp.refresh().catch(() => {});
    setSpBusy(false);
  };
  const spDisconnect = async () => { await window.mosaic.spDisconnect(); await sp.refresh(); };
  const yt = useYoutube();
  const [ytId, setYtId] = useState('');
  const [ytSecret, setYtSecret] = useState('');
  const [ytBusy, setYtBusy] = useState(false);
  useEffect(() => { if (yt.status) setYtId(yt.status.clientId || ''); }, [yt.status?.clientId]);
  const ytConnect = async () => {
    setYtBusy(true);
    try {
      await window.mosaic.ytSetCredentials(ytId, ytSecret);
      await window.mosaic.ytConnect();
      toast('YouTube connected');
    } catch (e) { toast(e.message.replace(/^Error invoking remote method '[^']+': (Error: )?/, '')); }
    await yt.refresh().catch(() => {});
    setYtBusy(false);
  };
  const ytDisconnect = async () => { await window.mosaic.ytDisconnect(); await yt.refresh(); };
  const [update, setUpdate] = useState(null);
  const [checking, setChecking] = useState(false);
  useEffect(() => { window.mosaic.getUpdate().then(setUpdate); }, []);
  const checkUpdate = async () => { setChecking(true); setUpdate(await window.mosaic.checkUpdate()); setChecking(false); };
  useEffect(() => { window.mosaic.getGenius().then(setGenius); }, []);
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
        <div className="set-group">
          <h3>Updates</h3>
          <div className={`set-card ${update?.available ? 'update-ready' : ''}`}>
            <div className="switch-row static">
              <span>
                {update?.available
                  ? <><b>Mosaic {update.latest} is ready</b><em>You’re on {update.current}. The new version is a download away; nothing is installed automatically.</em></>
                  : <><b>Mosaic {update?.current ?? ''}</b><em>{update?.error ? `Couldn’t check: ${update.error}` : update?.checkedAt ? 'You’re up to date.' : 'Checking for updates…'}</em></>}
              </span>
              {update?.available
                ? <button className="primary" onClick={() => window.mosaic.openUpdate()}>Update</button>
                : <button className="tonal" disabled={checking} onClick={checkUpdate}>{checking ? 'Checking…' : 'Check now'}</button>}
            </div>
          </div>
        </div>

        <div className="set-group">
          <h3>Playback</h3>
          <div className="set-card">
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
          <span><b>Skip silence at start and end</b><em>Jump past long silent intros and outros, like the blank first minute some SoundCloud uploads use to dodge copyright. Never touches the middle of a song.</em></span>
          <Switch on={s.skipSilence} onChange={(v) => lib.setSetting('skipSilence', v)} />
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
          </div>
        </div>
        <div className="set-group">
          <h3>Discord</h3>
          <div className="set-card">
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
          </div>
        </div>
        <div className="set-group">
          <h3>Beta</h3>
          <div className="set-card">
            <label className="switch-row">
              <span><b>Beta features</b><em>Early features that may be rough or break. Turn this on to see them below.</em></span>
              <Switch on={sp.beta} onChange={sp.setBeta} />
            </label>
            {sp.beta && (
              <>
                <label className="switch-row">
                  <span>
                    <b>Genius lyrics</b>
                    <em>When LRCLIB has nothing, scrape plain lyrics from genius.com. No timing, and it can break whenever Genius changes its pages.</em>
                  </span>
                  <Switch on={genius} onChange={(v) => { setGenius(v); window.mosaic.setGenius(v); }} />
                </label>
                <div className="sp-beta">
                  <span>
                    <b>Spotify</b>
                    <em>
                      {sp.status?.connected
                        ? <>Connected as {sp.status.user}{sp.status.premium ? '' : '. Playing inside Mosaic needs Spotify Premium.'}</>
                        : 'Search and play Spotify tracks inside Mosaic. Needs Spotify Premium and your own client ID.'}
                    </em>
                  </span>
                  {sp.status?.connected ? (
                    <button className="tonal" onClick={spDisconnect}>Disconnect</button>
                  ) : (
                    <button className="primary" disabled={spBusy || !clientId.trim()} onClick={spConnect}>{spBusy ? 'Waiting for Spotify…' : 'Connect'}</button>
                  )}
                </div>
                {!sp.status?.connected && (
                  <div className="sp-setup">
                    <input className="set-input" placeholder="Spotify client ID" value={clientId} onChange={(e) => setClientId(e.target.value)} spellCheck={false} />
                    <p className="muted small">
                      Make an app at <a href="https://developer.spotify.com/dashboard" onClick={(e) => { e.preventDefault(); window.mosaic.openExternal('https://developer.spotify.com/dashboard'); }}>developer.spotify.com/dashboard</a>,
                      add <code>{sp.status?.redirect || 'http://127.0.0.1:8898/callback'}</code> as a redirect URI, tick “Web Playback SDK”, then paste the client ID here.
                    </p>
                  </div>
                )}
                <div className="sp-beta">
                  <span>
                    <b>YouTube</b>
                    <em>
                      {yt.status?.connected
                        ? `Connected as ${yt.status.user}. Plays as music; use the video button in the player bar to show the picture.`
                        : 'Search YouTube music and play your liked videos. Needs your own Google client ID, and the API has a daily search limit.'}
                    </em>
                  </span>
                  {yt.status?.connected ? (
                    <button className="tonal" onClick={ytDisconnect}>Disconnect</button>
                  ) : (
                    <button className="primary" disabled={ytBusy || !ytId.trim()} onClick={ytConnect}>{ytBusy ? 'Waiting for Google…' : 'Connect'}</button>
                  )}
                </div>
                {!yt.status?.connected && (
                  <div className="sp-setup">
                    <input className="set-input" placeholder="Google client ID" value={ytId} onChange={(e) => setYtId(e.target.value)} spellCheck={false} />
                    <input className="set-input" placeholder={yt.status?.hasSecret ? 'Client secret (saved)' : 'Client secret'} value={ytSecret} onChange={(e) => setYtSecret(e.target.value)} spellCheck={false} />
                    <p className="muted small">
                      In <a href="https://console.cloud.google.com/apis/credentials" onClick={(e) => { e.preventDefault(); window.mosaic.openExternal('https://console.cloud.google.com/apis/credentials'); }}>Google Cloud</a>: enable the YouTube Data API v3,
                      add yourself as a test user on the OAuth consent screen, then create an OAuth client of type “Desktop app” and paste its ID and secret here.
                    </p>
                  </div>
                )}
              </>
            )}
          </div>
        </div>
        <div className="set-group">
          <h3>Performance</h3>
          <div className="set-card">
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
          </div>
        </div>
        <div className="set-group">
          <h3>Library</h3>
          <div className="set-card">
        <div className="btn-row">
          <button className="tonal" onClick={() => { lib.rescan(); toast('Rescanning your music…'); }}><MdRefresh size={20} /> Rescan local music</button>
          <button className="tonal" onClick={() => { lib.clearHistory(); toast('History and stats cleared'); }}><MdDelete size={20} /> Clear history & stats</button>
          <button className="tonal" onClick={onReset}><MdRestartAlt size={20} /> Restart onboarding</button>
        </div>
          </div>
        </div>
        <div className="set-group">
          <h3>Keyboard shortcuts</h3>
          <div className="set-card">
        <dl className="keys">
          {SHORTCUTS.map(([k, d]) => (
            <div key={k}><dt>{k.split(' + ').map((x, i) => <span key={i}><kbd>{x}</kbd>{i < k.split(' + ').length - 1 ? ' + ' : ''}</span>)}</dt><dd>{d}</dd></div>
          ))}
        </dl>
          </div>
        </div>
        <div className="set-group">
          <h3>About</h3>
          <div className="set-card">
        <p className="muted">Mosaic {update?.current ?? ''} · lyrics by <a href="https://lrclib.net" onClick={(e) => { e.preventDefault(); window.mosaic.openExternal('https://lrclib.net'); }}>LRCLIB</a> · colour from your artwork via Material You.</p>
          </div>
        </div>
      </section>
    </div>
  );
}
