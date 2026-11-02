// Applies the visitor's saved theme before the first paint (loaded synchronously in <head>; the
// CSP forbids inline scripts). "auto" (no data-theme) follows prefers-color-scheme.
(function () {
  var r = document.documentElement, t = null;
  try { t = localStorage.getItem('lordos-theme'); } catch (e) { /* storage blocked */ }
  if (t === 'light' || t === 'dark') r.setAttribute('data-theme', t);
  r.className += ' jt';
})();
