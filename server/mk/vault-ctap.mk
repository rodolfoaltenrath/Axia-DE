# FIDO2 for Passwords (src/vault/ctap2.c, p256.c, fidodev.c, the CBOR of axia-codecs; docs/PASSWORDS.md).
# ctap-test runs with `make test`; softfido and fido-e2e need root (uhid): see
# ../tests/fido-soft/run-pw-116.sh. The prerequisites are written by hand (this file is
# included before mk/vault.mk).
CTAP_SRC := src/vault/ctap2.c src/vault/p256.c src/vault/fidodev.c src/pkg/crypto.c $(CODEC_LIB)
CTAP_HDR := src/vault/ctap2.h src/vault/p256.h src/vault/cbor.h src/vault/fidodev.h src/vault/vcrypto.h src/pkg/crypto.h
build/ctap-test: tests/ctap_test.c $(CTAP_SRC) $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/ctap_test.c $(CTAP_SRC) $(CODEC_NATIVE_LIBS) -lpthread
	./$@ || { rm -f $@; exit 1; }
test: build/ctap-test
build/fido-e2e: tests/fido_e2e.c $(CTAP_SRC) $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ tests/fido_e2e.c $(CTAP_SRC) $(CODEC_NATIVE_LIBS) -lpthread
build/softfido: ../tests/fido-soft/softfido.c src/vault/p256.c src/pkg/crypto.c $(CODEC_LIB) $(CTAP_HDR)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) -std=c11 -o $@ ../tests/fido-soft/softfido.c src/vault/p256.c src/pkg/crypto.c $(CODEC_LIB) $(CODEC_NATIVE_LIBS)
