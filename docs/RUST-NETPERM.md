# Rust network permission service

`server/src/netperm/` implements `axia-netperm` entirely in Rust. The legacy C service,
rule compiler, DNS monitor, sock_diag client, BPF syscall wrapper, kernel programs and
ELF-to-C generator have been removed. The four remaining headers are ABI declarations
for the compositor and the C regression fixture; no inline C implementation remains.

The `axia-netperm` Cargo package points to this directory. It compiles the existing
Rust `fwrules.rs`, `dnsmap.rs` and `fwlists.rs` sources from axia-codecs directly, so the
service uses the same rule engine and parsers without linking the compositor or the
image/font codecs. Policy JSON is parsed strictly in Rust; malformed files leave the
previous policy active. Fixed-path reads check ownership, reject links and nonregular
files, and enforce the existing 4 MiB / 32 MiB limits.

The service preserves the existing Unix framing and SO_PEERCRED user isolation,
64 clients / four per UID, cgroup lifecycle watches, user@ restriction union, ALSA
classification, blocklists, DNS TTL expiry, traffic accounting, per-address and port
statistics, and socket destruction using tuples and cookies. Resolver notifications
compile only the affected address nodes. Session scans run every five seconds and
DNS/list expiration every thirty seconds. File descriptors, BPF links and mappings
are owned Rust values, so shutdown releases attachments.

## Build

From the repository root:

```sh
rustup component add rust-src
make -C server BPF=1 build/axia-netperm
```

This builds only the network service. It does not rebuild or start the desktop and
does not attach BPF programs. `BPF=0` builds a service reporting `fw.bpfNotBuilt`.
The two variants have separate Cargo output directories to avoid stale binaries.

The BPF feature invokes the matching rustc and rust-src, with `RUSTC_BOOTSTRAP=1`
scoped to the compiler subprocesses. Tier-3 BPF support, core compilation and atomic
intrinsics currently require unstable compiler interfaces. The build derives the
BPF target from the installed compiler, changing only its bitcode-object setting to
emit one machine-code ELF object. It builds core as bitcode and supplies an empty
compiler-builtins marker: no runtime intrinsic is permitted in the final object.
The ELF validator rejects global data, non-inlined calls and non-map relocations.
No Clang, external BPF linker or new downloaded Cargo dependency is required.

## Validation

```sh
cargo test --manifest-path rust/Cargo.toml -p axia-netperm --locked --offline
cargo test --manifest-path rust/Cargo.toml -p axia-netperm --features bpf --locked --offline
make -C server BPF=1 build/npcompile-test
make -C server BPF=1 build/npverify-test
```

The C fixture compares 480,000 randomized connections against `fw_decide`, exercises
the DNS sources, precedence, trie keys, caps, devices and restrictions, and checks
Rust's kernel context offsets against Linux UAPI headers. The Rust tests exercise
strict JSON, no-link file reads, protocol fragmentation, DNS/cache behavior,
netlink/BTF/ELF bounds and the desktop's nested counter messages.

With the BPF feature, a test interpreter executes the actual embedded instructions
and mocks kernel helpers and maps, including LPM lookups, counters, events and packet
loads. It rejects out-of-range and uninitialized stack reads. It covers all eleven
hooks, randomized connection decisions, IPv4-mapped IPv6, explicit rules versus
blocklists, the user@ union, devices, Bluetooth, incoming ports, packet drops,
reset bypass and system-service accounting. These tests never alter host policy.

`npverify-test` skips the kernel verifier without root. As root it creates temporary
maps, loads all programs with their actual hook types and licenses, then closes them;
it never attaches a hook. Kernel verifier acceptance and the live integration scripts
still need a root-enabled test VM. The historical VM results in FIREWALL.md describe
the earlier implementation, not verification of these newly generated Rust programs.
