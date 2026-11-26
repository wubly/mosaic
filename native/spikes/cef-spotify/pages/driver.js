// Injected into open.spotify.com by the spike on every main-frame load. It only observes: it
// checks that the page can reach 127.0.0.1 (F7) and what Widevine/codecs report (F3/F5), and
// reports which now-playing selectors exist (design doc 6.2). It never clicks anything.
(() => {
  if (window.__mosaicSpike) return;
  window.__mosaicSpike = true;
  const BRIDGE = "http://127.0.0.1:8898/log";
  let reachable = null;

  function post(kind, data = {}) {
    const body = JSON.stringify({ kind, page: "web", path: location.pathname, ...data });
    return fetch(BRIDGE, { method: "POST", body, headers: { "Content-Type": "text/plain" } })
      .then(() => {
        if (reachable !== true) console.log("[spike] " + JSON.stringify({ kind: "bridge_reachable", ok: true }));
        reachable = true;
      })
      .catch((e) => {
        if (reachable !== false) console.log("[spike] " + JSON.stringify({ kind: "bridge_reachable", ok: false, error: String(e) }));
        reachable = false;
        console.log("[spike] " + body);
      });
  }

  const SELECTORS = {
    nowPlaying: '[data-testid="now-playing-widget"]',
    trackLink: '[data-testid="now-playing-widget"] a[href*="/track/"], [data-testid="context-item-link"]',
    playPause: '[data-testid="control-button-playpause"]',
    next: '[data-testid="control-button-skip-forward"]',
    position: '[data-testid="playback-position"]',
    duration: '[data-testid="playback-duration"]',
    progress: '[data-testid="playback-progressbar"] input[type="range"]',
    volume: '[data-testid="volume-bar"] input[type="range"]',
    userMenu: '[data-testid="user-widget-link"]',
    loginButton: '[data-testid="login-button"]',
    adMarker: '[data-testid="context-item-info-ad-subtitle"], [data-testid="ad-link"]',
  };

  async function capabilities() {
    const types = ['audio/mp4; codecs="mp4a.40.2"', 'audio/mp4; codecs="flac"', 'audio/webm; codecs="vorbis"'];
    const mse = Object.fromEntries(types.map((t) => [t, window.MediaSource ? MediaSource.isTypeSupported(t) : null]));
    let widevine;
    try {
      const a = await navigator.requestMediaKeySystemAccess("com.widevine.alpha", [{
        initDataTypes: ["cenc"], audioCapabilities: types.map((contentType) => ({ contentType })),
      }]);
      await a.createMediaKeys();
      widevine = { ok: true };
    } catch (e) {
      widevine = { ok: false, error: String(e) };
    }
    post("web_capabilities", { mse, widevine });
  }

  function sample() {
    const found = {};
    for (const [name, sel] of Object.entries(SELECTORS)) found[name] = !!document.querySelector(sel);
    const q = (sel) => document.querySelector(sel);
    const link = q(SELECTORS.trackLink);
    const progress = q(SELECTORS.progress);
    post("web_state", {
      found,
      track: link ? link.getAttribute("href") : null,
      playPauseLabel: q(SELECTORS.playPause) ? q(SELECTORS.playPause).getAttribute("aria-label") : null,
      position: q(SELECTORS.position) ? q(SELECTORS.position).textContent : null,
      duration: q(SELECTORS.duration) ? q(SELECTORS.duration).textContent : null,
      progress: progress ? [progress.value, progress.max] : null,
    });
  }

  capabilities();
  setInterval(sample, 2000);
})();
