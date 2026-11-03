/* Keep a wallpaper alive while two displays reference it; release decoded originals
 * and both scale caches when the last node goes away. Theme assets keep their cache. */
#include <assert.h>
#include <sys/stat.h>
#include <unistd.h>
#include "../src/ui/image.c"
#include "stb_image_write.h"
int main(void) {
    char root[]="/tmp/axia-image-XXXXXX",dir[256],path[256];
    assert(mkdtemp(root));
    snprintf(dir,sizeof dir,"%s/axia",root);assert(!mkdir(dir,0700));
    snprintf(dir,sizeof dir,"%s/axia/wallpapers",root);assert(!mkdir(dir,0700));
    snprintf(path,sizeof path,"%s/picture.png",dir);
    uint8_t rgba[16*16*4];memset(rgba,255,sizeof rgba);
    assert(stbi_write_png(path,16,16,4,rgba,16*4));
    Image *a=image_get(path),*b=image_get(path);assert(a && a==b && a->refs==2);
    uint32_t pixels[32*32];Canvas c;canvas_init(&c,pixels,32,32,32);
    image_draw(&c,a,0,0,24,24,0,FIT_COVER,1);
    image_draw(&c,a,0,0,32,32,0,FIT_COVER,1);
    assert(a->scaled && a->scaled2);
    image_release(a);assert(images==b && b->refs==1);
    image_release(b);assert(!images);
    assert(!unlink(path));assert(!rmdir(dir));
    snprintf(dir,sizeof dir,"%s/axia",root);assert(!rmdir(dir));assert(!rmdir(root));
    puts("wallpaper images: shared displays retain pixels; last release frees all scale caches");return 0;
}
