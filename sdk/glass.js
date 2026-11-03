// Shared Dock material: overlays use the same tint, blur and outline.
export function dockGlass(t) {
  const dark = t.mode === 'dark';
  const effects = t.effects !== false;
  return {
    backgroundColor: effects ? (t.dockBg || (dark ? 'rgba(46,46,56,0.42)' : 'rgba(255,255,255,0.38)'))
      : (dark ? '#2a2a30' : '#f2f2f5'),
    backdropFilter: 'blur(' + (effects ? (t.blur ?? 24) : 0) + 'px)',
    border: '1px solid ' + (t.dockBorder || t.windowBorder || t.separator),
  };
}
