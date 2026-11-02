# Secret Service (org.freedesktop.secrets) with our own D-Bus: src/vault/dbus.c and secretsvc.c
# (docs/PASSWORDS.md). dbus-test runs in make test; secretsvc-demo is tested with
# tests/secretsvc/run-pw-116.sh against the real bus and secret-tool.
SS_SRC := src/vault/dbus.c src/vault/secretsvc.c src/vault/smem.c src/vault/vcrypto.c src/pkg/crypto.c
SS_HDR := src/vault/dbus.h src/vault/secretsvc.h src/vault/smem.h src/vault/vcrypto.h src/pkg/crypto.h
SS_SAN := $(shell echo 'int main(void){return 0;}' | $(CC) -fsanitize=address,undefined -x c - -o /dev/null 2>/dev/null && echo -fsanitize=address,undefined -fno-omit-frame-pointer)
build/dbus-test: tests/dbus_test.c tests/ss_memback.c tests/ss_memback.h $(SS_SRC) $(SS_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) $(SS_SAN) -std=c11 -Itests -o $@ tests/dbus_test.c tests/ss_memback.c $(SS_SRC) -lpthread
	ASAN_OPTIONS=detect_leaks=0 ./$@ || { rm -f $@; exit 1; }
test: build/dbus-test
build/secretsvc-demo: tests/secretsvc_demo.c tests/ss_memback.c tests/ss_memback.h $(SS_SRC) $(SS_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -Itests -o $@ tests/secretsvc_demo.c tests/ss_memback.c $(SS_SRC) -lpthread
test: build/secretsvc-demo
