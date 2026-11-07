/* Rust-owned native storage/transfers; call on the compositor event-loop thread.
 * Borrowed data remains valid until the next mutation of that channel. Transfer
 * constructors take ownership of fd even on failure. Free after removing its
 * Wayland event source. poll: 0 pending, 1 complete, -1 rejected/error. */
#ifndef AXIA_CLIPBOARD_NATIVE_H
#define AXIA_CLIPBOARD_NATIVE_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
typedef struct NativeReader NativeReader;
typedef struct NativeWriter NativeWriter;
void axia_native_reset(void);
const char *axia_native_text(void);
uint32_t axia_native_text_gen(void);
bool axia_native_set_text(const unsigned char *text, size_t len);
void axia_native_mark(const char *origin, bool sensitive);
const char *axia_native_origin(void);
bool axia_native_sensitive(void);
uint32_t axia_native_image_gen(void);
bool axia_native_is_image(void);
const char *axia_native_image_path(void);
const char *axia_native_image_origin(void);
bool axia_native_image_sensitive(void);
const unsigned char *axia_native_png(size_t *len);
void axia_native_text_active(void);
uint32_t axia_native_offer_gen(void);
void axia_native_offer_advance(void);
void axia_native_invalidate(bool primary);
const char *axia_native_primary(size_t *len, uint32_t *gen);
bool axia_native_set_primary(const unsigned char *text, size_t len);
bool axia_native_set_png(const unsigned char *png, size_t len, const char *origin, bool sensitive);
bool axia_native_use_image(const char *path);
int axia_native_mime_rank(const char *mime);
int axia_native_mime_choose(const char *const *mimes, size_t count);
bool axia_native_mime_secret(const char *const *mimes, size_t count);
NativeReader *axia_native_read_new(int fd, int kind, const char *origin, bool sensitive);
int axia_native_read_poll(NativeReader *reader);
bool axia_native_read_expired(const NativeReader *reader);
void axia_native_read_free(NativeReader *reader);
NativeWriter *axia_native_write_new(int fd, int kind);
int axia_native_write_poll(NativeWriter *writer);
bool axia_native_write_expired(const NativeWriter *writer);
void axia_native_write_free(NativeWriter *writer);
typedef struct NativeAccumulator NativeAccumulator;
typedef struct NativeSnapshot NativeSnapshot;
NativeAccumulator *axia_native_acc_new(bool primary);
bool axia_native_acc_append(NativeAccumulator *acc, const unsigned char *data, size_t len);
size_t axia_native_acc_finish(NativeAccumulator *acc, int kind, bool latin1, const char *origin);
void axia_native_acc_free(NativeAccumulator *acc);
NativeSnapshot *axia_native_snapshot_new(int kind, bool latin1);
const unsigned char *axia_native_snapshot_data(const NativeSnapshot *snapshot, size_t *len);
void axia_native_snapshot_free(NativeSnapshot *snapshot);
#endif
