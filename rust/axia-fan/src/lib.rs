//! axia-fand (docs/FANS.md): the root service that drives the fans by temperature. The pure
//! rules (profiles, curves with hysteresis, `fan.json`, the socket requests) are `curve`; the
//! sysfs side (finding the CPU temperature and the fans it may drive, writing their duty and
//! giving control back) is `hw`; the daemon itself (status, requests, the loop) is `daemon`.
//! `ffi` is the little C ABI `server/src/ui/js_fan.c` validates its arguments with.
pub mod curve;
pub mod daemon;
mod ffi;
pub mod hw;
