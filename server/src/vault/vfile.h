/* vfile.h — the vault file format (.lvault), version 1. See docs/PASSWORDS.md,
 * "The format".
 *
 *   header     "LORDVLT\x1a", version, header length, vault id, dates, counter,
 *              and the "slots" (one per way of opening: password, key file, password +
 *              key file, FIDO2 key, recovery key, session password)
 *   body       nonce(24) length(8) XChaCha20-Poly1305(JSON content) mac(16)
 *              AD = the whole header: changing a slot or a parameter is detected on open
 *   attachments count(4) and for each: id(16) nonce(24) length(8) ciphertext mac(16)
 *   trailer    "LVLTEND\0" + total length(8): a truncated file is recognized
 *
 * The vault key (VK, 32 random bytes) is never derived from the password: each slot
 * stores it wrapped (XChaCha20-Poly1305) with a key derived from its factor (Argon2id for what
 * a person types, BLAKE2b for what already has entropy: key file, FIDO2 key output,
 * recovery key). This way ways of opening can be added or removed without re-encrypting
 * everything, and changing the password is rewriting one slot.
 *
 * Everything is little endian. */
#ifndef LORD_VAULT_VFILE_H
#define LORD_VAULT_VFILE_H

#include <stddef.h>
#include <stdint.h>

#define VF_MAGIC "LORDVLT\x1a"
#define VF_VERSION 1
#define VF_MAX_SLOTS 16
#define VF_HDR_FIXED 56
#define VF_MAX_BODY (256u << 20)
#define VF_MAX_ATT  (64u << 20)

enum { VS_PASSWORD = 1, VS_KEYFILE, VS_PASSKEY, VS_FIDO2, VS_RECOVERY, VS_SESSION };
enum { VK_ARGON2ID = 1, VK_BLAKE2B = 2 };
enum {
    VF_OK = 0, VF_EIO = -1, VF_EFORMAT = -2, VF_EVERSION = -3, VF_ETRUNC = -4,
    VF_EBADKEY = -5,       /* no factor opens a slot (wrong password, another key…) */
    VF_ECORRUPT = -6,      /* opened the slot but the body fails authentication: tampered */
    VF_ENOMEM = -7, VF_ENOSLOT = -8,
};

typedef struct { uint32_t m_kib, t_cost, lanes; } VKdf;

typedef struct {
    uint8_t type, kdf;
    uint8_t id[8];
    char label[64];
    VKdf p;                        /* VK_ARGON2ID */
    uint8_t salt[16];
    uint8_t cred[255]; uint8_t cred_len; uint8_t hmac_salt[32]; uint8_t aaguid[16];   /* VS_FIDO2 */
    uint8_t fido_pin;              /* VS_FIDO2: 1 if the key's PIN was used (changes the hmac-secret output) */
    uint64_t created;
    uint8_t nonce[24], wrapped[32], mac[16];
} VSlot;

typedef struct { uint8_t id[16]; uint64_t off, len; uint8_t nonce[24], mac[16]; } VAttLoc;  /* in the file */

typedef struct {
    uint8_t vault_id[16];
    uint64_t created, saved;
    uint32_t counter;
    uint16_t flags;
    VSlot slots[VF_MAX_SLOTS];
    int nslots;
    size_t hlen;                   /* header bytes (when reading) */
    /* when reading: where the body and attachments are */
    uint64_t body_off, body_len;
    VAttLoc *atts; int natts;      /* malloc (not secrets: ids and positions) */
} VHeader;

/* Factors supplied by the user (unused ones are NULL). */
typedef struct {
    const char *password;          /* VS_PASSWORD, VS_PASSKEY, VS_SESSION */
    const uint8_t *keyfile;        /* 32 bytes: BLAKE2b of the key file (vf_keyfile_hash) */
    const uint8_t *fido;           /* 32 bytes: hmac-secret output of the key */
    const char *recovery;          /* the recovery key as typed */
} VFactors;

/* Reads the header and table of the file (data = whole file or at least up to the trailer). */
int  vf_parse(const uint8_t *data, size_t len, VHeader *h);
void vf_header_free(VHeader *h);
/* Tries the slots of type `type` (0 = any that uses the given factors) and yields the VK.
 * *slot = index of the one that opened. VF_EBADKEY if none. */
int  vf_open_slot(const VHeader *h, int type, const VFactors *f, uint8_t vk[32], int *slot);
/* Decrypts the body with the VK → text in smem (sm_free). VF_ECORRUPT if it fails authentication. */
int  vf_read_body(const uint8_t *data, size_t len, const VHeader *h, const uint8_t vk[32], uint8_t **out, size_t *outlen);
/* Builds a new slot of the given type that wraps the VK (derives with the factors). */
int  vf_make_slot(const VHeader *h, VSlot *s, int type, const VFactors *f, const uint8_t vk[32], const VKdf *kdf, const char *label);
/* Attachments: encrypt (when adding) and decrypt (when requested). id/nonce/mac stay in the content. */
int  vf_att_seal(const uint8_t vk[32], const uint8_t vault_id[16], const uint8_t id[16], const uint8_t *plain, size_t len,
                 uint8_t *cipher, uint8_t nonce[24], uint8_t mac[16]);
int  vf_att_open(const uint8_t vk[32], const uint8_t vault_id[16], const uint8_t id[16], const uint8_t *cipher, size_t len,
                 const uint8_t nonce[24], const uint8_t mac[16], uint8_t *plain);

/* An attachment to write: the encrypted bytes come from memory or from the old file (fd, off). */
typedef struct { uint8_t id[16], nonce[24], mac[16]; const uint8_t *cipher; int srcfd; uint64_t off, len; } VAttOut;
/* Saves the vault in dirfd/name atomically: writes name.tmp-<pid>, fsync, keeps the previous
 * version as name.bak (hard link, replaced atomically) and renames. h->saved and h->counter
 * are updated. Returns VF_OK or VF_EIO. */
int  vf_save(int dirfd, const char *name, VHeader *h, const uint8_t vk[32], const uint8_t *plain, size_t plen,
             const VAttOut *atts, int natts);
/* Same but to an already open fd (exporting a copy): no .bak and no rename. */
int  vf_write_fd(int fd, VHeader *h, const uint8_t vk[32], const uint8_t *plain, size_t plen, const VAttOut *atts, int natts);

/* New header (random id). */
void vf_header_new(VHeader *h);
/* BLAKE2b-256 of a key file. */
void vf_keyfile_hash(const uint8_t *data, size_t len, uint8_t out[32]);
/* New recovery key: 40 Crockford base32 characters in 8 groups ("ABCDE-FGHJK-…"). */
void vf_recovery_new(char out[48]);
/* Argon2id parameters to take about target_ms on this machine (measures). */
void vf_calibrate(int target_ms, VKdf *out, int *measured_ms);
/* Derives with Argon2id (memory in smem). 0 ok. */
int  vf_argon2id(const uint8_t *pass, size_t plen, const uint8_t salt[16], const VKdf *p, uint8_t out[32]);
const char *vf_strerror(int err);

#endif
