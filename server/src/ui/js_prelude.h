/* js_prelude.h — minimal JS bindings for callbacks and language coercion.
 * Rust owns the scheduling state (rust/axia-shell/src/scheduler.rs); callbacks and their
 * arguments stay visible to QuickJS GC in these Maps. Deadline ordering, repeats, cancellation
 * and frame snapshots cross the thin js_timers.c FFI bridge. timer_order_test.c compares
 * randomized traces, including the number of clock reads, against the previous JS implementation. */
#ifndef LORD_JS_PRELUDE_H
#define LORD_JS_PRELUDE_H

static const char prelude[] =
    "(function(){\n"
    "const g = globalThis;\n"
    "const fmt = v => typeof v === 'string' ? v : v instanceof Error ? (v.message + '\\n' + (v.stack || '')) :\n"
    "  (() => { try { return JSON.stringify(v); } catch (e) { return String(v); } })();\n"
    "const out = p => (...a) => lord.log(p + a.map(fmt).join(' '));\n"
    "g.console = { log: out(''), info: out(''), debug: out(''), warn: out('[warn] '), error: out('[error] ') };\n"
    "g.performance = { now: () => lord.now() };\n"
    "const native = lord.timers, timers = new Map();\n"
    "const add = (fn, ms, args, repeat) => { const delay = +ms;\n"
    "  const id = native.add(lord.now(), delay, repeat); timers.set(id, { fn, args }); return id; };\n"
    "g.setTimeout = (fn, ms, ...a) => add(fn, ms, a, false);\n"
    "g.setInterval = (fn, ms, ...a) => add(fn, ms, a, true);\n"
    "g.setImmediate = (fn, ...a) => add(fn, 0, a, false);\n"
    "g.clearTimeout = g.clearInterval = g.clearImmediate = id => { timers.delete(id); native.cancel(id); };\n"
    "g.queueMicrotask = fn => { Promise.resolve().then(fn); };\n"
    "let frames = new Map();\n"
    "g.requestAnimationFrame = fn => { const id = native.requestFrame(); frames.set(id, fn); return id; };\n"
    "g.cancelAnimationFrame = id => { frames.delete(id); native.cancelFrame(id); };\n"
    "g.__lordTick = now => {\n"
    "  const due = native.due(now); if (due) for (const id of due) {\n"
    "    const t = timers.get(id); if (!t) continue;\n"
    "    const kind = native.fire(id, now); if (kind < 0) continue; if (!kind) timers.delete(id);\n"
    "    try { t.fn(...t.args); } catch (e) { console.error('timer:', e); }\n"
    "  }\n"
    "  return native.next(lord.now());\n"
    "};\n"
    "g.__lordWantsFrame = () => native.hasFrames();\n"
    "g.__lordFrame = now => { const f = frames; frames = new Map();\n"
    "  for (const id of native.frames()) { const fn = f.get(id); try { fn(now); } catch (e) { console.error('raf:', e); } } };\n"
    "g.__lordFrameReady = (now, waiting, epoch) => native.frameReady(now, waiting, epoch);\n"
    "g.__lordFrameWait = (now, waiting, epoch) => native.frameWait(now, waiting, epoch);\n"
    "g.__lordTimerChanged = () => native.changed();\n"
    "g.__lordClearTimers = () => { timers.clear(); frames.clear(); native.reset(); };\n"
    "})();\n";

#endif
