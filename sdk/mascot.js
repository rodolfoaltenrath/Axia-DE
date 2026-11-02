// mascot.js — Lumo, the LordOS guardian (docs/brand/README.md, "Lumo").
//
//   <Lumo pose="confused" size={96} />   a pose: 'wave' (welcome), 'shield' (security), 'run' (busy), 'celebrate'
//                                        (done), 'confused' (error, unavailable), 'sleep' (idle)
//   <Lumo pose="avatar" size={24} />     the head in a violet circle (the simplified one up to 40 px)
//
// The images are PNG renders of assets/brand/mascot/*.svg in assets/mascot/ (tools/gen-mascot.py),
// twice as big as the largest size the UI draws. Lumo has its own dark-indigo outline and white
// and violet fills, so it reads on light and dark alike; never tint or recolor it. Keep it modest:
// one Lumo per view, in an empty, error, welcome or "done" state (docs/DESIGN.md, "Empty state").
// `alt` is the translated description (mascot.<pose>); a caller may pass its own.
import { createElement as h } from 'react';
import { Image } from '@lordos/runtime';
import { t } from './i18n.js';

const POSES = new Set(['wave', 'shield', 'celebrate', 'confused', 'sleep', 'run']);

export function lumoSource(pose = 'confused', size = 96) {
  if (pose === 'avatar') return size <= 40 ? 'asset:mascot/lumo-avatar-small-64.png' : 'asset:mascot/lumo-avatar-128.png';
  return `asset:mascot/lumo-${POSES.has(pose) ? pose : 'confused'}-256.png`;
}

export function Lumo({ pose = 'confused', size = 96, alt, style }) {
  return h(Image, {
    src: lumoSource(pose, size),
    alt: alt || t(`mascot.${POSES.has(pose) || pose === 'avatar' ? pose : 'confused'}`),
    style: [{ width: size, height: size }, style],
  });
}
