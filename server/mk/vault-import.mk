# Importing/exporting passwords (src/vault/vimport.c, kdbx.c and the CSV and XML readers of axia-codecs; docs/PASSWORDS.md).
# The test uses smem over malloc (tests/import_smem_shim.c) so ASan/LSan see every block.
IMPORT_SRC := src/vault/vimport.c src/vault/kdbx.c
IMPORT_HDR := src/vault/vimport.h src/vault/csvp.h src/vault/kdbx.h src/vault/xmlmini.h src/vault/vjson.h src/vault/smem.h src/codecs_ffi.h src/vault/vcrypto.h
IMPORT_SAN := $(shell echo 'int main(void){return 0;}' | $(CC) -fsanitize=address,undefined -x c - -o /dev/null 2>/dev/null && echo -fsanitize=address,undefined -fno-omit-frame-pointer)
build/import-test: tests/import_test.c tests/import_smem_shim.c $(IMPORT_SRC) $(IMPORT_HDR) src/vault/vjson.c src/vault/vcrypto.c src/pkg/crypto.c $(VAULT_CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(CFLAGS) $(WARN) $(IMPORT_SAN) -std=c11 -o $@ tests/import_test.c tests/import_smem_shim.c $(IMPORT_SRC) \
	    src/vault/vjson.c src/vault/vcrypto.c src/pkg/crypto.c $(VAULT_CODEC_LIB) -lm $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }
test: build/import-test
