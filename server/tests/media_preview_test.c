/* Previewing media must not resize helpers or evict the window's full-size cache. */
#include "../src/media/media_int.h"
#include "../src/ui/ui.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static MediaSrc source;
static unsigned sent;
int paint_preview_mode = 1;
PaintView paint_view = {0,0,0.25f,0,0,0};
MediaSrc *media_src_get(int id) { return id==1?&source:NULL; }
int mp_sendv(int fd, uint32_t type, uint32_t flags, int64_t a, int64_t b, int64_t d, int64_t e) {
    (void)fd;(void)type;(void)flags;(void)a;(void)b;(void)d;(void)e;sent++;return 0;
}
int main(void) {
    uint32_t image[200*100], cache[20*10], out[50*25];
    for (unsigned i=0;i<200*100;i++) image[i]=0xff22aa44;
    for (unsigned i=0;i<20*10;i++) cache[i]=0xff112233;
    source=(MediaSrc){.id=1,.kind=SK_PLAYER,.sock=7,.px=image,.w=200,.h=100,.opaque=1,
        .node=37,.box_w=200,.box_h=100,.sent_w=200,.sent_h=100,.cache=cache,.cache_w=20,.cache_h=10,.cache_seq=11};
    source.view.bg=0xff000000;
    Canvas c;canvas_init(&c,out,50,25,50);canvas_clear(&c,0xff000000);
    media_paint(&c,1,999,0,0,50,25,0,1);
    assert(sent==0 && source.node==37 && source.box_w==200 && source.box_h==100);
    assert(source.cache==cache && source.cache_w==20 && source.cache_h==10 && source.cache_seq==11);
    assert(cache[0]==0xff112233 && out[12*50+25]==0xff22aa44 && source.nmip==0);
    source.view.rot=90;source.view.mirror=1;source.view.zoom=1;
    media_paint(&c,1,999,0,0,50,25,0,1);
    assert(sent==0 && source.cache==cache && source.nmip==0);
    puts("media-preview: live pixels, helper size, normal cache, rotation and zoom preserved");
}
