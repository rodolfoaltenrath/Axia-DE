//! System Information's collector (docs/BENCHMARK.md): what hardware this machine has and what the
//! kernel made of it, read without root from world-readable files (sysfs, /proc, the device tree,
//! os-release, the kernel config). Used by `axia-sysreport` and `axia-fetch` and by `lord.sysinfo`;
//! nothing is written, executed (but `rpm -qa` for the package count of `axia-fetch`) or sent anywhere.
//!
//! Privacy by construction: the report is built from a fixed list of fields (`collect`), and the
//! structures have no place for serial numbers, UUIDs, MAC or IP addresses, host or user names, Wi-Fi
//! network names or home-folder paths. The tests fill fake systems with such values and check that none
//! reaches the JSON, the Markdown or the summary.
//!
//! The C server and the commands reach it through the C ABI of `server/src/sysreport/sysreport.h`
//! (`ffi`).
pub mod collect;
pub mod facts;
pub mod fetch;
mod ffi;
pub mod fsutil;
pub mod out;
pub mod periph;
pub mod rpifw;

pub use collect::{collect, cmdline_has, Report};
