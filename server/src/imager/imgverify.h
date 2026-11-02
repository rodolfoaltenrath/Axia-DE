/* imgverify.h — pure checks for the LordOS Imager's admin actions (imager_admin.c) and its tests
 * (tests/imager_test.c). No I/O except through the read callback; see docs/IMAGER.md.
 *
 *   - the release signature: SHA256SUMS.sig is base64 of the Ed25519 signature over the exact bytes
 *     of SHA256SUMS, made with the LordOS release key (docs/SIGNING.md, "Release files");
 *   - SHA256SUMS lines: "<64 hex>  <name>" (also "<hex> *<name>");
 *   - image names: what a release ships and what the imager writes;
 *   - the uncompressed size of an .xz file, from its index (to refuse a card that is too small
 *     before anything is erased). */
#ifndef LORD_IMGVERIFY_H
#define LORD_IMGVERIFY_H

#include <stddef.h>
#include <stdint.h>

#define IMG_SUMS_MAX  65536             /* SHA256SUMS: a few lines */
#define IMG_SIG_MAX   512               /* SHA256SUMS.sig: 88 bytes of base64 and a newline */

/* 0 if sig (base64, optional trailing newline) is a valid Ed25519 signature of sums by pub. */
int img_sig_verify(const uint8_t *sums, size_t n, const char *sig, size_t sn, const uint8_t pub[32]);
/* Finds name in SHA256SUMS; 0 and the hash in out, -1 if missing, -2 if the file is malformed or
 * lists the name twice. */
int img_sums_find(const char *sums, size_t n, const char *name, uint8_t out[32]);
/* A file name the imager accepts: [A-Za-z0-9._+~-], 1..128, not starting with '.' or '-'. */
int img_valid_name(const char *name);
/* Kind of image by suffix: 1 raw (.img, .iso), 2 xz (.img.xz), 0 anything else. */
int img_kind(const char *name);
/* A release tag: v<digits>.<digits>.<digits>[-<[a-z0-9.]+>], up to 32 characters. */
int img_valid_tag(const char *tag);

typedef int (*ImgRead)(void *ud, uint64_t off, void *buf, size_t n);   /* 0 if all n bytes were read */
/* Uncompressed size of an .xz file of fsize bytes (one or more streams, stream padding allowed),
 * from the stream footers and indexes (CRC-checked). 0 ok; -1 not xz or damaged. */
int img_xz_size(ImgRead rd, void *ud, uint64_t fsize, uint64_t *out);

uint32_t img_crc32(uint32_t crc, const void *p, size_t n);

#endif
