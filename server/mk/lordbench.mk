# LordBench implementations, entry points and tests are Rust. The archive keeps
# the ABI used by the compositor and sysreport during their migration.
build/obj/main.o: src/bench/lbframes.h src/bench/lbinput.h
build/obj/ui/grab.o: src/bench/lbinput.h
build/obj/ui/js_hw.o: src/bench/lbrun.h

# Shared engine and sandbox subsystems still undergoing their own migration.
# No benchmark C implementation or QuickJS is linked into the helper.
LB_HOST_SRC := src/sec/sandbox.c src/sec/cgroup.c $(ENGINE_SRC)
LB_HOST_OBJ := $(LB_HOST_SRC:src/%.c=build/bench-host/%.o)
build/bench-host/%.o: src/%.c $(HDR) mk/lordbench.mk build/bench-features
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -fPIC -ffunction-sections -fdata-sections -c $< -o $@
build/bench-host/libbench_host.a: $(LB_HOST_OBJ)
	$(AR) rcs $@ $^
comma := ,
LB_FEATURES := native-host$(if $(filter 1,$(GPU)),$(comma)gpu)
.PHONY: bench-features-force
build/bench-features: bench-features-force
	@mkdir -p build
	@printf '%s\n' '$(LB_FEATURES)' > $@.new
	@cmp -s $@ $@.new && rm $@.new || mv $@.new $@
LB_NATIVE_ENV := AXIA_BENCH_HOST_DIR=$(abspath build/bench-host)
# Keep native-host's archive (which bundles the shared engine) separate from
# BENCH_LIB, used by the existing C compositor and testbench.
LB_TARGET_DIR := $(abspath build/bench-native)
all: build/axia-bench
# Native-host has its own target dir and compiles the shell again. Keep it after
# the main archive to avoid overlapping large Rust builds on package builders.
# The order-only prerequisite never forces the main archive to rebuild.
build/axia-bench: $(BENCH_INPUTS) $(RUST_INPUTS) build/bench-host/libbench_host.a build/bench-features | $(RUST_LIB)
	$(LB_NATIVE_ENV) $(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-bench --bin axia-bench --features $(LB_FEATURES) --target-dir $(LB_TARGET_DIR) --release --locked --offline
	cp build/bench-native/release/axia-bench $@

.PHONY: lordbench-test
test: lordbench-test
lordbench-test: build/axia-bench
	$(LB_NATIVE_ENV) $(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-bench --features $(LB_FEATURES) --test lordbench-native --target-dir $(LB_TARGET_DIR) --release --locked --offline -- --test-threads=1
	find build/bench-native/release/deps -maxdepth 1 -type f -executable -name 'lordbench_native-*' -printf '%T@ %p\n' | sort -nr | head -1 | cut -d' ' -f2- | xargs -r -d '\n' -I{} cp '{}' build/lordbench-test
# Existing build callers keep an alias to the Rust harness.
build/lordbench-test: lordbench-test

build/lordbench-cli: $(BENCH_INPUTS) build/axia-bench
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-bench --bin lordbench-cli --target-dir $(abspath build/bench-rust) --release --locked --offline
	cp build/bench-rust/release/lordbench-cli $@

# Optional reproducibility tool. The committed corpus is embedded by Rust.
build/lbcorpus: $(BENCH_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-bench --bin lbcorpus --target-dir $(abspath build/bench-rust) --release --locked --offline
	cp build/bench-rust/release/lbcorpus $@
