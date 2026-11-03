/* A failed image upload must hide the previous cursor before software fallback. */
#define ioctl test_cursor_ioctl
#include "../src/kms.c"
#undef ioctl
#include <stdarg.h>

static int fail_image, uploads, hides;
int test_cursor_ioctl(int fd, unsigned long request, ...) {
    (void)fd;
    va_list ap; va_start(ap,request);
    void *arg=va_arg(ap,void *); va_end(ap);
    if (request==DRM_IOCTL_MODE_CURSOR2) {
        struct drm_mode_cursor2 *c=arg;
        if (!c->handle) { hides++; return 0; }
        uploads++;
        if (fail_image) { errno=EIO; return -1; }
    }
    return 0;
}
#define CHECK(c) do { if (!(c)) { fprintf(stderr,"FAIL line %d: %s\n",__LINE__,#c); return 1; } } while (0)
int main(void) {
    uint32_t map[64*64], image[32*32]={0};
    Kms k={ .fd=9, .crtc_id=53, .cursor_handle=7, .cursor_map=map,
            .cursor_size=sizeof map, .cursor_w=64, .cursor_h=64 };
    CHECK(kms_cursor_image(&k,image,32,32,0,0)==0);
    CHECK(k.cursor_ok && k.cursor_visible && uploads==1 && hides==0);
    fail_image=1;
    CHECK(kms_cursor_image(&k,image,32,32,0,0)==-1);
    CHECK(!k.cursor_ok && !k.cursor_visible && uploads==2 && hides==1);
    fail_image=0;
    CHECK(kms_cursor_image(&k,image,32,32,0,0)==0);
    CHECK(k.cursor_ok && k.cursor_visible);
    kms_cursor_show(&k,0);
    CHECK(!k.cursor_visible && hides==2);
    puts("kms-cursor: image failure hides the previous hardware cursor");
    return 0;
}
