# Passwords and the system keyring. Rust cryptography is included in the secure
# codecs archive, so each executable links only one Rust runtime/allocator.
# Common vault primitives (AES, SHA-1, HMAC, HKDF, secrets arena, Rust codecs on the arena)
VAULT_BASE := src/vault/smem.c src/vault/vcrypto.c src/pkg/crypto.c
build/vcrypto-test: tests/vcrypto_test.c $(VAULT_BASE) src/vault/vcrypto.h src/vault/smem.h
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/vcrypto_test.c $(VAULT_BASE) -lpthread
	./$@ || { rm -f $@; exit 1; }
test: build/vcrypto-test
# Lists embedded in axia-vault (EFF words, common passwords): ../third_party/wordlists
build/obj/vault/lists.o: ../third_party/wordlists/eff_large_wordlist.txt ../third_party/wordlists/10k-most-common.txt
	@mkdir -p build/gen/vault build/obj/vault
	cp ../third_party/wordlists/eff_large_wordlist.txt build/gen/vault/words.txt
	cp ../third_party/wordlists/10k-most-common.txt build/gen/vault/common.txt
	cd build/gen/vault && $(LD) -r -b binary -z noexecstack -o ../../obj/vault/lists.o words.txt common.txt
VAULT_CORE := src/vault/smem.c src/vault/vcrypto.c src/vault/vjson.c src/vault/vfile.c src/vault/vmodel.c src/vault/vgen.c \
              src/vault/totp.c src/vault/vwire.c src/pkg/crypto.c
VAULT_HDR := $(wildcard src/vault/*.h) src/sec/sandbox.h
# format, model, TOTP, generator, JSON and protocol (tests/vault_test.c)
build/vault-test: tests/vault_test.c $(VAULT_CORE) $(VAULT_CODEC_LIB) build/obj/vault/lists.o $(VAULT_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/vault_test.c $(VAULT_CORE) $(VAULT_CODEC_LIB) build/obj/vault/lists.o $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }
test: build/vault-test
# axia-vault: the confined Passwords and keyring helper (src/vault/vault.c)
LV_SRC := $(VAULT_CORE) src/vault/vault.c src/vault/keyring.c src/vault/vimport.c src/vault/csvp.c src/vault/kdbx.c \
          src/vault/qr.c src/vault/ctap2.c src/vault/p256.c src/vault/secretsvc.c src/vault/dbus.c \
          src/sec/sandbox.c src/sec/cgroup.c
build/axia-vault: $(LV_SRC) $(VAULT_CODEC_LIB) build/obj/vault/lists.o $(VAULT_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ $(LV_SRC) $(VAULT_CODEC_LIB) build/obj/vault/lists.o -lpthread -lm $(CODEC_NATIVE_LIBS)
# (only when all its parts are there, so the tree always builds)
LV_READY := $(and $(wildcard src/vault/secretsvc.c),$(wildcard src/vault/dbus.c),$(wildcard src/vault/vimport.c),$(wildcard src/vault/kdbx.c),$(wildcard src/vault/csvp.c))
ifneq ($(LV_READY),)
all: build/axia-vault
endif
# axia-server side: launching axia-vault, its channel and lord.vault (src/vault/vclient.c, src/ui/js_vault.c).
# They are added to build/axia-server as prerequisites ($^ includes them in the link).
VC_OBJ := build/obj/vault/vclient.o build/obj/vault/vwire.o build/obj/vault/fidodev.o build/obj/ui/js_vault.o
build/axia-server: $(VC_OBJ)
$(VC_OBJ): $(wildcard src/vault/vclient.h src/vault/vwire.h src/vault/fidodev.h src/ui/js_vault.h)
# axia-vault end to end with axia-server's client (needs build/axia-vault; does not run by itself)
build/vault-e2e: tests/vault_e2e.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c src/vault/vjson.c src/vault/smem.c $(VAULT_HDR) build/axia-vault
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/vault_e2e.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c src/vault/vjson.c src/vault/smem.c -lpthread
# the keyring op of isolated .lpk apps (src/sec/brokerkr.c) against axia-vault (needs build/axia-vault)
build/brokerkr-e2e: tests/brokerkr_e2e.c src/sec/brokerkr.c src/sec/json.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c src/sec/broker_int.h src/sec/broker.h $(VAULT_HDR) build/axia-vault
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/brokerkr_e2e.c src/sec/brokerkr.c src/sec/json.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c -lpthread
# demo vault for the screenshots (not a test)
build/vault-demo: tests/vault_demo.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c src/vault/vjson.c src/vault/smem.c $(VAULT_HDR)
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/vault_demo.c src/vault/vclient.c src/vault/vwire.c src/vault/fidodev.c src/vault/vjson.c src/vault/smem.c -lpthread
