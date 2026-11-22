# System Information (0.8.5, rust/axia-sysreport, docs/BENCHMARK.md): the collector, shared by the
# `axia-sysreport` and `axia-fetch` commands (one binary; build/axia-fetch is a link) and lord.sysinfo.
# Read-only, no root; `bench` starts the sandboxed axia-bench next to it. The collector, the report, the
# summary and the peripherals are Rust (the crate is a static library for this command; axia-server gets
# it through libaxia_shell.a); cli.c is the command's main and the benchmark's glue.
SYSREPORT_TARGET := build/sysreport-rust
SYSREPORT_LIB := $(SYSREPORT_TARGET)/release/libaxia_sysreport.a
SYSREPORT_INPUTS := $(wildcard ../rust/axia-sysreport/src/*.rs) ../rust/axia-sysreport/Cargo.toml $(CODEC_INPUTS)
$(SYSREPORT_LIB): $(SYSREPORT_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-sysreport --lib --target-dir $(abspath $(SYSREPORT_TARGET)) --release --locked --offline
all: build/axia-sysreport build/axia-fetch
build/axia-fetch: build/axia-sysreport
	ln -sf axia-sysreport $@
build/axia-sysreport: src/sysreport/cli.c src/sysreport/sysreport.h $(wildcard src/bench/*.h) build/gen/lord_version.h $(SYSREPORT_LIB) $(BENCH_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) -Ibuild/gen $(CFLAGS) $(WARN) -std=c11 -o $@ src/sysreport/cli.c src/sec/json.c $(SYSREPORT_LIB) $(BENCH_LIB) -lm -lpthread $(CODEC_NATIVE_LIBS)
# The fake machines full of serials, UUIDs, MACs and names that must not reach a report, the peripherals
# (on screen only), the session facts and the boot medium are tests of the crate (cargo test, part of
# test-rust); `cargo run -p axia-sysreport --example fixture -- <dir> amd|intel|nvidia|rpi5|vm` writes a fake
# tree for `axia-sysreport report --root`
# (0.8.7) the catalogue's signed snapshot (src/sysreport/hwsnap.c, lord.sysinfo.community()): signature first,
# structure, limits, no links; and the snapshot tools/hwreport/catalog.mjs wrote, signed with a test key
test: build/hwsnap-test
build/hwsnap-test: tests/hwsnap_test.c src/sysreport/hwsnap.c src/sec/codesig.c src/sec/json.c src/pkg/crypto.c src/sysreport/hwsnap.h src/sec/codesig.h src/sec/json.h src/pkg/crypto.h build/gen/codesig_build.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/hwsnap_test.c src/sysreport/hwsnap.c src/sec/codesig.c src/sec/json.c src/pkg/crypto.c -lpthread
	./$@ ../assets/hwreport/snapshot.json || { rm -f $@; exit 1; }
