// Shared Dock material: overlays use the same tint, blur and outline.
export function dockGlass(t) {
  const dark = t.mode === 'dark';
  const effects = t.effects !== false;
  const material = lord.settings.glassMaterial(dark, effects);
  return {
    backgroundColor: effects ? (t.dockBg || material.backgroundColor)
      : material.backgroundColor,
    backdropFilter: 'blur(' + (effects ? (t.blur ?? 24) : 0) + 'px)',
    border: '1px solid ' + (t.dockBorder || material.borderColor),
  };
}
