// desktopmodel.js — the constants of Desktops and the Overview's hit test, with no runtime or React (so
// node tests can import them: server/tests/desktop_model_test.mjs, sdk/sessionmodel.js). Which window
// shows on which desktop, what is in front and who has the keyboard are Rust's
// (rust/axia-shell/src/winmodel/session.rs, through sdk/windows.js); the list of desktops, the saved
// values and the Overview's layouts too (sdk/winmodel.js).

export const MAX_DESKTOPS = 9;               // rust winmodel/desktops.rs MAX_DESKTOPS says the same
export const FIRST_DESKTOP = 1;

export const hitRect = (r, x, y) => !!r && x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
