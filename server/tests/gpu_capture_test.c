/* Optional hardware probe on a render node. Never opens a display, requests
 * DRM master, changes scanout or connects to the user's Wayland session. */
#include "../src/wl/wl.c"
#include <assert.h>
#include <gbm.h>
#include <GLES2/gl2.h>
Ui ui;
PaintView paint_view;
int paint_capture_mode, paint_preview_mode;
static Node privacy;
Node *node_get(int id) { return id==7?&privacy:NULL; }
static double time_ms(void) {struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1000.0+t.tv_nsec/1e6;}
int main(int argc,char **argv) {
    Kms k={.fd=open(argc>1?argv[1]:"/dev/dri/renderD128",O_RDWR|O_CLOEXEC),.width=512,.height=256};
    if(k.fd<0){perror("render node");return 77;}
    if(gpu_init(&k)){close(k.fd);return 77;}
    enum{W=512,H=256};
    uint32_t input[W*H],output[W*H];
    for(int y=0;y<H;y++)for(int x=0;x<W;x++)input[y*W+x]=x<W/2?0xffe04020:0xff2050e0;
    struct gbm_device *device=gbm_create_device(k.fd);assert(device);
    struct gbm_bo *bo=gbm_bo_create(device,W,H,GBM_FORMAT_XRGB8888,GBM_BO_USE_LINEAR|GBM_BO_USE_RENDERING);assert(bo);
    uint32_t stride;void *map_data;
    uint8_t *map=gbm_bo_map(bo,0,0,W,H,GBM_BO_TRANSFER_WRITE,&stride,&map_data);assert(map);
    for(int y=0;y<H;y++)memcpy(map+y*stride,input+y*W,W*4);
    gbm_bo_unmap(bo,map_data);
    int fd=gbm_bo_get_fd(bo);assert(fd>=0);
    GpuDmabuf buffer={.width=W,.height=H,.nplanes=1,.fourcc=DRM_FORMAT_XRGB8888,.modifier=DRM_FORMAT_MOD_LINEAR,
        .fd={fd},.stride={gbm_bo_get_stride(bo)}};
    GpuTex *texture=gpu_tex_dmabuf(&buffer);assert(texture);
    Dmabuf dm={.d=buffer,.tex=texture};
    Surf surface={.id=100,.node=7,.role=R_TOPLEVEL,.w=W,.h=H,.lw=W,.lh=H,.dmabuf=&dm,.content=1,.opaque=1,.preview_gen=1};
    Surf child={.id=101,.role=R_SUB,.w=W,.h=H,.lw=64,.lh=64,.sx=128,.sy=64,.dmabuf=&dm,.content=1,.opaque=1};
    surface.kids[0]=&child;surface.nkids=1;
    surfs[0]=&surface;
    Canvas c;canvas_init(&c,output,W,H,W);paint_view=(PaintView){0,0,1,0,0,0};ui.width=W;ui.height=H;
    paint_capture_mode=1;canvas_clear(&c,0xff000000);
    paint_tree(&c,&surface,0,0,1,(Rect){0,0,W,H});
    assert(output[0]==input[0] && output[W*H-1]==input[W*H-1]);
    assert(output[80*W+176]==0xff2050e0);
    assert(surface.px==NULL && surface.dmabuf==&dm);
    fs_surf=&surface;canvas_clear(&c,0);wl_paint_fullscreen(&c);
    assert(output[0]==input[0] && output[W*H-1]==input[W*H-1]);
    assert(output[80*W+176]==0xff2050e0);
    privacy.protect=1;wl_paint_fullscreen(&c);assert(!(output[0]&0xffffff) && !(output[W*H-1]&0xffffff));
    privacy.protect=0;fs_surf=NULL;paint_capture_mode=0;
    ndraws[0]=0;paint_tree(&c,&surface,0,0,1,(Rect){0,0,W,H});assert(ndraws[0]==2);
    axia_glass_set_backdrop(glass_backdrop);canvas_clear(&c,0);
    canvas_backdrop_blur(&c,64,64,384,128,12,24);
    uint32_t left=output[100*W+90],right=output[100*W+420];
    assert((left>>24)==255 && ((left>>16)&255)>150 && (right&255)>150);
    assert(output[0]==0); // no holes outside the glass rectangle are filled
    GLint viewport[4];glViewport(3,4,111,99);glDisable(GL_BLEND);glEnable(GL_SCISSOR_TEST);
    uint32_t crop[16];assert(!gpu_tex_read_region(texture,crop,4,4,0,0,16,16));
    glGetIntegerv(GL_VIEWPORT,viewport);assert(viewport[0]==3 && viewport[1]==4 && viewport[2]==111 && viewport[3]==99);
    assert(!glIsEnabled(GL_BLEND) && glIsEnabled(GL_SCISSOR_TEST));
    glViewport(0,0,W,H);glEnable(GL_BLEND);glDisable(GL_SCISSOR_TEST);
    /* A second retained GPU image checks generation invalidation independently
     * of the GBM driver's CPU mapping support after external-image import. */
    for(int i=0;i<W*H;i++)input[i]=0xff20cc40;
    GpuTex *next_texture=gpu_tex_pixels(NULL,input,W,H,1);assert(next_texture);
    gpu_tex_free(texture);texture=next_texture;dm.tex=texture;surface.preview_gen++;
    canvas_clear(&c,0);canvas_backdrop_blur(&c,64,64,384,128,12,24);assert(((output[100*W+90]>>8)&255)>150);
    paint_capture_mode=1;canvas_clear(&c,0);canvas_backdrop_blur(&c,64,64,384,128,12,24);
    assert(!(output[100*W+90]&0xffffff));paint_capture_mode=0;
    double start=time_ms();
    for(int i=0;i<40;i++) {surface.preview_gen++;canvas_clear(&c,0);canvas_backdrop_blur(&c,64,64,384,128,12,24);}
    printf("gpu-capture: full/window pixels, protected fullscreen, cropped glass and retained buffers passed; renderer=%s; changing glass %.3f ms/frame\n",gpu_renderer(),(time_ms()-start)/40);
    axia_glass_set_backdrop(NULL);gpu_tex_free(texture);close(fd);gbm_bo_destroy(bo);gbm_device_destroy(device);gpu_shutdown();close(k.fd);
    return 0;
}
