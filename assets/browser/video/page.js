// page.js — runs in the page itself (the page's world), before the site's own scripts (docs/VIDEO.md).
//
// 1. Holds back VP9 and/or AV1: the media element, MediaSource and the Media Capabilities API answer
//    "not supported" for them, so the site's player picks H.264, which costs far less to decode in
//    software. Which ones arrives from stutter.js (which can read the policy; this world can't) as a
//    'lordos-video-codecs' event on the document, detail "vp9,av1", "vp9", "av1" or "", a few
//    milliseconds after the page starts: long before the site's player loads and asks.
// 2. Lowers the player's quality one step when stutter.js asks for it (a 'lordos-video-lower' event)
//    and answers with 'lordos-video-lowered' (detail: the new quality, or '').
// The page itself could send those events too; it would only change what it is told about itself.
(() => {
  const patterns = [];
  document.addEventListener('lordos-video-codecs', (e) => {
    const list = typeof e.detail === 'string' ? e.detail.split(',') : [];
    patterns.length = 0;
    if (list.includes('vp9')) patterns.push(/vp0?9/i);
    if (list.includes('av1')) patterns.push(/av0?1/i);
  });
  const held = (type) => patterns.length > 0 && typeof type === 'string' && patterns.some((re) => re.test(type));

  const wrapStatic = (cls) => {
    if (!cls || typeof cls.isTypeSupported !== 'function') return;
    const orig = cls.isTypeSupported.bind(cls);
    Object.defineProperty(cls, 'isTypeSupported', {
      configurable: true, writable: true,
      value: function isTypeSupported(type) { return held(type) ? false : orig(type); },
    });
  };
  wrapStatic(globalThis.MediaSource);
  wrapStatic(globalThis.ManagedMediaSource);
  wrapStatic(globalThis.WebKitMediaSource);

  const proto = globalThis.HTMLMediaElement && HTMLMediaElement.prototype;
  if (proto && typeof proto.canPlayType === 'function') {
    const orig = proto.canPlayType;
    Object.defineProperty(proto, 'canPlayType', {
      configurable: true, writable: true,
      value: function canPlayType(type) { return held(type) ? '' : orig.call(this, type); },
    });
  }

  const mc = globalThis.navigator && navigator.mediaCapabilities;
  if (mc && typeof mc.decodingInfo === 'function') {
    const orig = mc.decodingInfo.bind(mc);
    Object.defineProperty(mc, 'decodingInfo', {
      configurable: true, writable: true,
      value: function decodingInfo(cfg) {
        const type = cfg && cfg.video && cfg.video.contentType;
        if (held(type)) return Promise.resolve({ supported: false, smooth: false, powerEfficient: false, configuration: cfg });
        return orig(cfg);
      },
    });
  }

  // The player's quality steps, highest first (the names its own API uses).
  const STEPS = ['highres', 'hd2880', 'hd2160', 'hd1440', 'hd1080', 'hd720', 'large', 'medium', 'small', 'tiny'];
  const LABEL = { highres: '4320p', hd2880: '2880p', hd2160: '2160p', hd1440: '1440p', hd1080: '1080p', hd720: '720p', large: '480p', medium: '360p', small: '240p', tiny: '144p' };
  const FLOOR = STEPS.indexOf('medium');          // never below 360p by itself

  function lower() {
    const p = document.getElementById('movie_player') || document.querySelector('.html5-video-player');
    if (!p || typeof p.getAvailableQualityLevels !== 'function') return '';
    const avail = p.getAvailableQualityLevels().filter((q) => STEPS.includes(q));
    if (!avail.length) return '';
    let cur = typeof p.getPlaybackQuality === 'function' ? p.getPlaybackQuality() : '';
    if (!STEPS.includes(cur)) cur = avail[0];
    const next = avail.find((q) => STEPS.indexOf(q) > STEPS.indexOf(cur) && STEPS.indexOf(q) <= FLOOR);
    if (!next) return '';
    if (typeof p.setPlaybackQualityRange === 'function') p.setPlaybackQualityRange(next, next);
    if (typeof p.setPlaybackQuality === 'function') p.setPlaybackQuality(next);
    return LABEL[next] || next;
  }

  document.addEventListener('lordos-video-lower', () => {
    let q = '';
    try { q = lower(); } catch (e) { q = ''; }
    document.dispatchEvent(new CustomEvent('lordos-video-lowered', { detail: q }));
  });
})();
