// desktopmodel.js — which window shows on which desktop, with no runtime or React (so it can be unit
// tested with node: server/tests/desktop_model_test.mjs). These rules run over the window list on every
// render, so they stay next to it in JS; the list of desktops, the saved values and the Overview's
// layouts are Rust's (sdk/winmodel.js). The runtime parts are sdk/windows.js (the stores and wm.*),
// sdk/desktops.js (switching, persistence, the animation) and sdk/overview.jsx.
//
// A desktop is a stable number; `list` is their order on screen and `active` the one showing. A
// window carries w.desktop. Everything that decides what is shown, what is in front and who has the
// keyboard goes through the functions here, so a window on a hidden desktop has one definition.
//
// Picture-in-picture is sticky: it shows on every desktop and never counts as the front window.

export const MAX_DESKTOPS = 9;               // rust winmodel/desktops.rs MAX_DESKTOPS says the same
export const FIRST_DESKTOP = 1;

export const onDesktop = (w, active) => !!w.pip || (w.desktop ?? FIRST_DESKTOP) === active;
// the window is out of sight: minimized, or on a desktop that isn't showing
export const windowHidden = (w, active) => !w || !!w.minimized || !onDesktop(w, active);

// The windows the theme frames on this desktop (fullscreen and picture-in-picture are the SDK's).
export const visibleOn = (list, active) => list.filter((w) => !w.minimized && !w.fullscreen && !w.pip && onDesktop(w, active));

// What the theme keeps in its tree, in stacking order. A LordOS app minimized or on a hidden desktop
// stays mounted (and display: none, WindowShell), so it keeps its state, unsaved text included. A Linux
// window has its state in its own process, and with no node it can't be reached or painted. In the
// overview, the minimized windows of this desktop are shown too.
export function mountedOn(list, active, overview = false) {
  return list.filter((w) => {
    if (w.fullscreen || w.pip) return false;
    if (!w.args?.surface) return true;
    return onDesktop(w, active) && (overview || !w.minimized);
  });
}

// In the overview a minimized surface window may have nothing to draw: an X11 window is iconic, and only
// a Wayland app keeps its last buffer. So a placeholder goes behind the body: what the window does have
// covers it, and otherwise it is seen. A LordOS app in the session has its body, and the clear parts of
// that body would show the placeholder through it.
export const overviewPlaceholder = (w, inOverview) => !!inOverview && !!w.minimized && !!w.args?.surface;

// The front window of a desktop (windows are ordered bottom to top). Never a window of another
// desktop, and never picture-in-picture, which floats over all of them with no keyboard.
export function focusedOn(list, active) {
  for (let i = list.length - 1; i >= 0; i--) {
    const w = list[i];
    if (!w.minimized && !w.pip && onDesktop(w, active)) return w;
  }
  return null;
}

// Which Linux surface gets the Wayland keyboard (sdk/wayland.js). 0 with a shell surface open, or
// when the front window of the showing desktop isn't a Linux one: never another desktop's.
export function keyboardSurface(list, active, shellBusy, surfaceOf) {
  const top = shellBusy ? null : focusedOn(list, active);
  return top ? surfaceOf(top.id) : 0;
}

// A request from a Linux app that would bring its window forward (moving, resizing, the window menu).
// These come from the app, so a window on a hidden desktop doesn't get to change the desktop.
export const acceptsAppRequest = (w, active) => !!w && onDesktop(w, active);

// The compositor's view of a Linux window: parked = on a hidden desktop. It loses the keyboard and any
// fullscreen until it comes back. X11 windows go iconic for both, so X's stacking can't catch the pointer.
export function surfaceState(w, active) {
  const parked = !onDesktop(w, active);
  return { maximized: !!w.maximized, parked, iconic: !!w.minimized || parked };
}

// The window an app comes back with: on this desktop if it has one, a visible one before a minimized
// one, else the top one of any desktop (focusing that one switches desktop).
export function pickWindow(wins, active) {
  const here = wins.filter((w) => onDesktop(w, active));
  const from = here.length ? here : wins;
  return [...from].reverse().find((w) => !w.minimized) || from[from.length - 1] || null;
}

// ---- the Overview ----

export const hitRect = (r, x, y) => !!r && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
