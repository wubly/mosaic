// Run by the spike's renderer in every frame before the frame's own scripts (on_context_created),
// so it also sees the Web Playback SDK's iframe on sdk.scdn.co and open.spotify.com's player.
// Logs which formats and key systems the player checks and uses (design doc F5), and the
// file/licence requests it makes. Only observes; every call is passed through unchanged.
(() => {
  if (window.__mosaicInstrument) return;
  window.__mosaicInstrument = true;
  const origin = location.origin;
  // WebView2 and Firefox-family hosts have no console hook: those post to the server instead.
  const viaServer = /Edg\/|Firefox\//.test(navigator.userAgent);
  const rawFetch = window.fetch && window.fetch.bind(window);
  const log = (kind, data) => {
    const line = JSON.stringify({ kind, origin, ...data });
    console.log("[spike] " + line);
    // WebView2 has no console hook in the host: post to the spike's server (CORS-enabled /log).
    if (viaServer && rawFetch) {
      rawFetch("http://127.0.0.1:8898/log", { method: "POST", body: line, headers: { "Content-Type": "text/plain" } }).catch(() => {});
    }
  };

  if (window.MediaSource) {
    const isType = MediaSource.isTypeSupported.bind(MediaSource);
    MediaSource.isTypeSupported = (type) => {
      const ok = isType(type);
      log("fmt_isTypeSupported", { type, ok });
      return ok;
    };
    const addSb = MediaSource.prototype.addSourceBuffer;
    MediaSource.prototype.addSourceBuffer = function (type) {
      try {
        const sb = addSb.call(this, type);
        log("fmt_addSourceBuffer", { type, ok: true });
        return sb;
      } catch (e) {
        log("fmt_addSourceBuffer", { type, ok: false, error: String(e) });
        throw e;
      }
    };
  }

  const canPlay = HTMLMediaElement.prototype.canPlayType;
  HTMLMediaElement.prototype.canPlayType = function (type) {
    const result = canPlay.call(this, type);
    log("fmt_canPlayType", { type, result });
    return result;
  };

  if (navigator.requestMediaKeySystemAccess) {
    const rmksa = navigator.requestMediaKeySystemAccess.bind(navigator);
    navigator.requestMediaKeySystemAccess = async (keySystem, configs) => {
      const asked = (configs || []).flatMap((c) =>
        (c.audioCapabilities || []).map((a) => `${a.contentType} [${a.robustness || ""}]`));
      try {
        const access = await rmksa(keySystem, configs);
        const granted = (access.getConfiguration().audioCapabilities || []).map((a) => a.contentType);
        log("fmt_keySystem", { keySystem, asked, ok: true, granted });
        return access;
      } catch (e) {
        log("fmt_keySystem", { keySystem, asked, ok: false, error: String(e) });
        throw e;
      }
    };
  }

  // EME: key session messages and key statuses (is the licence usable?).
  if (window.MediaKeys) {
    const createSession = MediaKeys.prototype.createSession;
    MediaKeys.prototype.createSession = function (...args) {
      const session = createSession.apply(this, args);
      log("eme_session", { type: args[0] || "temporary" });
      session.addEventListener("message", (e) => log("eme_message", { messageType: e.messageType, bytes: e.message.byteLength }));
      session.addEventListener("keystatuseschange", () => {
        const statuses = [];
        session.keyStatuses.forEach((status) => statuses.push(status));
        log("eme_keystatus", { statuses });
      });
      const update = session.update.bind(session);
      session.update = (response) => update(response).then(
        () => log("eme_update", { ok: true, bytes: response.byteLength }),
        (e) => { log("eme_update", { ok: false, error: String(e) }); throw e; });
      return session;
    };
  }
  if (HTMLMediaElement.prototype.setMediaKeys) {
    const setMediaKeys = HTMLMediaElement.prototype.setMediaKeys;
    HTMLMediaElement.prototype.setMediaKeys = function (keys) {
      return setMediaKeys.call(this, keys).then(
        () => log("eme_setMediaKeys", { ok: true }),
        (e) => { log("eme_setMediaKeys", { ok: false, error: String(e) }); throw e; });
    };
  }

  // Media element events, attached the first time the player calls play().
  const watched = new WeakSet();
  const watch = (el) => {
    if (watched.has(el)) return;
    watched.add(el);
    for (const ev of ["playing", "waiting", "stalled", "pause", "ended", "error", "encrypted", "waitingforkey", "canplay", "loadedmetadata", "emptied", "suspend"]) {
      el.addEventListener(ev, () => log("media_" + ev, {
        t: Math.round(el.currentTime * 1000), ready: el.readyState, net: el.networkState,
        error: el.error ? `${el.error.code} ${el.error.message}` : undefined,
      }));
    }
    let last = -1;
    el.addEventListener("timeupdate", () => {
      const s = Math.floor(el.currentTime);
      if (s !== last && s % 5 === 0) log("media_timeupdate", { t: Math.round(el.currentTime * 1000) });
      last = s;
    });
  };
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function () {
    watch(this);
    const p = play.call(this);
    if (p && p.then) {
      p.then(() => log("media_play", { ok: true }), (e) => log("media_play", { ok: false, error: String(e) }));
    }
    return p;
  };

  // Requests that reveal the chosen file format: storage-resolve, CDN audio, licence.
  const interesting = /storage-resolve|audio-|\/audio\/|scdn\.co\/audio|widevine|license|playplay|manifest|track-playback|cdn/i;
  const short = (u) => String(u).replace(/[?#].*$/, "").slice(0, 180);
  const origFetch = window.fetch && window.fetch.bind(window);
  if (origFetch) {
    window.fetch = async (input, init) => {
      const url = typeof input === "string" ? input : input && input.url;
      const res = await origFetch(input, init);
      if (url && interesting.test(url)) {
        log("net_fetch", { url: short(url), status: res.status, type: res.headers.get("content-type") });
        if (/storage-resolve|manifest|track-playback/i.test(url)) {
          res.clone().text().then((t) => log("net_body", { url: short(url), body: t.slice(0, 600) })).catch(() => {});
        }
      }
      return res;
    };
  }
  const open = XMLHttpRequest.prototype.open;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    if (interesting.test(url)) {
      this.addEventListener("loadend", () =>
        log("net_xhr", { url: short(url), status: this.status, type: this.getResponseHeader("content-type") }));
    }
    return open.call(this, method, url, ...rest);
  };
})();
