/* Exercise the real main-loop cursor path with KMS calls recorded, without
 * opening a DRM device or moving the user's pointer. */
#define main axia_session_main
#include "../src/main.c"
#undef main

static int checks, uploads, moves, texture_uploads, fail_upload, gpu_on;
static int last_x, last_y, client_mode, viewers, recording;
static unsigned client_gen = 1;
static uint32_t pixels[80 * 80];
static CursorImage image = { pixels, 32, 32, 3, 4 };
Ui ui;
#define CHECK(c) do { checks++; if (!(c)) { fprintf(stderr, "FAIL line %d: %s\n", __LINE__, #c); return 1; } } while (0)

int gpu_active(void) { return gpu_on; }
int mirror_watchers(void) { return viewers; }
int media_needs_cpu(void) { return recording; }
GpuTex *gpu_tex_pixels(GpuTex *t, const uint32_t *p, int w, int h, int opaque) {
    (void)t; (void)p; (void)w; (void)h; (void)opaque;
    texture_uploads++; return (GpuTex *)1;
}
int grab_active(void) { return 0; }
int grab_cursor(void) { return CUR_DEFAULT; }
int wl_pointer_locked(void) { return 0; }
int wl_cursor(const uint32_t **p, int *w, int *h, int *hx, int *hy, unsigned *gen) {
    *gen = client_gen;
    if (client_mode == 1) { *p = image.px; *w = image.w; *h = image.h; *hx = image.hx; *hy = image.hy; }
    return client_mode;
}
const CursorImage *cursor_image_pct(int shape, int pct) { (void)shape; (void)pct; return &image; }
void canvas_init(Canvas *c, uint32_t *p, int w, int h, int stride) { *c = (Canvas){ .px=p, .w=w, .h=h, .stride=stride }; }
void canvas_bitmap(Canvas *c, int x, int y, const char *const *rows, uint32_t fg, uint32_t bg) {
    (void)c; (void)x; (void)y; (void)rows; (void)fg; (void)bg;
}
int kms_cursor_image(Kms *k, const uint32_t *p, int w, int h, int hx, int hy) {
    (void)p; (void)w; (void)h; (void)hx; (void)hy; uploads++;
    k->cursor_ok = k->cursor_visible = !fail_upload;
    return fail_upload ? -1 : 0;
}
void kms_cursor_show(Kms *k, int visible) { if (k->cursor_ok) k->cursor_visible=visible; }
void kms_cursor_move(Kms *k, int x, int y) { if (k->cursor_ok && k->cursor_visible) { moves++; last_x=x; last_y=y; } }

static Server setup(void) {
    Server s = {0};
    s.out[0].on=1; s.out[0].gpu=-1; s.out[0].kms.fd=9;
    s.out[0].v=out_view(0,0,1366,768,100); s.out[0].d.scale=100;
    s.cx=100; s.cy=200; s.over_app=1;
    return s;
}
int main(void) {
    unsetenv("LORD_SW_CURSOR");
    Server s=setup();
    cpu_cursor_start(&s);
    CHECK(s.hw_cursor && uploads==1 && texture_uploads==0);
    CHECK(last_x==97 && last_y==196);
    CHECK(s.out[0].full_copy==2);
    s.out[0].full_copy=0; s.out[0].kms.flip_pending=1;
    int before=moves;
    for (int i=0;i<1000;i++) {
        s.cx=100+i; cursor_motion(&s); update_cursor(&s);
    }
    CHECK(moves-before==1000); /* cursor moves while the scene's flip is pending */
    CHECK(uploads==1 && texture_uploads==0); /* no per-motion image/texture uploads */
    CHECK(!s.cursor_dirty && !s.out[0].cdirty && !out_needs(&s,&s.out[0]));
    viewers=1; cursor_motion(&s); CHECK(s.cursor_dirty); /* remote cursor updates preserved */
    viewers=0; s.cursor_dirty=0;
    recording=1; cursor_motion(&s); CHECK(s.cursor_dirty);
    recording=0; s.cursor_dirty=0;

    client_mode=2; client_gen++;
    update_cursor(&s);
    CHECK(!s.out[0].kms.cursor_visible);
    before=moves; hw_cursor_move(&s); CHECK(moves==before);
    client_mode=1; client_gen++;
    update_cursor(&s);
    CHECK(s.out[0].kms.cursor_visible && uploads==2);
    image.hx=7; client_gen++; update_cursor(&s);
    CHECK(uploads==3 && last_x==s.cx-7);

    image.w=80; client_gen++; update_cursor(&s);
    CHECK(!s.hw_cursor && !s.out[0].kms.cursor_visible);
    CHECK(s.cursor_dirty && s.out[0].cdirty && s.out[0].full_copy==2);
    CHECK(out_needs(&s,&s.out[0]));
    before=moves; s.cursor_dirty=0; cursor_motion(&s);
    CHECK(s.cursor_dirty && moves==before); /* software fallback keeps tracking */
    image.w=32; image.hx=3; client_mode=0;

    s=setup(); fail_upload=1; cpu_cursor_start(&s);
    CHECK(!s.hw_cursor && s.cursor_dirty);
    fail_upload=0;
    int count=uploads;
    s=setup(); s.headless=1; cpu_cursor_start(&s); CHECK(!s.hw_cursor && uploads==count);
    s=setup(); s.out[0].d.rot=1; cpu_cursor_start(&s); CHECK(!s.hw_cursor && uploads==count);
    s=setup(); s.out[1].on=1; cpu_cursor_start(&s); CHECK(!s.hw_cursor && uploads==count);
    s=setup(); s.out[0].kms.fd=-1; cpu_cursor_start(&s); CHECK(!s.hw_cursor && uploads==count);
    s=setup(); setenv("LORD_SW_CURSOR","1",1); cpu_cursor_start(&s); CHECK(!s.hw_cursor && uploads==count);
    unsetenv("LORD_SW_CURSOR");

    /* GPU texture caching retains the existing path. */
    s=setup(); gpu_on=1; s.cursor_mode=-1;
    update_cursor(&s); CHECK(texture_uploads==1);
    update_cursor(&s); CHECK(texture_uploads==1);
    printf("cpu-cursor: %d checks passed; 1000 motions, 1 image upload, 0 scene redraw requests\n", checks);
    return 0;
}
