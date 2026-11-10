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
    /* The compositor clips to the frame geometry, including CSD shadows and children,
     * with logical coordinates retained until the output/animation is known. */
    privacy.ax=20;privacy.ay=30;privacy.w=800;privacy.h=600;
    StyleExt edges={0};edges.r.rad[2]=edges.r.rad[3]=12;
    privacy.s.ext=&edges;privacy.s.overflow_hidden=1;
    root.maximized=1;root.csd=1;
    GpuRoundClip round=maximized_round(&root,7);
    assert(round.x==20 && round.y==30 && round.w==800 && round.h==600 && round.rx==12);
    window_round=round;ndraws[0]=0;gpu_on=1;
    paint_tree(&c,&root,0,0,1,(Rect){0,0,4,4});
    assert(ndraws[0]==2 && draws[0][0].round.rx==12 && draws[0][1].round.rx==12);
    window_round=(GpuRoundClip){0};gpu_on=0;
    tfv.on=1;tfv.sx=0.5f;tfv.sy=0.25f;tfv.tx=100;tfv.ty=200;
    round=maximized_round(&root,7);tfv.on=0;
    assert(round.x==110 && round.y==207.5f && round.w==400 && round.h==150 && round.rx==6 && round.ry==3);
    paint_view=(PaintView){10,20,1.5f,0,0,0};
    round=physical_round(maximized_round(&root,7));
    assert(round.x==15 && round.y==15 && round.w==1200 && round.rx==18);
    root.fullscreen=1;assert(maximized_round(&root,7).rx==0);
    root.fullscreen=0;root.maximized=0;assert(maximized_round(&root,7).rx==0);
    privacy.s.ext=NULL;
    puts("wl-preview: retained pixels, subsurfaces, geometry, privacy and unchanged mapping passed");
    return 0;
}
