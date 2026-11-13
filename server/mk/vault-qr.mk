# Our own QR codes (rust/axia-codecs/src/qr.rs, the C ABI of src/vault/qr.h): known answers (segno), round trip, error
# correction, real images (tests/qr/) and fuzz. With ASan/UBSan if the compiler has them.
QR_SAN := $(shell echo 'int main(void){return 0;}' | $(CC) -fsanitize=address,undefined -x c -o /dev/null - 2>/dev/null && echo -fsanitize=address,undefined)
build/qr-test: tests/qr_test.c src/vault/qr.h src/vault/smem.c src/vault/smem.h tests/qr/qr_ref.h $(VAULT_CODEC_LIB)
	@mkdir -p build
	$(CC) $(CPPFLAGS) -Itests $(CFLAGS) $(QR_SAN) $(WARN) -std=c11 -o $@ tests/qr_test.c src/vault/smem.c $(VAULT_CODEC_LIB) -lm -lpthread $(CODEC_NATIVE_LIBS)
	./$@ || { rm -f $@; exit 1; }
test: build/qr-test
