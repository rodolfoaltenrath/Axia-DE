# Importing/exporting passwords (src/vault/vimport.c, csvp.c, kdbx.c, xmlmini.c; docs/PASSWORDS.md).
# The test uses smem over malloc (tests/import_smem_shim.c) so ASan/LSan see every block.
IMPORT_SRC := src/vault/vimport.c src/vault/csvp.c src/vault/kdbx.c src/vault/xmlmini.c
IMPORT_HDR := src/vault/vimport.h src/vault/csvp.h src/vault/kdbx.h src/vault/xmlmini.h src/vault/vjson.h src/vault/smem.h src/vault/vstb.h src/vault/vcrypto.h
IMPORT_SAN := $(shell echo 'int main(void){return 0;}' | $(CC) -fsanitize=address,undefined -x c - -o /dev/null 2>/dev/null && echo -fsanitize=address,undefined -fno-omit-frame-pointer)
build/import-test: tests/import_test.c tests/import_smem_shim.c $(IMPORT_SRC) $(IMPORT_HDR) src/vault/vjson.c src/vault/vcrypto.c src/pkg/crypto.c build/obj/vault/vstb.o build/obj/vault/monocypher.o
	@mkdir -p build
	$(CC) $(CPPFLAGS) $(VAULT_CPP) $(CFLAGS) $(WARN) $(IMPORT_SAN) -std=c11 -o $@ tests/import_test.c tests/import_smem_shim.c $(IMPORT_SRC) \
	    src/vault/vjson.c src/vault/vcrypto.c src/pkg/crypto.c build/obj/vault/vstb.o build/obj/vault/monocypher.o -lm
	./$@ || { rm -f $@; exit 1; }
test: build/import-test
