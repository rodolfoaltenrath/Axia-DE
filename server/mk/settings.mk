# Settings' hardware pages (src/hw, docs/SETTINGS.md): displays (EDID, modes, Night Light),
# Bluetooth (BlueZ over our own D-Bus client), battery and power, date and time, default apps.
# Objects are added to axia-server as extra prerequisites ($^ in its link rule takes them).
HW_SRC := src/hw/jbuf.c src/hw/display.c \
          src/hw/sysbus.c src/hw/bt.c src/hw/ppd.c src/hw/hw.c src/hw/notifyd.c src/ui/js_hw.c \
          src/sysreport/hwsnap.c \
          src/ui/js_fan.c
HW_HDR := $(wildcard src/hw/*.h) src/apps.h $(wildcard src/sysreport/*.h) $(wildcard src/fan/*.h) src/ui/js_hw.h src/ui/js_fan.h src/kms.h src/vault/dbus.h
HW_OBJ := $(HW_SRC:src/%.c=build/obj/%.o)
$(HW_OBJ): $(HW_HDR)
build/axia-server: $(HW_OBJ)
# pure parts: EDID, mode selection, gamma/temperature/sun math, mimeapps.list and globs2,
# and the admin actions' validators
test: build/settings-hw-test
build/settings-hw-test: tests/settings_hw_test.c src/hw/hw_admin.c src/hw/jbuf.c src/sec/json.c $(HW_HDR) src/bench/boottest.h $(RUST_LIB) $(BENCH_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/settings_hw_test.c src/hw/hw_admin.c src/hw/jbuf.c src/sec/json.c $(RUST_LIB) $(BENCH_LIB) $(RUST_NATIVE_LIBS) -ldl -lpthread -lm
	./$@ || { rm -f $@; exit 1; }
# BlueZ parsing (GetManagedObjects, PropertiesChanged, Battery1, paths, state JSON) without a bus
test: build/bt-test
BT_TEST_SRC := tests/bt_test.c src/hw/bt.c src/hw/sysbus.c src/hw/jbuf.c src/vault/dbus.c src/sec/json.c
build/bt-test: $(BT_TEST_SRC) $(HW_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $(BT_TEST_SRC)
	./$@ || { rm -f $@; exit 1; }
# power_supply, uevents, the Store's hardware facts and the browsers' title bar switch live in
# rust/axia-hw (src/hw/{power,uevent,hwscan,brframe}.h are their C ABI); their tests are cargo tests there
