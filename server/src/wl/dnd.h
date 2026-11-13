/* ABI only. Implementation and tests: rust/axia-shell/src/wayland/dnd*.rs. */
#ifndef AXIA_DND_H
#define AXIA_DND_H
#include <wayland-server-core.h>
#include <stdint.h>
void dnd_init(struct wl_display *display, struct wl_list *devices);
void dnd_shutdown(void);
typedef struct DndSource DndSource;
void dnd_source_free(DndSource *source);
int dnd_source_used(const DndSource *source);
int dnd_source_is_drag(const DndSource *source);
void dnd_source_actions(struct wl_resource *resource, DndSource **source, uint32_t actions);
int dnd_start_source(struct wl_resource *source, struct wl_resource *origin, const char *const *mimes, int count, DndSource **state, uint32_t button);
int dnd_start_files(const char *paths, int origin_node, int internal_move);
int dnd_active(void);
int dnd_origin_node(void);
void dnd_cancel(void);
int dnd_key(uint32_t code, int value, int alt);
/* Called with the existing compositor hit test, not a second scene traversal. */
void dnd_pointer(struct wl_resource *surface, int token, float x, float y, int moved, int released, uint32_t time);
void dnd_events(void (*callback)(int id, const char *type, const char *json));
int dnd_button(uint32_t button, int down);
void dnd_record_press(struct wl_resource *origin, uint32_t serial, uint32_t button);
int dnd_press_button(struct wl_client *client, struct wl_resource *origin, uint32_t serial, struct wl_resource *focus);
void dnd_surface_gone(struct wl_resource *surface);
int node_set_file_drop(int id, uint32_t token);
struct Node;
uint32_t node_file_drop(const struct Node *node);
#endif
