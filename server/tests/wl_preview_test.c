/* Real retained surface pixels, geometry, subsurfaces and privacy without mapping a window. */
#include "../src/wl/wl.c"
#include <assert.h>
Ui ui;
PaintView paint_view;
static Node privacy;
Node *node_get(int id) {return id==7?&privacy:NULL;}
static int gpu_on, reads, fail_read;
int paint_capture_mode, paint_preview_mode;
int gpu_active(void){return gpu_on;}
unsigned gpu_generation(void){return gpu_on?1:0;}
int gpu_tex_current(const GpuTex *t){return t!=NULL;}
GpuTex *gpu_tex_dmabuf(const GpuDmabuf *d){(void)d;return NULL;}
void gpu_tex_free(GpuTex *t){(void)t;}
int gpu_tex_read_scaled(const GpuTex *t,uint32_t *p,int w,int h){(void)t;(void)p;(void)w;(void)h;return -1;}
int gpu_tex_read(const GpuTex *t,uint32_t *p,int w,int h){assert(t);reads++;if(fail_read)return -1;for(int i=0;i<w*h;i++)p[i]=0xff204080;return 0;}
int main(void) {
    uint32_t content[16],child_px[4],out[16];
    for(int i=0;i<16;i++)content[i]=0xff204080u;
    for(int i=0;i<4;i++)child_px[i]=0xffee2040u;
    Surf root={.id=100,.node=7,.role=R_TOPLEVEL,.w=4,.h=4,.lw=4,.lh=4,.px=content,.content=1,.opaque=1,.preview_gen=3};
    Surf child={.id=101,.role=R_SUB,.w=2,.h=2,.lw=2,.lh=2,.sx=2,.sy=2,.px=child_px,.content=1,.opaque=1,.preview_gen=4};
    assert(wl_fullscreen_window()==0);
    fs_surf=&root;
    assert(wl_fullscreen_window()==100);
    root.content=0;assert(wl_fullscreen_window()==0);root.content=1;
    fs_surf=NULL;
    root.kids[0]=&child;root.nkids=1;surfs[0]=&root;
    int w=0,h=0;assert(wl_preview_size(100,&w,&h));assert(w==4&&h==4);
    assert(!root.mapped); // minimized/unmounted frames still retain client pixels
    Canvas c;canvas_init(&c,out,4,4,4);canvas_clear(&c,0);
    paint_view=(PaintView){0,0,1,0,0,0};
    wl_preview_paint(&c,100,0,0,4,4,1);
    assert(out[0]==content[0]);assert(out[15]==child_px[0]);
    assert(!root.mapped && root.conf_w==0 && root.conf_h==0 && root.node==7);
    uint64_t gen=wl_preview_generation(100);child.preview_gen++;assert(gen!=wl_preview_generation(100));
    root.has_geom=1;root.gw=3;root.gh=2;assert(wl_preview_size(100,&w,&h)&&w==3&&h==2);
    privacy.protect=1;assert(!wl_preview_size(100,&w,&h));privacy.protect=0;
    assert(!wl_preview_size(999,&w,&h));
    Dmabuf dm={.tex=(GpuTex*)1};root.dmabuf=&dm;root.px=NULL;gpu_on=1;
    paint_capture_mode=1;canvas_clear(&c,0xff000000);
    paint_tree(&c,&root,0,0,1,(Rect){0,0,4,4});
    assert(out[0]==0xff204080 && out[15]==child_px[0] && reads==1);
    assert(root.px==NULL && root.dmabuf==&dm && !root.mapped && root.conf_w==0);
    fail_read=1;paint_tree(&c,&root,0,0,1,(Rect){0,0,4,4});assert(wl_capture_error(1));assert(!wl_capture_error(0));fail_read=0;
    paint_capture_mode=0;canvas_clear(&c,0xff000000);ndraws[0]=0;
    paint_tree(&c,&root,0,0,1,(Rect){0,0,4,4});assert(reads==2 && ndraws[0]==2);
    root.dmabuf=NULL;gpu_on=0;
    puts("wl-preview: retained pixels, subsurfaces, geometry, privacy and unchanged mapping passed");
    return 0;
}
