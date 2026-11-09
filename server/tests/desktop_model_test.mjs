// desktop_model_test.mjs — what of Desktops stays in JS (sdk/desktopmodel.js's constants and hit test,
// session restore's desktop field) and a scan of the sources. What shows, what is in front and who has
// the keyboard (the regression: focus never lands on a hidden desktop) are tested in Rust
// (rust/axia-shell/src/winmodel/tests.rs).
//
//   node tests/desktop_model_test.mjs        (from server/; run_suites.sh runs it when node exists)
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const M = await import(path.join(here, '../../sdk/desktopmodel.js'));
const S = await import(path.join(here, '../../sdk/sessionmodel.js'));
const { hitRect, MAX_DESKTOPS, FIRST_DESKTOP } = M;

let n = 0, bad = 0;
const eq = (name, got, want) => {
  n++;
  const a = JSON.stringify(got), b = JSON.stringify(want);
  if (a !== b) { bad++; console.error(`FAIL ${name}: ${a} != ${b}`); }
};
eq('constants', [MAX_DESKTOPS, FIRST_DESKTOP], [9, 1]);

// session restore keeps each window's desktop, and only a sane number
{
  const F = { x: 10, y: 20, w: 400, h: 300 };
  eq('desk kept', S.cleanEntry({ id: 'app:files', f: F, desk: 2 }).desk, 2);
  eq('first desktop is left out', 'desk' in S.cleanEntry({ id: 'app:files', f: F, desk: 0 }), false);
  eq('too large', 'desk' in S.cleanEntry({ id: 'app:files', f: F, desk: 500 }), false);
  eq('not a number', 'desk' in S.cleanEntry({ id: 'app:files', f: F, desk: '2' }), false);
  eq('negative', 'desk' in S.cleanEntry({ id: 'app:files', f: F, desk: -1 }), false);
}

// the overview's hit test
eq('hit', [hitRect({ x: 1, y: 1, w: 2, h: 2 }, 2, 2), hitRect({ x: 1, y: 1, w: 2, h: 2 }, 3, 2), hitRect(null, 0, 0)], [true, false, false]);

// lord.setStyle takes native properties only: a CSS transform there is `js: unsupported style 'transform'`
// and does nothing (the Desktops animation's slide)
{
  const fs = await import('node:fs');
  const sent = (dir, out = []) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) { if (e.name !== 'node_modules') sent(p, out); } else if (/\.jsx?$/.test(e.name) && /\b(setStyle|style|set)\([^,()]+(\.id)?,\s*'transform'/.test(fs.readFileSync(p, 'utf8'))) out.push(p);
    }
    return out;
  };
  const root = path.join(here, '../..');
  eq('nothing sends transform to setStyle', ['sdk', 'themes', 'apps', 'runtime'].flatMap((d) => sent(path.join(root, d))).map((p) => path.relative(root, p)), []);
}

if (bad) { console.error(`desktop-model-test: ${bad} of ${n} failed`); process.exit(1); }
console.log(`desktop-model-test: ${n} checks passed`);
