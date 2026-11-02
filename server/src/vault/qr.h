/* qr.h — our own QR codes: encode (the QR of a Wi-Fi network to join from a phone, the recovery
 * key) and decode (add a 2FA code from the image of an otpauth:// QR).
 *
 * Encode: byte/alphanumeric/numeric modes, versions 1–40, L/M/Q/H error correction, all 8 masks
 * with the ISO/IEC 18004 penalty. Decode: clean images (screenshots, exported PNG/JPEG, straight
 * and sharp photos): binarizes, finds the three finder patterns, builds the grid (with the
 * alignment pattern from version 2), reads the format, removes the mask and corrects errors
 * with Reed-Solomon. It is not a camera reader (no strong perspective). */
#ifndef LORD_VAULT_QR_H
#define LORD_VAULT_QR_H

#include <stddef.h>
#include <stdint.h>

enum { QR_ECC_L = 0, QR_ECC_M, QR_ECC_Q, QR_ECC_H };
#define QR_MAX_SIZE 177                 /* side length of version 40 */

/* Encodes len bytes as a QR with the requested correction level (raises the version until it fits).
 * modules: QR_MAX_SIZE*QR_MAX_SIZE bytes (1 = dark), row by row with side *size.
 * Returns the version (1–40) or -1 if it does not fit. */
int qr_encode(const uint8_t *data, size_t len, int ecc, uint8_t *modules, int *size);

/* Reads a QR from a 0xAARRGGBB image (w*h). out receives the bytes (with a trailing 0 if it fits).
 * Returns the length or -1 if none was found/it could not be read. */
int qr_decode(const uint32_t *argb, int w, int h, uint8_t *out, size_t outcap);

/* Same as qr_encode with a fixed mask (0–7; -1 = the best) and a minimum version; *mask_out is the one used. */
int qr_encode_ex(const uint8_t *data, size_t len, int ecc, int mask, int minver, uint8_t *modules, int *size, int *mask_out);

#endif
