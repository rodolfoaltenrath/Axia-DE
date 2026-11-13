# axia-server — libc + kernel headers + libxkbcommon + libwayland-server + vendored QuickJS and Rust codecs.
# No libdrm, libinput, X11 or WebKit; the Wayland compositor (src/wl) and the terminal (src/term) are our own.
CC      ?= cc
CARGO   ?= cargo
RUST_MANIFEST := ../rust/Cargo.toml
RUST_LIB := build/rust/release/libaxia_shell.a
CODEC_LIB := build/codecs/release/libaxia_codecs.a
VAULT_CODEC_LIB := build/vault-codecs/release/libaxia_codecs.a
# axia-maild's archive also carries the mail parsers (rust/axia-mail, docs/RUST-MAIL.md).
MAIL_CODEC_LIB := build/mail-codecs/release/libaxia_codecs.a
CRYPTO_INPUTS := ../rust/axia-crypto/Cargo.toml $(wildcard ../rust/axia-crypto/src/*.rs) ../rust/vendor/argon2-0.6.0/src/lib.rs
CODEC_INPUTS := $(CRYPTO_INPUTS) ../rust/axia-codecs/Cargo.toml $(wildcard ../rust/axia-codecs/src/*.rs) $(RUST_MANIFEST) ../rust/Cargo.lock
MAIL_CODEC_INPUTS := $(CODEC_INPUTS) ../rust/axia-mail/Cargo.toml $(wildcard ../rust/axia-mail/src/*.rs)
CODEC_NATIVE_LIBS := -Wl,--gc-sections -ldl -lpthread -lm
# The Rust archive also contains launcher callbacks into the host sandbox.
# Helpers using only MIME/filesystem functions must discard unreachable sections.
RUST_NATIVE_LIBS := -Wl,--gc-sections $(shell pkg-config --libs gio-2.0) -lxkbcommon
# Recurse through module directories: a native_view/ edit must rebuild the Rust
# archive even when no top-level .rs file changed.
rust_sources = $(wildcard $(1)/*.rs) $(foreach d,$(wildcard $(1)/*/),$(call rust_sources,$(patsubst %/,%,$(d))))
RUST_INPUTS := $(RUST_MANIFEST) ../rust/Cargo.lock ../rust/axia-shell/Cargo.toml $(call rust_sources,../rust/axia-shell/src) $(call rust_sources,../apps) ../rust/native_apps.rs ../.cargo/config.toml
RUST_INPUTS += $(wildcard ../rust/axia-shell/src/*.css ../assets/vscode/axia-window-controls/*)
RUST_INPUTS += $(CODEC_INPUTS)
RUST_INPUTS += ../assets/emoji.json
RUST_INPUTS += ../assets/monitor/geoip-country.mmdb ../assets/monitor/land.bin
# The desktop runs on a host distribution: devices come from libseat, and networking,
# packages and login stay with the host.
CPPFLAGS += -DLORD_LIBSEAT $(shell pkg-config --cflags libseat)
LDLIBS += $(shell pkg-config --libs libseat)
CFLAGS  ?= -O2 -g
WARN    := -Wall -Wextra
CPPFLAGS += -D_GNU_SOURCE -Isrc -Ibuild/gen -isystem ../third_party/quickjs
LDLIBS  += -lm -lpthread -lxkbcommon -lwayland-server -lwayland-client -lcrypt

# Architecture: x86_64 (PC) or aarch64 (Raspberry Pi 5/4 and other ARM64, see docs/RASPBERRY.md). Never
# -march=native: the .deb has to run on any CPU of the architecture. On aarch64 NEON is
# part of ARMv8-A (no -mfpu needed) and canvas.c's NEON loops are used (NEON=0 turns them off;
# tests/canvas_test.c compares them with the C ones).
ARCH ?= $(shell $(CC) -dumpmachine 2>/dev/null | cut -d- -f1)
ifeq ($(ARCH),aarch64)
NEON ?= 1
ifeq ($(NEON),1)
CPPFLAGS += -DLORD_NEON
endif
endif

# Wayland protocols: wayland-scanner generates the code from the wayland-protocols XML files.
WLPROTO ?= $(shell pkg-config --variable=pkgdatadir wayland-protocols)
WL_XML  := $(WLPROTO)/stable/xdg-shell/xdg-shell.xml $(WLPROTO)/unstable/xdg-decoration/xdg-decoration-unstable-v1.xml
WL_GEN  := build/gen/xdg-shell-protocol.h build/gen/xdg-decoration-unstable-v1-protocol.h
WL_OBJ  := build/gen/xdg-shell-protocol.o build/gen/xdg-decoration-unstable-v1-protocol.o
# dmabuf, presentation, viewporter and mouse for games (M7/GPU): they also work on the CPU path
WL_MORE := staging/xdg-activation/xdg-activation-v1 stable/linux-dmabuf/linux-dmabuf-v1 stable/presentation-time/presentation-time stable/viewporter/viewporter \
           unstable/relative-pointer/relative-pointer-unstable-v1 unstable/pointer-constraints/pointer-constraints-unstable-v1 \
           staging/cursor-shape/cursor-shape-v1 unstable/tablet/tablet-unstable-v2 \
           staging/fractional-scale/fractional-scale-v1 unstable/xdg-output/xdg-output-unstable-v1 \
           unstable/primary-selection/primary-selection-unstable-v1 unstable/idle-inhibit/idle-inhibit-unstable-v1
WL_GEN  += $(foreach p,$(WL_MORE),build/gen/$(notdir $(p))-protocol.h)
WL_OBJ  += $(foreach p,$(WL_MORE),build/gen/$(notdir $(p))-protocol.o)

# GPU compositing (src/gpu): Mesa (GBM + EGL + GLES2) if its -dev packages are present; otherwise (or with
# GPU=0) it's built without GPU and axia-server always composites on the CPU. See docs/GPU.md.
GPU ?= $(shell pkg-config --exists gbm egl glesv2 2>/dev/null && echo 1 || echo 0)
ifeq ($(GPU),1)
CPPFLAGS += -DLORD_GPU $(shell pkg-config --cflags gbm egl glesv2)
LDLIBS   += $(shell pkg-config --libs gbm egl glesv2)
endif

# Wayland client headers for app mode (see CLIENT_PROTO; the server defines the interfaces)
APP_CLIENT_HDR := build/gen/client/xdg-shell-client-protocol.h build/gen/client/xdg-decoration-unstable-v1-client-protocol.h \
                  build/gen/client/pointer-gestures-unstable-v1-client-protocol.h
QJS_DIR := ../third_party/quickjs
QJS_OBJ := $(patsubst %,build/qjs/%.o,quickjs libregexp libunicode cutils dtoa)

# .lpk app isolation, permissions and signing (src/sec, see docs/PRIVACY.md and docs/SIGNING.md)
# Monitor is implemented in apps/monitor/*.rs; C only binds the shell compatibility ABI and the
# native views that Rust mounts (js_nativeview.c, rust/axia-shell/src/native_view).
MON_SRC := src/ui/js_monitor.c src/ui/js_nativeview.c
# Media (src/media, docs/MEDIA.md): only the axia-server side. Only axia-media links FFmpeg.
MEDIA_SRC := src/media/media.c src/media/hwdec.c src/media/mview.c src/media/capture.c src/media/annot.c src/media/maudio.c src/media/mpulse.c src/media/proto.c src/ui/js_media.c
SEC_SRC := src/sec/json.c src/sec/sign.c src/sec/pkgstate.c src/sec/perms.c src/sec/cgroup.c src/sec/sandbox.c src/sec/brokersrv.c src/sec/brokercam.c src/sec/brokerkr.c src/sec/brokermail.c src/sec/restrict.c src/sec/appid.c src/sec/pwguard.c src/sec/codesig.c
# App mode: axia-server --app <id> runs an .lpk app as an isolated Wayland client (src/app)
APP_SRC := src/app/appclient.c src/app/appspawn.c src/app/probe.c src/ui/js_broker.c src/ui/js_brokercam.c src/ui/js_brokermail.c src/app/apphost_bc.c
SRC := src/main.c src/kms.c src/gpu/gpu.c src/canvas.c src/lpk.c src/users/admin_rescue.c src/users/homedirs.c src/wl/wl.c src/wl/xwm.c \
       src/ui/text.c \
       src/ui/paint.c src/ui/js.c src/ui/js_timers.c src/ui/js_anim.c src/ui/js_sys.c src/ui/js_term.c src/ui/js_wasm.c src/wasm/wasm.c src/wasm/wasm_run.c src/ui/js_sound.c src/ui/js_net.c src/sound/snd.c src/sound/pulse.c src/ui/otf.c src/ui/fonts.c \
       src/ui/unicode.c src/ui/bidi.c src/ui/shape.c src/ui/shape_indic.c src/ui/shape_syl.c src/ui/shape_khmer.c src/ui/shape_myanmar.c src/ui/shape_use.c src/ui/thbrk.c src/ui/textlayout.c \
       src/term/vt.c src/term/pty.c src/term/term.c src/term/term_paint.c src/term/hold.c \
       src/net/nl.c src/net/state.c \
       src/pkg/crypto.c src/pkg/xz.c src/ui/js_sec.c \
       $(SEC_SRC) $(APP_SRC) $(MON_SRC) $(MEDIA_SRC)
# Documents and printing (M9): axia-doc helper sessions and our own IPP/mDNS/eSCL client
# (src/doc, src/print; see docs/DOCUMENTS.md and docs/PRINTING.md). No Poppler or libarchive here.
DOC_SRC := src/doc/doc.c src/doc/wire.c src/ui/js_doc.c src/print/ipp.c src/print/phttp.c src/print/mdns.c src/print/cups.c \
           src/print/escl.c src/print/print.c src/ui/js_print.c
SRC += $(DOC_SRC)
# Per-app firewall (src/fw, docs/FIREWALL.md): rules, decision and names learned from DNS
FW_SRC := src/fw/rules.c src/fw/dnsmap.c src/fw/fwmap.c src/fw/fwlists.c src/fw/fwsub.c src/ui/js_fw.c src/sec/netconn.c
SRC += $(FW_SRC)
OBJ := $(SRC:src/%.c=build/obj/%.o)
HDR := $(wildcard src/*.h src/gpu/*.h src/ui/*.h src/term/*.h src/wl/*.h src/wasm/*.h src/sound/*.h src/pkg/*.h src/net/*.h src/users/*.h src/sec/*.h src/platform/*.h src/media/*.h src/app/*.h src/doc/*.h src/print/*.h) $(WL_GEN) $(APP_CLIENT_HDR)
HDR += $(wildcard src/fw/*.h)
# Touchpad and touchscreens: our own driver (src/tp, no libinput), gestures and touches for the
# Wayland apps (Rust wayland/input.rs) and lord.input (src/ui/js_input.c). See docs/TRACKPAD.md.
TP_SRC := src/ui/js_input.c
SRC += $(TP_SRC)
# Window management: Rust dragging and cursors, with the existing painter's drag layer (docs/WINDOWS.md).
# cursor-shape-v1 needs the tablet-v2 interfaces (they're only linked, not advertised).
# GTK/GNOME/Qt apps: our own org.freedesktop.portal.Settings (src/ui/portal.c) with the D-Bus client from
# Passwords (src/vault/dbus.c, docs/PASSWORDS.md).
WM_SRC := src/ui/js_wm.c src/ui/js_snap.c src/ui/js_winmodel.c src/ui/js_switcher.c src/ui/portal.c src/ui/portal_files.c src/vault/dbus.c
SRC += $(WM_SRC)
# Clock and Help (docs/SYSTEM.md): the help articles (the time zones are Rust: axia-codecs/src/tzif.rs)
CLOCK_SRC := src/ui/js_help.c
SRC += $(CLOCK_SRC)
# Logs (docs/SYSTEM.md): our own systemd journal reader, the logs-system action and lord.logs
LOGS_SRC := src/logs/journal.c src/ui/js_logs.c
SRC += $(LOGS_SRC)
HDR += $(wildcard src/logs/*.h)
# Backups (docs/SYSTEM.md): hard-linked snapshots in a low-priority thread
BACKUP_SRC := src/backup/engine.c src/backup/crypt.c src/backup/backup.c src/ui/js_backup.c
SRC += $(BACKUP_SRC)
HDR += $(wildcard src/backup/*.h)
# Backup encryption uses allocation-free Rust primitives through axia-shell.
# Disks (docs/SYSTEM.md): disks, partitions and mounts without root, our own SMART and the disk-* actions
DISK_SRC := src/disk/disk.c src/disk/smart.c src/disk/smartctl.c src/ui/js_disk.c
SRC += $(DISK_SRC)
HDR += $(wildcard src/disk/*.h)
SRC += src/platform/admin.c src/platform/net.c
SRC += src/platform/seat.c src/platform/power.c src/ui/js_files.c src/ui/js_launcher.c src/ui/js_native_apps.c
HDR += $(wildcard src/platform/*.h)
OBJ := $(SRC:src/%.c=build/obj/%.o)
HDR += $(wildcard src/tp/*.h)
build/obj/wl/wl.o build/wl-activation-test build/wl-fullscreen-test build/wl-input-region-test build/wl-preview-test: src/wl/wl_activation.c

.PHONY: all clean test test-full test-platform test-rust drag-golden
test-platform: build/apps-actions-test build/defaults-test build/settings-hw-test
test test-platform: build/wl-fullscreen-test build/wl-activation-test

build/wl-activation-test: tests/wl_activation_test.c src/wl/wl.c $(HDR) $(WL_GEN) build/gen/xdg-activation-v1-protocol.o
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< build/gen/xdg-activation-v1-protocol.o -lwayland-server -lm
	./$@ || { rm -f $@; exit 1; }

build/wl-fullscreen-test: tests/wl_fullscreen_test.c src/wl/wl.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< -lwayland-server -lm
	./$@ || { rm -f $@; exit 1; }

test-platform: build/ui-damage-test build/brframe-test build/glass-damage-test build/wl-preview-test build/media-preview-test build/portal-files-test build/fsops-test build/battery-refresh-test build/host-power-test build/glass-renderer-test build/seat-test build/platform-admin-test build/keygrab-test build/switcher-ffi-test build/snap-ffi-test build/shadow-ffi-test build/winmodel-ffi-test build/wl-input-region-test build/cpu-cursor-test build/kms-cursor-test test-rust
	./build/wl-fullscreen-test
	./build/wl-activation-test
	./build/ui-damage-test
	./build/glass-damage-test
	./build/brframe-test
	./build/wl-preview-test
	./build/fsops-test
	./build/battery-refresh-test
	./build/host-power-test
	./build/portal-files-test
	./build/media-preview-test
	./build/glass-renderer-test
	cargo test --manifest-path $(RUST_MANIFEST) --target-dir build/rust --locked --offline --test image_runtime -- --ignored --nocapture
	./build/seat-test
	./build/platform-admin-test
	./build/switcher-ffi-test
	./build/snap-ffi-test
	./build/shadow-ffi-test
	./build/winmodel-ffi-test
	./build/keygrab-test
	./build/wl-input-region-test
	./build/cpu-cursor-test
	./build/kms-cursor-test
	./build/apps-actions-test
	./build/defaults-test
	./build/settings-hw-test
build/glass-renderer-test: tests/glass_renderer_test.c tests/glass_renderer_golden.h src/canvas.c src/canvas.h src/glass_ffi.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $< src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/glass-damage-test: tests/glass_damage_test.c src/ui/paint.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< -lm
build/ui-damage-test: tests/ui_damage_test.c src/main.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/outputs.c -lm
build/battery-refresh-test: tests/battery_refresh_test.c src/hw/hw.c src/hw/power.c src/sec/json.c src/platform/power.h $(HDR) $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/hw/power.c src/sec/json.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/portal-files-test: tests/portal_files_test.c src/ui/portal_files.c src/ui/portal_files.h src/vault/dbus.c
	# Keep transport calls visible to --wrap; RPM LTO can inline the real bus functions.
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -fno-lto -Wl,--wrap=dbus_send,--wrap=dbus_reply,--wrap=dbus_error -o $@ $< src/ui/portal_files.c src/vault/dbus.c
build/media-preview-test: tests/media_preview_test.c src/media/mview.c src/media/media_int.h src/canvas.c $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/media/mview.c src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/host-power-test: tests/host_power_test.c src/platform/power.c src/platform/power.h src/vault/dbus.c $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -o $@ $< src/platform/power.c src/vault/dbus.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/cpu-cursor-test: tests/cpu_cursor_test.c src/main.c src/outputs.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/outputs.c -lm
build/kms-cursor-test: tests/kms_cursor_test.c src/kms.c src/kms.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $<
# These existing host tests supply their own scene fixtures. Keep their symbols
# separate from the production Rust runtime carried by the same static archive.
build/wl-preview-test build/gpu-capture-test: CPPFLAGS += -Dui=axia_fixture_ui -Dnode_get=axia_fixture_node_get
build/wl-preview-test: tests/wl_preview_test.c src/wl/wl.c $(HDR) $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -lwayland-server -lwayland-client -ldl -lpthread -lm
build/gpu-capture-test: tests/gpu_capture_test.c src/wl/wl.c src/gpu/gpu.c $(HDR) $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< src/gpu/gpu.c src/kms.c src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) $(LDLIBS) -ldl
# Optional surfaceless Mesa probe (mesa-dri-drivers), without opening a display.
build/gpu-round-test: tests/gpu_round_test.c src/gpu/gpu.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< $(shell pkg-config --libs gbm egl glesv2) -lm
build/wl-input-region-test: tests/wl_input_region_test.c src/wl/wl.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -ffunction-sections -fdata-sections -Wl,--gc-sections -o $@ $< -lwayland-server -lwayland-client -lm
test-rust:
	$(CARGO) test --manifest-path $(RUST_MANIFEST) --target-dir $(abspath build/rust) --locked --offline
test test-platform: build/input-ffi-test
build/input-ffi-test: tests/input_ffi_test.c src/input.h src/tp/tp.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -Wl,--wrap=ioctl,--wrap=stat -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
test test-platform: build/key-latency-ffi-test
build/key-latency-ffi-test: tests/key_latency_ffi_test.c src/keylag.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -Wl,--wrap=clock_gettime -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
test test-platform: build/touch-route-ffi-test
build/touch-route-ffi-test: tests/touch_route_ffi_test.c tests/wayland_input_fixture.c tests/wayland_input_fixture.h src/tp/tp.h src/tp/tp_route.h src/ui/ui.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -Wl,--wrap=clock_gettime -o $@ tests/touch_route_ffi_test.c tests/wayland_input_fixture.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -lwayland-server -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
test test-platform: build/window-grab-ffi-test
build/window-grab-ffi-test: tests/window_grab_ffi_test.c src/ui/grab.h src/cursor.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
test test-platform: build/text-input-ffi-test
test test-platform: build/wayland-input-ffi-test
XWL_TEST_WRAP := stat lstat open unlink chmod mkdir bind listen socket socketpair fork waitpid kill syscall getenv execve unshare usleep time fcntl dup2 wl_client_create wl_event_loop_add_fd wl_event_source_remove
test test-platform: build/xwayland-ffi-test
build/xwayland-ffi-test: tests/xwayland_ffi_test.c src/wl/xwayland.h src/sec/cgroup.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) $(foreach f,$(XWL_TEST_WRAP),-Wl,--wrap=$(f)) -o $@ tests/xwayland_ffi_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -lwayland-server -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
build/wayland-input-ffi-test: tests/wayland_input_ffi_test.c tests/wayland_input_fixture.c tests/wayland_input_fixture.h src/wl/wl_input.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -Wl,--wrap=clock_gettime -o $@ tests/wayland_input_ffi_test.c tests/wayland_input_fixture.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -lwayland-server -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
build/text-input-ffi-test: tests/text_input_ffi_test.c src/wl/wl_input.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -o $@ tests/text_input_ffi_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -lwayland-server -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
$(RUST_LIB): $(RUST_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) --target-dir $(abspath build/rust) --release --locked --offline
build/switcher-ffi-test: tests/switcher_ffi_test.c src/ui/switcher_ffi.h $(RUST_LIB)
	$(CC) $(CFLAGS) $(WARN) -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/snap-ffi-test: tests/snap_ffi_test.c src/ui/snap_ffi.h $(RUST_LIB)
	$(CC) $(CFLAGS) $(WARN) -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/winmodel-ffi-test: tests/winmodel_ffi_test.c src/ui/winmodel_ffi.h $(RUST_LIB)
	$(CC) $(CFLAGS) $(WARN) -o $@ $< $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
# sdk/windows.js over the real bridge and Rust window list, in QuickJS (tests/windows_bridge_test.mjs)
test: build/windows-bridge-host
build/windows-bridge-host: tests/windows_bridge/host.c src/ui/js_winmodel.c src/ui/js_winmodel.h src/ui/winmodel_ffi.h $(QJS_OBJ) $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -Isrc/ui -o $@ tests/windows_bridge/host.c src/ui/js_winmodel.c $(QJS_OBJ) $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
build/seat-test: tests/seat_test.c src/platform/seat.c src/platform/seat.h
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -D_GNU_SOURCE -DLORD_LIBSEAT $(shell pkg-config --cflags libseat) -Isrc -o $@ tests/seat_test.c src/platform/seat.c
build/platform-admin-test: tests/platform_admin_test.c src/platform/admin.c src/admin.h
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -D_GNU_SOURCE -Isrc -o $@ tests/platform_admin_test.c src/platform/admin.c
ALL += build/axia-server
# axia-media: the media helper (docs/MEDIA.md). It's the only one that links FFmpeg (and libva);
# axia-server never does. If FFmpeg's -dev packages are missing, everything else is built without it.
MEDIA ?= $(shell pkg-config --exists libavcodec libavformat libavutil libswscale libswresample 2>/dev/null && echo 1 || echo 0)
ifeq ($(MEDIA),1)
ALL += build/axia-media
endif
# axia-doc: the document helper (docs/DOCUMENTS.md). It's the only one that links Poppler and
# libarchive (and cairo, FreeType and fontconfig, which come with Poppler); axia-server never does.
DOCHELPER ?= $(shell pkg-config --exists poppler-glib libarchive cairo-pdf cairo-ft freetype2 fontconfig 2>/dev/null && echo 1 || echo 0)
ifeq ($(DOCHELPER),1)
ALL += build/axia-doc
endif
# axia-netperm (src/netperm, docs/FIREWALL.md step 2): the root service that applies per-app network
# permissions with cgroup BPF programs. The programs are compiled with clang -target bpf (BPF=1,
# if clang is available) and go inside the binary (build/bpf2c → build/gen/np_bpf.h). Without clang it still
# builds and the service warns that it has no BPF. No libbpf or bcc. Not part of `all`: no unit
# installs it on a host distribution yet.
.DEFAULT_GOAL := all
BPF ?= 0
NP_SRC := src/netperm/netperm.c src/netperm/bpfsys.c src/netperm/npcompile.c src/netperm/npdns.c src/netperm/npdiag.c \
          src/fw/rules.c src/fw/dnsmap.c src/fw/fwmap.c src/fw/fwlists.c src/sec/json.c
ifeq ($(BPF),1)
NP_GEN := build/gen/np_bpf.h
NP_CPP := -DNP_HAVE_BPF
endif
build/bpf2c: src/netperm/bpf2c.c
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -std=c11 -o $@ $<
build/np.bpf.o: src/netperm/np.bpf.c src/netperm/np_shared.h
	@mkdir -p build
	clang -target bpf -O2 -Wall -Wextra -Isrc -I/usr/include/$(shell $(CC) -dumpmachine) -c -o $@ $<
build/gen/np_bpf.h: build/np.bpf.o build/bpf2c
	@mkdir -p build/gen
	build/bpf2c $< $@
build/axia-netperm: $(NP_SRC) $(wildcard src/netperm/*.h) $(wildcard src/fw/*.h) src/sec/json.h $(NP_GEN)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(NP_CPP) $(CFLAGS) $(WARN) -std=c11 -o $@ $(NP_SRC)
# tests/npverify_test.c: the BTF reader, and (as root) every program through the kernel verifier
ifeq ($(BPF),1)
test: build/npverify-test
build/npverify-test: tests/npverify_test.c src/netperm/bpfsys.c $(wildcard src/netperm/*.h) $(NP_GEN)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/npverify_test.c src/netperm/bpfsys.c
	./$@ || { rm -f $@; exit 1; }
endif
test: build/npcompile-test
build/npcompile-test: tests/npcompile_test.c src/netperm/npcompile.c src/netperm/npdns.c src/fw/rules.c src/fw/dnsmap.c src/fw/fwmap.c src/sec/json.c $(wildcard src/netperm/*.h) $(wildcard src/fw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/npcompile_test.c src/netperm/npcompile.c src/netperm/npdns.c src/fw/rules.c src/fw/dnsmap.c src/fw/fwmap.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
# axia-fand (src/fan, docs/FANS.md): the root service that drives the fans by temperature. libc
# only; it executes nothing and has no network. tests/fan_test.c: curves, fan.json, requests and a
# fake sysfs (run by tests/run_suites.sh).
FAN_SRC := src/fan/fancurve.c src/fan/fanhw.c src/sec/json.c
build/axia-fand: src/fan/fand.c $(FAN_SRC) $(wildcard src/fan/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ src/fan/fand.c $(FAN_SRC)
test: build/ptrlock-test
build/ptrlock-test: tests/ptrlock_test.c src/wl/ptrlock.h src/testbench/lockkey.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -Isrc -o $@ tests/ptrlock_test.c
	./$@ || { rm -f $@; exit 1; }
test: build/fan-test
build/fan-test: tests/fan_test.c $(FAN_SRC) $(wildcard src/fan/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fan_test.c $(FAN_SRC)
all: $(ALL)

# Headless tests: terminal emulator (tests/vt_dump.c), OpenType shaping against
# HarfBuzz (tests/shape_test.c), bidi against BidiCharacterTest.txt (tests/bidi_test.c) and
# input methods (tests/ime_test.c); Thai line breaking (tests/thbrk_test.c, against libthai
# with tests/thbrk_compare.sh).
#
# Most suites run inside their own build rule (`./$@ || ...`). The ones below are only built here; once
# every prerequisite of `test` is done, tests/run_suites.sh runs them (a few seconds) and fails if any
# fails; a suite whose tool or data isn't installed says "skipped: needs X". `make test-full` adds the
# helpers (axia-media, axia-doc), the sandbox (Landlock kernel) and the HarfBuzz comparison.
test: build/vt-dump build/shape-test build/bidi-test build/ime-test build/thbrk-test build/sound-test build/wasm-test build/wasm-js-test build/fsops-test build/sign-test build/canvas-test build/arcpath-test build/ipp-test build/tp-test build/media-test
	@sh tests/run_suites.sh
test-full: test build/sandbox-test build/doc-test $(filter build/axia-media build/axia-doc,$(ALL))
	@sh tests/run_suites.sh full
SHAPE_SRC := src/ui/otf.c src/ui/shape.c src/ui/shape_indic.c src/ui/shape_syl.c src/ui/shape_khmer.c src/ui/shape_myanmar.c src/ui/shape_use.c src/ui/unicode.c
build/shape-test: tests/shape_test.c $(SHAPE_SRC) $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/shape_test.c $(SHAPE_SRC)
WASM_SRC := src/wasm/wasm.c src/wasm/wasm_run.c
build/wasm-js-test: tests/wasm_js_test.c src/ui/js_wasm.c src/ui/js_wasm.h $(WASM_SRC) src/wasm/wasm.h $(QJS_OBJ)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/wasm_js_test.c src/ui/js_wasm.c $(WASM_SRC) $(QJS_OBJ) -lm -lpthread
build/wasm-test: tests/wasm_test.c $(WASM_SRC) src/wasm/wasm.h src/wasm/wasm_int.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/wasm_test.c $(WASM_SRC) -lm
build/thbrk-test: tests/thbrk_test.c src/ui/thbrk.c src/ui/thbrk.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/thbrk_test.c src/ui/thbrk.c
# package signing: SHA-256 (NIST), Ed25519 (RFC 8032), JSON, contenido.json and catalog
build/pkgstate-test: tests/pkgstate_test.c src/sec/pkgstate.c src/sec/sign.c src/sec/json.c src/lpk.c src/pkg/crypto.c $(CODEC_LIB) $(wildcard src/sec/*.h)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/pkgstate_test.c src/sec/pkgstate.c src/sec/sign.c src/sec/json.c src/lpk.c src/pkg/crypto.c $(CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)
build/sandbox-test: tests/sandbox_test.c src/sec/sandbox.c src/sec/cgroup.c src/sec/sandbox.h src/sec/cgroup.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -D_GNU_SOURCE -o $@ tests/sandbox_test.c src/sec/sandbox.c src/sec/cgroup.c -lpthread
build/sign-test: tests/sign_test.c src/sec/json.c src/sec/sign.c src/lpk.c src/pkg/crypto.c $(CODEC_LIB) $(wildcard src/sec/*.h) src/lpk.h src/pkg/crypto.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -DLORD_SIGN_TEST -o $@ tests/sign_test.c src/sec/json.c src/sec/sign.c src/lpk.c src/pkg/crypto.c $(CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)
# Verified system code: manifest parser, signature, per-file check, reading under the root (docs/SIGNING.md)
test: build/codesig-test
build/codesig-test: tests/codesig_test.c src/sec/codesig.c src/sec/json.c src/pkg/crypto.c src/sec/codesig.h src/sec/json.h src/pkg/crypto.h build/gen/codesig_build.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/codesig_test.c src/sec/codesig.c src/sec/json.c src/pkg/crypto.c -lpthread
	./$@ ../tools/release/lordos-release.pub.pem || { rm -f $@; exit 1; }
# Clock time zones: TZif and POSIX rules against glibc (docs/SYSTEM.md)
# tests/xwm_test.c: Rust X11 wire helpers of the window manager (x11/wire.rs, docs/WAYLAND.md)
test: build/keygrab-test
build/keygrab-test: tests/keygrab_test.c src/wl/keygrab.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -o $@ tests/keygrab_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@

test test-platform: build/xwm-test
build/xwm-test: tests/xwm_test.c src/wl/x11wire.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/xwm_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
# tests/timer_order_test.c: the JS timers (src/ui/js_prelude.h) against the prelude they replaced
.PHONY: test-timers
test test-platform: test-timers
test-timers: build/timer-order-test
	./build/timer-order-test
build/timer-order-test: tests/timer_order_test.c tests/timer_js_baseline.h src/ui/js_prelude.h src/ui/js_timers.c src/ui/js_timers.h src/scheduler_ffi.h $(QJS_OBJ) $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/timer_order_test.c src/ui/js_timers.c $(QJS_OBJ) $(RUST_LIB) $(RUST_NATIVE_LIBS) -lm -lpthread
test: build/tzif-test
build/tzif-test: tests/tzif_test.c src/ui/tzif.h $(CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/tzif_test.c $(CODEC_LIB) $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }
# journal reader (Logs): normal and compact synthetic files, LZ4 and XZ, broken and
# truncated files; `build/journal-test dump <file>` dumps JSON to compare with journalctl
test: build/journal-test
build/journal-test: tests/journal_test.c src/logs/journal.c src/logs/journal.h src/pkg/xz.c $(CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/journal_test.c src/logs/journal.c src/pkg/xz.c $(CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }
# per-app firewall: rules (precedence, networks, JSON) and DNS names (broken packets, LRU)
test: build/fw-rules-test build/dnsmap-test build/broker-restrict-test
# the firewall's non-network restrictions in the real broker code (docs/FIREWALL.md); open() is
# wrapped so a fake /dev/video9 can be opened
BROKER_RESTRICT_SRC := src/sec/brokersrv.c src/sec/brokercam.c src/sec/restrict.c src/sec/perms.c src/sec/json.c \
                       src/fw/rules.c src/fw/fwmap.c src/media/proto.c
build/broker-restrict-test: tests/broker_restrict_test.c $(BROKER_RESTRICT_SRC) $(wildcard src/sec/*.h src/fw/*.h src/media/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/broker_restrict_test.c $(BROKER_RESTRICT_SRC) -Wl,--wrap=open
	./$@ || { rm -f $@; exit 1; }
# axia-maild for the isolated system apps (src/sec/brokermail.c): who may ask what, files, Mail-only verbs
test: build/brokermail-test
build/brokermail-test: tests/brokermail_test.c src/sec/brokermail.c src/sec/json.c $(wildcard src/sec/*.h src/mail/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/brokermail_test.c src/sec/brokermail.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
# tests/launch_cli.c: the real Linux app launcher outside the desktop, for tests/fwlinux/files.sh
# (built by `make test`, run by that script in the VM)
LAUNCH_CLI_SRC := src/sec/cgroup.c src/sec/sandbox.c \
                  src/sec/restrict.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
test: build/launch-cli
build/launch-cli: tests/launch_cli.c $(LAUNCH_CLI_SRC) src/apps.h $(wildcard src/sec/*.h src/fw/*.h src/hw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/launch_cli.c $(LAUNCH_CLI_SRC) $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
# tests/apps_actions_test.c: desktop actions (Actions= and [Desktop Action …]) in the real launcher
test: build/apps-actions-test
build/apps-actions-test: tests/apps_actions_test.c $(LAUNCH_CLI_SRC) src/apps.h $(wildcard src/sec/*.h src/fw/*.h src/hw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/apps_actions_test.c $(LAUNCH_CLI_SRC) $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
	./$@ || { rm -f $@; exit 1; }
# tests/defaults_test.c: the default app for a MIME type (user choice, system default, first
# declaring app) and launchWith('') over a fake XDG tree
test: build/defaults-test
build/defaults-test: tests/defaults_test.c $(LAUNCH_CLI_SRC) src/apps.h $(wildcard src/sec/*.h src/fw/*.h src/hw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/defaults_test.c $(LAUNCH_CLI_SRC) $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
	./$@ || { rm -f $@; exit 1; }
# tests/notifyd_cli.c: the real notification server on a private bus, for tests/fwlinux/notify.sh
NOTIFYD_CLI_SRC := src/hw/notifyd.c src/sec/appid.c src/hw/jbuf.c src/vault/dbus.c src/sec/restrict.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
test: build/notifyd-cli build/notifyd-test
build/notifyd-test: tests/notifyd_test.c $(NOTIFYD_CLI_SRC) $(wildcard src/hw/*.h src/sec/*.h src/fw/*.h src/vault/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/notifyd_test.c $(NOTIFYD_CLI_SRC)
	./$@ || { rm -f $@; exit 1; }
build/notifyd-cli: tests/notifyd_cli.c $(NOTIFYD_CLI_SRC) $(wildcard src/hw/*.h src/sec/*.h src/fw/*.h src/vault/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/notifyd_cli.c $(NOTIFYD_CLI_SRC)
# tests/pwguard_cli.c: the PipeWire guard against the session's PipeWire, for tests/fwlinux/mic.sh;
# tests/pwguard_test.c: its POD reader and node classes
PWGUARD_SRC := src/sec/pwguard.c src/sec/appid.c src/sec/restrict.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
test: build/pwguard-cli build/pwguard-test
build/pwguard-cli: tests/pwguard_cli.c $(PWGUARD_SRC) $(wildcard src/sec/*.h src/fw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/pwguard_cli.c $(PWGUARD_SRC)
build/pwguard-test: tests/pwguard_test.c $(PWGUARD_SRC) $(wildcard src/sec/*.h src/fw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/pwguard_test.c $(PWGUARD_SRC)
	./$@ || { rm -f $@; exit 1; }
# tests/wl_globals.c: the compositor's globals as a client sees them (tests/fwlinux/screen.sh)
build/wl-globals: tests/wl_globals.c
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -std=c11 -o $@ $< -lwayland-client
build/fw-rules-test: tests/fw_rules_test.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c $(wildcard src/fw/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fw_rules_test.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
# subscribed blocklists (0.8.5): raw parsers, the set, the compiled file, fuzzing (docs/FIREWALL.md, "Blocklists")
test: build/fwlists-test
build/fwlists-test: tests/fwlists_test.c src/fw/fwlists.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c $(wildcard src/fw/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fwlists_test.c src/fw/fwlists.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
build/fwlists-test-asan: tests/fwlists_test.c src/fw/fwlists.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c $(wildcard src/fw/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) -O1 -g -fsanitize=address,undefined -fno-sanitize-recover=undefined $(WARN) -std=c11 -o $@ tests/fwlists_test.c src/fw/fwlists.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
	./$@
# the destinations a user types in the rule sheet (tests/fw_dest_cases.txt), through the store's
# parser; apps/monitor/tests.rs checks the Rust editor against the same cases.
test: build/fwdest-test
build/fwdest-test: tests/fwdest_test.c tests/fw_dest_cases.txt tests/fw_domain_cases.txt src/fw/rules.c src/fw/fwmap.c src/sec/json.c $(wildcard src/fw/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fwdest_test.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
build/dnsmap-test: tests/dnsmap_test.c src/fw/dnsmap.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c $(wildcard src/fw/*.h) src/sec/json.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/dnsmap_test.c src/fw/dnsmap.c src/fw/rules.c src/fw/fwmap.c src/sec/json.c
	./$@ || { rm -f $@; exit 1; }
# backup engine: snapshots, hard links, restore, retention, interruptions, exclusions, encryption
test: build/backup-test
build/backup-test: tests/backup_test.c src/backup/engine.c src/backup/crypt.c src/backup/engine.h src/backup/crypt.h src/fsops.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/backup_test.c src/backup/engine.c src/backup/crypt.c -lpthread $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
	./$@ || { rm -f $@; exit 1; }
# Monitor collection, rendering and regression tests are in apps/monitor/*.rs.
# Keep the migrated regression suite in the standard test entry point.
test: test-rust
# canvas.c rows: SIMD (NEON on aarch64) against C, bit for bit, and how long each one takes
# UI engine (docs/DESIGN.md "Engine notes"): ellipsis in LTR/RTL/Arabic/Thai/emoji, per-side borders,
# per-corner radii, logical edges, transforms and hit testing, and the dragged window's frames against
# the golden hashes in tests/drag_golden_{cpu,gpu}.txt; `build/engine-test bench` times a 2000-row
# list and a window drag on one and two displays
ENGINE_SRC := src/gpu/gcontend.c src/ui/text.c src/ui/textlayout.c \
              src/ui/paint.c src/ui/otf.c src/ui/fonts.c src/ui/unicode.c src/ui/bidi.c src/ui/shape.c src/ui/shape_indic.c \
              src/ui/shape_syl.c src/ui/shape_khmer.c src/ui/shape_myanmar.c src/ui/shape_use.c src/ui/thbrk.c src/canvas.c
test: build/engine-test
build/engine-test: tests/engine_test.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(HDR) tests/drag_golden_cpu.txt tests/drag_golden_gpu.txt $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/engine_test.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
	./$@ || { rm -f $@; exit 1; }
# wlroots proof of concept (poc/axia-wlr.c, docs/WLROOTS.md): the UI engine on wlroots 0.20, with
# wlroots doing the backend, protocols and input. Built on request only, never installed.
WLR_PKGS := wlroots-0.20 wayland-server xkbcommon pixman-1 libdrm
build/axia-wlr: poc/axia-wlr.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(HDR) $(RUST_LIB)
	$(CC) $(CPPFLAGS) -DWLR_USE_UNSTABLE $(shell pkg-config --cflags $(WLR_PKGS)) $(CFLAGS) $(WARN) -std=c11 -o $@ \
	    poc/axia-wlr.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(shell pkg-config --libs $(WLR_PKGS)) $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
# the frame pipeline's decisions (Rust frame_pipeline.rs, docs/GPU.md "The frame pipeline"): LORD_PIPELINE, the auto
# mode, what a display does next, when input waits, and a display on a vblank grid with and without it
test: build/fpipe-test
build/fpipe-test: tests/fpipe_test.c src/fpipe.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fpipe_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
# Re-record only after an intentional rasterizer change has been reviewed and
# independent damage/composition checks pass. Never use this to mask a regression.
# The Rust font migration baseline is documented in docs/RUST-CODECS.md.
drag-golden: tests/engine_test.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(HDR) $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o build/engine-golden tests/engine_test.c $(ENGINE_SRC) src/outputs.c src/bench/lbinput.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
	./build/engine-golden drag-golden
build/shadow-ffi-test: tests/shadow_ffi_test.c src/canvas.c src/canvas.h src/shadow_ffi.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/shadow_ffi_test.c src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/canvas-test: tests/canvas_test.c src/canvas.c src/canvas.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -DLORD_CANVAS_TEST -o $@ tests/canvas_test.c src/canvas.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/fsops-test: tests/fsops_test.c src/fsops.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fsops_test.c -lpthread $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lm -lpthread
# touchpad driver (src/tp): gestures of the simulated touchpad (../tests/touchpad-sim) → what's recognized
build/tp-test: tests/tp_test.c src/tp/tp.h ../tests/touchpad-sim/gestures.c ../tests/touchpad-sim/gestures.h $(RUST_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/tp_test.c ../tests/touchpad-sim/gestures.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
# the on-screen keyboard: devices, allowed keys, the queue and XKB labels
test: build/osk-test
build/osk-test: tests/osk_test.c src/osk.h src/keyboard.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/osk_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm -lxkbcommon
	./$@ || (rm -f $@; exit 1)

build/ime-test: tests/ime_test.c src/ime.h src/keyboard.h $(RUST_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/ime_test.c $(RUST_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
build/bidi-test: tests/bidi_test.c src/ui/bidi.c src/ui/unicode.c $(HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/bidi_test.c src/ui/bidi.c src/ui/unicode.c
build/sound-test: tests/sound_test.c src/sound/pulse.c src/sound/pulse.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/sound_test.c src/sound/pulse.c
build/sound-cli: tests/sound_cli.c src/sound/snd.c src/sound/pulse.c src/sound/snd.h src/sound/pulse.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/sound_cli.c src/sound/snd.c src/sound/pulse.c -lm
# Test Wayland app with dmabuf (udmabuf): see docs/GPU.md. Needs libwayland-client.
CLIENT_PROTO := stable/xdg-shell/xdg-shell unstable/xdg-decoration/xdg-decoration-unstable-v1 stable/linux-dmabuf/linux-dmabuf-v1 stable/presentation-time/presentation-time \
                unstable/relative-pointer/relative-pointer-unstable-v1 unstable/pointer-constraints/pointer-constraints-unstable-v1 \
                unstable/pointer-gestures/pointer-gestures-unstable-v1 unstable/primary-selection/primary-selection-unstable-v1 \
                stable/viewporter/viewporter staging/fractional-scale/fractional-scale-v1 unstable/xdg-output/xdg-output-unstable-v1 \
                stable/tablet/tablet-v2 staging/cursor-shape/cursor-shape-v1 unstable/idle-inhibit/idle-inhibit-unstable-v1
CLIENT_GEN := $(foreach p,$(CLIENT_PROTO),build/gen/client/$(notdir $(p))-client-protocol.h build/gen/client/$(notdir $(p))-protocol.c)
define wl_client_proto
build/gen/client/$(notdir $(1))-client-protocol.h: $(WLPROTO)/$(1).xml
	@mkdir -p build/gen/client
	wayland-scanner client-header $$< $$@
build/gen/client/$(notdir $(1))-protocol.c: $(WLPROTO)/$(1).xml
	@mkdir -p build/gen/client
	wayland-scanner private-code $$< $$@
endef
$(foreach p,$(CLIENT_PROTO),$(eval $(call wl_client_proto,$(p))))
build/dmabuf-client: tests/dmabuf_client.c $(CLIENT_GEN)
	$(CC) $(CFLAGS) -D_GNU_SOURCE -Ibuild/gen $(WARN) -std=c11 -o $@ tests/dmabuf_client.c $(filter %.c,$(CLIENT_GEN)) -lwayland-client
# Who draws the frame: xdg-decoration and KDE's server-decoration modes, also switched live
# (tests/deco_client.c; run inside a session: WAYLAND_DISPLAY=wayland-0 build/deco-client)
build/deco-client: tests/deco_client.c $(CLIENT_GEN)
	$(CC) $(CFLAGS) -D_GNU_SOURCE -Ibuild/gen $(WARN) -std=c11 -o $@ tests/deco_client.c $(filter %.c,$(CLIENT_GEN)) -lwayland-client

# Clipboard and primary-selection offers bound to their selection (tests/xwayland/clip_offers.sh, docs/WAYLAND.md)
build/clipboard-x11-protocol: tests/clipboard_x11_protocol.c
	$(CC) $(CFLAGS) $(WARN) -std=c11 -D_DEFAULT_SOURCE -o $@ $< $(shell pkg-config --cflags --libs x11)
build/clipboard-native-protocol: tests/clipboard_native_protocol.c tests/clip_offers.c $(CLIENT_GEN)
	$(CC) $(CFLAGS) -D_GNU_SOURCE -Ibuild/gen $(WARN) -std=c11 -o $@ tests/clipboard_native_protocol.c $(filter %.c,$(CLIENT_GEN)) -lwayland-client
build/clip-offers: tests/clip_offers.c $(CLIENT_GEN)
	$(CC) $(CFLAGS) -D_GNU_SOURCE -Ibuild/gen $(WARN) -std=c11 -o $@ tests/clip_offers.c $(filter %.c,$(CLIENT_GEN)) -lwayland-client
# what slows the GPU under CPU load (tests/gpu_contention.c, docs/GPU.md): a test tool, never installed
GC_PKGS := gbm egl glesv2
build/gpu-contention: tests/gpu_contention.c src/gpu/gcontend.c src/gpu/gcontend.h
	@mkdir -p build
	$(CC) $(CFLAGS) $(WARN) -std=gnu11 -D_GNU_SOURCE $(shell pkg-config --cflags $(GC_PKGS)) -Isrc -o $@ tests/gpu_contention.c src/gpu/gcontend.c $(shell pkg-config --libs $(GC_PKGS)) -lpthread

# the player's way out of a failing hardware decoder (src/media/helper/lm_hwfall.h)
test: build/hwfall-test
build/hwfall-test: tests/hwfall_test.c src/media/helper/lm_hwfall.h
	$(CC) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/hwfall_test.c
	./$@ || { rm -f $@; exit 1; }

MEDIA_PKGS := libavcodec libavformat libavutil libswscale libswresample
MEDIA_VA := $(shell pkg-config --exists libva libva-drm 2>/dev/null && echo 1 || echo 0)
ifeq ($(MEDIA_VA),1)
MEDIA_PKGS += libva libva-drm
MEDIA_CPPFLAGS := -DLM_VAAPI
endif
LM_SRC := $(wildcard src/media/helper/*.c) src/media/proto.c src/sec/sandbox.c src/sec/cgroup.c
build/axia-media: $(LM_SRC) $(wildcard src/media/helper/*.h) src/media/proto.h src/media/aring.h src/sec/sandbox.h src/sec/cgroup.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(MEDIA_CPPFLAGS) $(shell pkg-config --cflags $(MEDIA_PKGS)) $(CFLAGS) $(WARN) -std=c11 -o $@ $(LM_SRC) \
	    $(shell pkg-config --libs $(MEDIA_PKGS)) -lpthread -lm
# axia-media with a decoder that fails after 30 frames (HWFALL=2: frames that can't be read back), to run the way
# out of it where there is no hardware (LM_HWFALL_TEST, docs/MEDIA.md); a test tool, never installed
HWFALL ?= 1
build/axia-media-hwfall: $(LM_SRC) $(wildcard src/media/helper/*.h) src/media/proto.h src/media/aring.h src/sec/sandbox.h src/sec/cgroup.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(MEDIA_CPPFLAGS) -DLM_HWFALL_TEST=$(HWFALL) $(shell pkg-config --cflags $(MEDIA_PKGS)) $(CFLAGS) $(WARN) -std=c11 -o $@ $(LM_SRC) \
	    $(shell pkg-config --libs $(MEDIA_PKGS)) -lpthread -lm
MEDIA_TEST_SRC := tests/media_test.c src/media/proto.c src/media/maudio.c src/media/mpulse.c src/sound/pulse.c
build/media-test: $(MEDIA_TEST_SRC) src/media/proto.h src/media/aring.h src/media/media_int.h src/media/mpulse.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $(MEDIA_TEST_SRC) -lm

build/vt-dump: tests/vt_dump.c build/obj/term/vt.o $(CODEC_LIB)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $^ $(LDLIBS) $(CODEC_NATIVE_LIBS)

build/gen/%-protocol.h: $(WLPROTO)/stable/xdg-shell/%.xml
	@mkdir -p build/gen
	wayland-scanner server-header $< $@
build/gen/%-protocol.c: $(WLPROTO)/stable/xdg-shell/%.xml
	@mkdir -p build/gen
	wayland-scanner private-code $< $@
build/gen/xdg-decoration-unstable-v1-protocol.h: $(WLPROTO)/unstable/xdg-decoration/xdg-decoration-unstable-v1.xml
	@mkdir -p build/gen
	wayland-scanner server-header $< $@
build/gen/xdg-decoration-unstable-v1-protocol.c: $(WLPROTO)/unstable/xdg-decoration/xdg-decoration-unstable-v1.xml
	@mkdir -p build/gen
	wayland-scanner private-code $< $@
define wl_proto
build/gen/$(notdir $(1))-protocol.h: $(WLPROTO)/$(1).xml
	@mkdir -p build/gen
	wayland-scanner server-header $$< $$@
build/gen/$(notdir $(1))-protocol.c: $(WLPROTO)/$(1).xml
	@mkdir -p build/gen
	wayland-scanner private-code $$< $$@
endef
$(foreach p,$(WL_MORE),$(eval $(call wl_proto,$(p))))

build/gen/%.o: build/gen/%.c
	$(CC) $(CPPFLAGS) $(CFLAGS) -w -c -o $@ $<

build/axia-server: $(OBJ) $(QJS_OBJ) $(WL_OBJ) $(RUST_LIB)
	$(CC) $(LDFLAGS) -o $@ $(filter-out $(RUST_LIB),$^) $(RUST_LIB) $(RUST_NATIVE_LIBS) $(LDLIBS)

# Version, channel and build of LordOS, shown in About and Settings → General (lord.version and
# lord.system.info()). Version and channel come from ../package.json ("version" and "lordos": {"channel"}),
# the only place to set them; the build is the date (SOURCE_DATE_EPOCH if set) and the git commit, or
# LORD_BUILD=... The header is rewritten only when its contents change.
LORD_VERSION := $(shell sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' ../package.json 2>/dev/null | head -1 | tr -cd 'A-Za-z0-9.+~-')
LORD_CHANNEL := $(shell sed -n 's/.*"channel": *"\([^"]*\)".*/\1/p' ../package.json 2>/dev/null | head -1 | tr -cd 'a-z0-9-')
LORD_BUILD ?= $(shell d=$$(date -u -d "@$${SOURCE_DATE_EPOCH:-$$(date +%s)}" +%Y%m%d 2>/dev/null || date -u +%Y%m%d); \
                      c=$$(git -C .. rev-parse --short=8 HEAD 2>/dev/null); echo "$$d$${c:+.$$c}")
build/gen/lord_version.h: FORCE
	@mkdir -p build/gen
	@printf '#define LORD_VERSION "%s"\n#define LORD_CHANNEL "%s"\n#define LORD_BUILD "%s"\n' \
	    '$(or $(LORD_VERSION),0.0.0)' '$(LORD_CHANNEL)' '$(shell printf %s '$(LORD_BUILD)' | tr -cd 'A-Za-z0-9.+~-')' > $@.new
	@if cmp -s $@.new $@; then rm -f $@.new; else mv $@.new $@; fi
build/obj/ui/js.o build/obj/ui/js_sys.o build/obj/ui/js_hw.o build/obj/term/pty.o: build/gen/lord_version.h
# Verified system code (src/sec/codesig.c, docs/SIGNING.md "Verified system code"). RELEASE=1: a
# release build, which only runs a manifest signed with the LordOS release key (no development mode).
# CODE_PUBKEY=<64 hex>: a TEST build that trusts a throwaway key instead of the release key (VM tests
# only; never for a release, and refused together with RELEASE=1).
RELEASE ?= 0
CODE_PUBKEY ?=
ifneq ($(and $(filter 1,$(RELEASE)),$(CODE_PUBKEY)),)
$(error CODE_PUBKEY is for test builds only, not with RELEASE=1)
endif
build/gen/codesig_build.h: FORCE
	@mkdir -p build/gen
	@{ [ '$(RELEASE)' = 1 ] && echo '#define LORD_CODE_RELEASE 1'; 	   k=$$(printf %s '$(CODE_PUBKEY)' | tr -cd '0-9a-f'); [ -n "$$k" ] && printf '#define LORD_CODE_PUBKEY_HEX "%s"\n' "$$k"; true; } > $@.new
	@if cmp -s $@.new $@; then rm -f $@.new; else mv $@.new $@; fi
build/obj/sec/codesig.o: build/gen/codesig_build.h
.PHONY: FORCE
FORCE:


build/obj/%.o: src/%.c $(HDR)
	@mkdir -p $(dir $@)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -c -o $@ $<

# Switching GPU builds must rebuild objects even without `make clean`.
build/gen/profile.h: FORCE
	@mkdir -p build/gen
	@printf '/* profile=desktop gpu=%s */\n' '$(GPU)' > $@.new
	@if cmp -s $@.new $@; then rm -f $@.new; else mv $@.new $@; fi
$(OBJ): build/gen/profile.h

# The CPU path's pixel loops (blur, rounded rectangles, blending) vectorize at -O3: the backdrop blur
# 9.6 → 5.3 ms and rounded rectangles 1.45× faster on x86-64, same pixels (a later -O3 wins over -O2)
build/obj/canvas.o: CFLAGS += -O3

# Third-party code: compiled as is, without our warnings.

build/qjs/%.o: $(QJS_DIR)/%.c
	@mkdir -p $(dir $@)
	$(CC) $(CFLAGS) -D_GNU_SOURCE -DCONFIG_VERSION=\"$$(cat $(QJS_DIR)/VERSION)\" -fwrapv -ffp-contract=off -w -c -o $@ $<

clean:
	rm -rf build

# ---- documents (M9) ----
LD_SRC := src/doc/helper.c src/doc/h_pdf.c src/doc/h_arc.c src/doc/h_font.c src/doc/h_make.c src/doc/wire.c src/doc/arcpath.c \
          src/sec/json.c src/sec/sandbox.c src/sec/cgroup.c
LD_PKGS := poppler-glib libarchive cairo-pdf cairo-ft freetype2 fontconfig
build/axia-doc: $(LD_SRC) $(CODEC_LIB) $(wildcard src/doc/*.h) src/sec/sandbox.h src/sec/cgroup.h src/sec/json.h
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 $(shell pkg-config --cflags $(LD_PKGS)) -o $@ $(LD_SRC) $(CODEC_LIB) $(shell pkg-config --libs $(LD_PKGS)) -lm $(CODEC_NATIVE_LIBS)
# rules for extracting without leaving the folder (tests/arcpath_test.c)
build/arcpath-test: tests/arcpath_test.c src/doc/arcpath.c src/doc/arcpath.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/arcpath_test.c src/doc/arcpath.c
# IPP encoder/decoder, mDNS and page ranges (tests/ipp_test.c)
# (0.8.5) the components' network connectors (src/sec/netconn.c) and what they need, for the tests
NETCONN_SRC := src/sec/netconn.c src/sec/cgroup.c src/fw/rules.c src/fw/fwmap.c
# the connectors: requests, sockets, the real connector process and its fail-closed start
test: build/netconn-test
build/netconn-test: tests/netconn_test.c $(NETCONN_SRC) src/sec/json.c src/sec/netconn.h src/sec/cgroup.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/netconn_test.c $(NETCONN_SRC) src/sec/json.c -lpthread
	./$@ || { rm -f $@; exit 1; }
build/ipp-test: tests/ipp_test.c src/print/ipp.c src/print/mdns.c src/print/cups.c src/print/phttp.c src/doc/wire.c src/sec/json.c $(NETCONN_SRC) $(wildcard src/print/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/ipp_test.c src/print/ipp.c src/print/mdns.c src/print/cups.c src/print/phttp.c src/doc/wire.c src/sec/json.c $(NETCONN_SRC) -lpthread
# axia-doc end to end: malicious archives, PDF and the lockdown (needs build/axia-doc)
build/doc-test: tests/doc_test.c src/doc/wire.c src/sec/json.c $(wildcard src/doc/*.h)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/doc_test.c src/doc/wire.c src/sec/json.c

# Passwords and the system keyring (docs/PASSWORDS.md): rules in mk/vault*.mk
include $(sort $(wildcard mk/*.mk))

# The modules above add compositor objects outside SRC; rebuild them on GPU
# changes too, so switching builds cannot retain a GPU object.
$(HW_OBJ) $(LAN_SRV_OBJ) $(LB_SRV_OBJ) $(MAILC_OBJ) $(VC_OBJ) build/obj/outputs.o: build/gen/profile.h
build/axia-bench: build/gen/profile.h


# SMART parsers (ATA and NVMe) of the Disks app: hand-built data, healthy/warning/failing
test: build/smart-test
build/smart-test: tests/smart_test.c src/disk/smart.c src/disk/disk.h src/admin.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/smart_test.c src/disk/smart.c
	./$@ || { rm -f $@; exit 1; }
# smart-* actions (smartmontools): which device names are accepted, JSON compaction and LOG pieces;
# Disk SMART helper tests (Monitor sensor coverage is in Rust).
test: build/smartctl-test
build/smartctl-test: tests/smartctl_test.c src/disk/smartctl.c src/disk/disk.h src/admin.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/smartctl_test.c src/disk/smartctl.c
	./$@ || { rm -f $@; exit 1; }

# a system with no administrator (src/users/admin_rescue.c): who may become one
test: build/admin-rescue-test
build/admin-rescue-test: tests/admin_rescue_test.c src/users/admin_rescue.c src/users/admin_rescue.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/admin_rescue_test.c src/users/admin_rescue.c
	./$@ || { rm -f $@; exit 1; }
# user folders (src/users/homedirs.c): languages, user-dirs.dirs, no overwriting, no symlinks
test: build/homedirs-test
build/homedirs-test: tests/homedirs_test.c src/users/homedirs.c src/users/homedirs.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/homedirs_test.c src/users/homedirs.c
	./$@ || { rm -f $@; exit 1; }
# hardware video decoding probe (src/media/hwdec.c, docs/VIDEO.md): virtual GPUs and virtual machines
test: build/hwdec-test
build/hwdec-test: tests/hwdec_test.c src/media/hwdec.c src/media/hwdec.h src/media/media_int.h src/media/proto.c src/media/proto.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/hwdec_test.c src/media/hwdec.c src/media/proto.c
	./$@ || { rm -f $@; exit 1; }


# Shared filesystem algorithms are Rust, including helper/test consumers.
build/fsops-test build/launch-cli build/apps-actions-test build/defaults-test build/backup-test: $(RUST_LIB)

# Separate target directories prevent secure-memory feature unification with
# the compositor's ordinary allocator, including during parallel builds.
$(CODEC_LIB): $(CODEC_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-codecs --target-dir $(abspath build/codecs) --release --locked --offline
$(VAULT_CODEC_LIB): $(CODEC_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-codecs --no-default-features --features secure-memory --target-dir $(abspath build/vault-codecs) --release --locked --offline
$(MAIL_CODEC_LIB): $(MAIL_CODEC_INPUTS)
	$(CARGO) build --manifest-path $(RUST_MANIFEST) -p axia-codecs --features mail --target-dir $(abspath build/mail-codecs) --release --locked --offline
.PHONY: test-codecs
test-codecs:
	$(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-codecs --target-dir $(abspath build/codecs) --locked --offline

build/obj/codecs-test-smem.o: src/vault/smem.c src/vault/smem.h
	@mkdir -p $(dir $@)
	$(CC) -D_GNU_SOURCE $(CFLAGS) -fPIC -c -o $@ $<
.PHONY: test-codecs-secure
test-codecs-secure: build/obj/codecs-test-smem.o
	RUSTFLAGS="$(RUSTFLAGS) -C link-arg=$(abspath $<) -C link-arg=-lpthread" $(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-codecs --no-default-features --features secure-memory --target-dir $(abspath build/vault-codecs-tests) --locked --offline
test: test-codecs test-codecs-secure

.PHONY: test-crypto
test-crypto:
	$(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-crypto --target-dir $(abspath build/crypto-tests) --locked --offline
test: test-crypto

.PHONY: test-mail-rust
test-mail-rust:
	$(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-mail --target-dir $(abspath build/mail-tests) --locked --offline
test: test-mail-rust

# Rust compatibility tests call the actual C format consumers against immutable
# files produced by the previous crypto backend. Crypto itself is Rust only.
CRYPTO_FORMAT_SRC := src/vault/vfile.c src/vault/vcrypto.c src/pkg/crypto.c src/vault/smem.c src/backup/crypt.c
build/crypto-format-test.so: $(CRYPTO_FORMAT_SRC) src/crypto_ffi.h $(wildcard src/vault/*.h) src/backup/crypt.h $(CODEC_LIB)
	@mkdir -p build
	printf '%s\n' '{ global: vf_*; bkc_*; sm_free; local: *; };' > build/crypto-format-test.map
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -shared -fPIC -o $@ $(CRYPTO_FORMAT_SRC) $(CODEC_LIB) -Wl,--version-script=build/crypto-format-test.map $(CODEC_NATIVE_LIBS)
.PHONY: test-crypto-native
test-crypto-native: build/crypto-format-test.so
	AXIA_CRYPTO_FORMAT_LIBRARY=$(abspath $<) $(CARGO) test --manifest-path $(RUST_MANIFEST) -p axia-crypto --target-dir $(abspath build/crypto-tests) --locked --offline --test formats existing_vault_backup_slots_and_attachment -- --ignored
test: test-crypto-native
