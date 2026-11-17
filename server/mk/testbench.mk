# axia-testbench (src/testbench, docs/PAPER-VALIDATION.md): the visual test bench, a plain Wayland
# client (test card, frame and presentation timing, every key, the pointer, the compositor's globals).
# libwayland-client and libxkbcommon only; it reads no files. The measurement pieces (src/bench: frame
# times, presentation, latency, the GL renderer's kind, the 3D scene, the report file) are shared with
# LordBench (0.8.6, docs/BENCHMARK.md) and tested on their own (make bench-test).
TESTBENCH_SRC := src/testbench/testbench.c src/testbench/checks.c src/testbench/gl3d.c src/testbench/card.c src/testbench/draw.c
all: build/axia-testbench
# the 3D view needs EGL, GLESv2 and wayland-egl (Mesa); without them the bench builds and says so
TB_GL := $(shell pkg-config --exists egl glesv2 wayland-egl 2>/dev/null && echo 1)
TB_GL_FLAGS := $(if $(TB_GL),-DTB_GL $(shell pkg-config --cflags egl glesv2 wayland-egl))
TESTBENCH_SRC += $(if $(TB_GL),src/bench/glscene.c)
TB_GL_LIBS := $(if $(TB_GL),$(shell pkg-config --libs egl glesv2 wayland-egl))
build/axia-testbench: $(TESTBENCH_SRC) $(wildcard src/testbench/*.h src/bench/*.h) $(CLIENT_GEN) $(BENCH_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(TB_GL_FLAGS) -Ibuild/gen $(WARN) -std=c11 -o $@ $(TESTBENCH_SRC) $(filter %.c,$(CLIENT_GEN)) $(BENCH_LIB) -Wl,--gc-sections -lwayland-client -lxkbcommon -ldl -lpthread -lm $(TB_GL_LIBS)
.PHONY: bench-test
# Rust owns measurement, protocol and boot-state tests, including persistence failures.
test: bench-test
bench-test: $(BENCH_INPUTS)
	$(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-bench --target-dir $(abspath build/bench-rust) --locked --offline
