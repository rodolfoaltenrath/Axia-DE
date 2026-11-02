# FIDO2 for Passwords (src/vault/ctap2.c, p256.c, cbor.c, fidodev.c; docs/PASSWORDS.md).
# ctap-test runs with `make test`; softfido and fido-e2e need root (uhid): see
# ../tests/fido-soft/run-pw-116.sh. The prerequisites are written by hand (this file is
# included before mk/vault.mk).
CTAP_SRC := src/vault/ctap2.c src/vault/p256.c src/vault/cbor.c src/vault/fidodev.c src/vault/vcrypto.c src/pkg/crypto.c
CTAP_HDR := src/vault/ctap2.h src/vault/p256.h src/vault/cbor.h src/vault/fidodev.h src/vault/vcrypto.h src/pkg/crypto.h
build/ctap-test: tests/ctap_test.c $(CTAP_SRC) $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/ctap_test.c $(CTAP_SRC) -lpthread
	./$@ || { rm -f $@; exit 1; }
test: build/ctap-test
build/fido-e2e: tests/fido_e2e.c $(CTAP_SRC) $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fido_e2e.c $(CTAP_SRC) -lpthread
build/softfido: ../tests/fido-soft/softfido.c src/vault/p256.c src/vault/cbor.c src/vault/vcrypto.c src/pkg/crypto.c build/obj/vault/monocypher.o $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(VAULT_CPP) $(CFLAGS) $(WARN) -std=c11 -o $@ ../tests/fido-soft/softfido.c src/vault/p256.c src/vault/cbor.c src/vault/vcrypto.c src/pkg/crypto.c build/obj/vault/monocypher.o
