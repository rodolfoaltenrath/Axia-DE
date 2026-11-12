/* Test-only wire peers for the Rust Wayland input implementation and touch router. */
#ifndef AXIA_WAYLAND_INPUT_FIXTURE_H
#define AXIA_WAYLAND_INPUT_FIXTURE_H
#include <wayland-server-core.h>
#include <stddef.h>
#include <stdint.h>
typedef struct {
    int fd, alive;
    unsigned global;
    struct wl_client *client;
    struct wl_resource *seat, *surface, *pointers[6];
    unsigned char pending[8192];
    size_t used;
} WliPeer;
typedef struct { unsigned peer, object, opcode, n; int32_t args[8]; } WliEvent;
extern WliPeer wli_peers[2];
extern WliEvent wli_events[262144];
extern unsigned wli_nevents;
extern unsigned long long wli_digest;
extern uint64_t wli_time_ms;
extern int wli_focus, wli_hit;
extern const unsigned wli_versions[6];
void wli_init(void);
void wli_pump(void);
void wli_request(unsigned peer, unsigned object, unsigned opcode, const uint32_t *args, unsigned n);
void wli_bind_manager(unsigned peer, unsigned id, unsigned version);
void wli_finish_peer(unsigned peer);
void wli_finish(void);
const WliEvent *wli_last(unsigned peer, unsigned object, unsigned opcode);
#endif
