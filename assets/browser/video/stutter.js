// stutter.js — notices when a video keeps dropping frames (docs/VIDEO.md).
//
// Every 2 s, while a video plays in a visible tab, it reads video.getVideoPlaybackQuality(). If more
// than 15 % of the frames of the last 2 s were dropped, three samples in a row (6 s), the video is
// stuttering: with "Lower quality automatically" (autoLower, off by default) the quality goes one
// step down by itself and a short notice says so; otherwise a small notice offers to do it. The
// notice can be dismissed, and then it doesn't come back in this tab. Nothing leaves the browser.
/* global lordosVideo */
(() => {
  if (window.__lordosStutter) return;
  window.__lordosStutter = true;
  const api = lordosVideo.api;
  const msg = (k, subs) => api.i18n.getMessage(k, subs) || k;

  const SAMPLE_MS = 2000;
  const DROP_RATIO = 0.15;
  const SAMPLES = 3;
  const MIN_FRAMES = 20;          // per sample: a paused or tiny video doesn't count
  const SETTLE_MS = 6000;         // after play, seek or a quality change: the player is rebuffering
  const AUTO_COOLDOWN_MS = 20000;

  // The policy, also for page.js: which codecs to hold back (it runs in the page's world, which can't
  // read the extension's storage). A change made while the page is open counts from the next video.
  let policy = lordosVideo.normalize(null);
  const load = () => lordosVideo.read().then((p) => {
    policy = p;
    document.dispatchEvent(new CustomEvent('lordos-video-codecs', { detail: lordosVideo.blocked(p).join(',') }));
  });
  load();
  api.storage.onChanged.addListener((_c, area) => { if (area === 'managed') load(); });

  let dismissed = false;
  let last = null;                // { video, total, dropped }
  let bad = 0;
  let quietUntil = Date.now() + SETTLE_MS;
  let lastAuto = 0;
  let host = null;

  const settle = () => { quietUntil = Date.now() + SETTLE_MS; bad = 0; last = null; };
  for (const ev of ['play', 'seeking', 'ratechange', 'loadstart', 'resize']) document.addEventListener(ev, settle, true);

  function playing() {
    let best = null;
    for (const v of document.querySelectorAll('video')) {
      if (v.paused || v.ended || v.readyState < 3 || typeof v.getVideoPlaybackQuality !== 'function') continue;
      if (!best || v.videoWidth * v.videoHeight > best.videoWidth * best.videoHeight) best = v;
    }
    return best;
  }

  function lowerOnce() {
    return new Promise((resolve) => {
      const done = (e) => { document.removeEventListener('lordos-video-lowered', done); clearTimeout(t); resolve(typeof e?.detail === 'string' ? e.detail : ''); };
      const t = setTimeout(done, 1500);
      document.addEventListener('lordos-video-lowered', done);
      document.dispatchEvent(new CustomEvent('lordos-video-lower'));
    });
  }

  function hide() { if (host) { host.remove(); host = null; } }

  // A small card at the bottom of the page (or of the player in fullscreen), in a closed shadow
  // root so the site's styles don't reach it.
  function show(text, actions) {
    hide();
    host = document.createElement('div');
    const root = host.attachShadow({ mode: 'closed' });
    // built node by node: the site's Trusted Types policy refuses HTML strings
    const style = document.createElement('style');
    style.textContent = `
      .card { position: fixed; inset-inline-start: 16px; bottom: 16px; z-index: 2147483647; max-width: 360px;
        display: flex; gap: 10px; align-items: center; flex-wrap: wrap; padding: 10px 12px;
        background: rgba(28, 28, 32, .94); color: #f2f2f7; border-radius: 12px;
        font: 13px/1.35 system-ui, sans-serif; box-shadow: 0 6px 24px rgba(0, 0, 0, .35); }
      .text { flex: 1 1 200px; }
      button { font: inherit; border: 0; border-radius: 8px; padding: 5px 10px; cursor: pointer;
        background: rgba(255, 255, 255, .14); color: inherit; }
      button.main { background: #0a84ff; color: #fff; }
      button:focus-visible { outline: 2px solid #64d2ff; outline-offset: 1px; }`;
    const card = document.createElement('div');
    card.className = 'card';
    card.setAttribute('role', 'status');
    card.setAttribute('aria-live', 'polite');
    const span = document.createElement('span');
    span.className = 'text';
    span.textContent = text;
    card.appendChild(span);
    root.append(style, card);
    for (const a of actions) {
      const b = document.createElement('button');
      b.textContent = a.label;
      if (a.main) b.className = 'main';
      b.addEventListener('click', a.run);
      card.appendChild(b);
    }
    (document.fullscreenElement || document.documentElement).appendChild(host);
  }

  const dismiss = { label: msg('dismiss'), run: () => { dismissed = true; hide(); } };

  async function stuttering() {
    if (policy.autoLower) {
      if (Date.now() - lastAuto < AUTO_COOLDOWN_MS) return;
      lastAuto = Date.now();
      const q = await lowerOnce();
      settle();
      if (q && !dismissed) {
        show(msg('lowered', [q]), [dismiss]);
        setTimeout(hide, 8000);
      }
      return;
    }
    if (dismissed || host) return;
    show(msg('stutter'), [
      { label: msg('lower'), main: true, run: async () => {
        const q = await lowerOnce();
        settle();
        if (q) { show(msg('lowered', [q]), [dismiss]); setTimeout(hide, 6000); } else show(msg('lowerManual'), [dismiss]);
      } },
      dismiss,
    ]);
  }

  setInterval(() => {
    if (document.visibilityState !== 'visible') { last = null; return; }
    const v = playing();
    if (!v) { last = null; bad = 0; return; }
    const q = v.getVideoPlaybackQuality();
    const now = { video: v, total: q.totalVideoFrames, dropped: q.droppedVideoFrames };
    const prev = last;
    last = now;
    if (!prev || prev.video !== v || Date.now() < quietUntil) return;
    const total = now.total - prev.total, dropped = now.dropped - prev.dropped;
    if (total < MIN_FRAMES || dropped < 0) return;
    bad = dropped / total > DROP_RATIO ? bad + 1 : 0;
    if (bad >= SAMPLES) { bad = 0; stuttering(); }
  }, SAMPLE_MS);
})();
