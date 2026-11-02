// policy.js — reads the settings LordOS writes for this extension (docs/VIDEO.md).
//
// LordOS writes them as the browser's managed policy for this extension (Chromium:
// /etc/chromium/policies/managed/lordos-video.json, "3rdparty" → "extensions" → this id), so the
// extension never touches the network or the user's files and nothing is reported anywhere.
//
//   mode       "auto" (default): hold back only the codecs this computer can't decode in hardware
//              "always": hold back VP9 and AV1 everywhere the extension runs
//              "never": do nothing (the stutter notice stays)
//   autoLower  lower the quality one step by itself when the video keeps dropping frames
//   hwVp9, hwAv1  what LordOS found at boot (VA-API profiles, V4L2 stateless decoders, virtual GPU)
//
// Without a policy (another distribution, or LordOS hasn't written it yet) nothing is held back.
/* global chrome, browser */
var lordosVideo = (() => {
  const api = typeof browser !== 'undefined' ? browser : chrome;
  const DEFAULTS = { mode: 'auto', autoLower: false, hwVp9: true, hwAv1: true, managed: false };

  function normalize(p) {
    const o = { ...DEFAULTS };
    if (!p || typeof p !== 'object') return o;
    if (['auto', 'always', 'never'].includes(p.mode)) { o.mode = p.mode; o.managed = true; }
    if (typeof p.autoLower === 'boolean') o.autoLower = p.autoLower;
    if (typeof p.hwVp9 === 'boolean') o.hwVp9 = p.hwVp9;
    if (typeof p.hwAv1 === 'boolean') o.hwAv1 = p.hwAv1;
    return o;
  }

  // The codecs to hold back: ['vp9', 'av1'], one of them or none.
  function blocked(o) {
    if (o.mode === 'never') return [];
    if (o.mode === 'always') return ['vp9', 'av1'];
    return [!o.hwVp9 && 'vp9', !o.hwAv1 && 'av1'].filter(Boolean);
  }

  async function read() {
    try {
      const p = await api.storage.managed.get(['mode', 'autoLower', 'hwVp9', 'hwAv1']);
      return normalize(p);
    } catch (e) {
      return normalize(null);          // no managed storage (no policy file): defaults
    }
  }

  return { api, read, blocked, normalize };
})();
