/* Native window controller against Rust-owned nodes and geometry. Painting and
 * the benchmark pointer are recorders; no window system or worker is started. */
#include "ui/grab.h"
#include "cursor.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
static int layers,ends,last_resize,virtual_pointer,virtual_fd=-1,have_pointer;
void paint_layer_begin(int node,int resize){assert(node>0);layers++;last_resize=resize;}
void paint_layer_end(void){ends++;}
int lbi_start(float rx,float ry,float period){(void)rx;(void)ry;(void)period;virtual_fd=virtual_pointer?9:-1;return virtual_pointer?0:-1;}
void lbi_stop(void){virtual_fd=-1;}
int lbi_fd(void){return virtual_fd;}
int lbi_take(float *x,float *y){if(!have_pointer)return 0;have_pointer=0;*x=50;*y=-15;return 1;}
void lbi_path(float rx,float ry,float period,double time,float *x,float *y){double angle=2*M_PI*time/period;*x=(float)(rx*sin(angle));*y=(float)(ry*sin(2*angle)/2);}
static Dim px(float value){return (Dim){value,UNIT_PX};}
static Node *window(float x,float y,float w,float h){Node *n=node_new(NODE_VIEW);assert(n);n->s.absolute=1;n->s.pos[EDGE_L]=px(x);n->s.pos[EDGE_T]=px(y);n->s.width=px(w);n->s.height=px(h);n->x=n->ax=x;n->y=n->ay=y;n->w=w;n->h=h;return n;}
static GrabLimits limits(void){return (GrabLimits){.inset=6,.min_w=160,.min_h=100,.min_top=28};}
static void move_to(float x,float y){grab_note_pointer(x,y,1);grab_motion(x,y);}
static GrabEnd finish(float x,float y){GrabEnd end;grab_note_pointer(x,y,0);assert(grab_finish(&end)==1);assert(!grab_active());return end;}
static void test_move_restore_and_resize(void){
    Node *n=window(100,100,320,240);int id=n->id;GrabLimits lim=limits();n->s.pos[EDGE_L]=(Dim){50,UNIT_PCT};n->s.width=(Dim){30,UNIT_PCT};n->s.height=(Dim){0,UNIT_AUTO};
    grab_note_pointer(120,130,0);assert(grab_begin(id,0,&lim)==-1);
    grab_note_pointer(120,130,1);assert(grab_begin(id,0,&lim)==0&&grab_cursor()==CUR_MOVE);assert(layers==1&&last_resize==0);
    float x,y;int edges;assert(grab_state(&x,&y,&edges)&&x==120&&y==130&&edges==0);
    move_to(160,180);assert(n->s.pos[EDGE_L].v==140&&n->s.pos[EDGE_T].v==150);
    GrabEnd end=finish(160,180);assert(end.node==id&&end.dx==40&&end.dy==50&&end.dw==0&&end.dh==0&&end.zone==-1);
    assert(n->s.pos[EDGE_L].unit==UNIT_PX);grab_restore();
    assert(n->s.pos[EDGE_L].unit==UNIT_PCT&&n->s.pos[EDGE_L].v==50&&n->s.width.unit==UNIT_PCT&&n->s.height.unit==UNIT_AUTO);
    n->s.pos[EDGE_L]=px(100);n->s.pos[EDGE_T]=px(100);n->s.width=px(320);n->s.height=px(240);
    lim.max_w=400;lim.max_h=300;grab_note_pointer(420,340,1);assert(grab_begin(id,10,&lim)==0&&grab_cursor()==CUR_SE);assert(last_resize==1);
    move_to(1420,1340);assert(n->s.width.v==412&&n->s.height.v==312);
    end=finish(1420,1340);assert(end.dw==92&&end.dh==72);grab_restore();assert(n->s.width.v==320&&n->s.height.v==240);
    node_destroy(n);
}
static void test_detach_and_linked_resize(void){
    Node *n=window(0,28,1280,772);GrabLimits lim=limits();lim.inset=0;lim.detach_w=600;lim.detach_h=400;grab_note_pointer(640,40,1);
    assert(!grab_begin(n->id,0,&lim));int before=layers;move_to(642,42);assert(n->s.width.v==1280&&layers==before);
    move_to(644,44);assert(n->s.pos[EDGE_L].v==344&&n->s.pos[EDGE_T].v==32&&n->s.width.v==600&&layers==before+1);
    GrabEnd end=finish(644,44);assert(end.detached==1&&end.dx==344&&end.dy==4&&end.dw==-680&&end.dh==-372);grab_restore();node_destroy(n);
    Node *a=window(0,28,640,752),*b=window(640,28,640,752);lim=limits();lim.inset=0;lim.area=(AxiaSnapRect){0,28,1280,752};lim.nlinks=1;lim.links[0].node=b->id;lim.links[0].min_w=160;lim.links[0].min_h=100;
    grab_note_pointer(640,400,1);before=layers;assert(!grab_begin(a->id,8,&lim));assert(layers==before);
    move_to(700,400);assert(a->s.width.v==700&&b->s.pos[EDGE_L].v==700&&b->s.width.v==580);
    end=finish(700,400);assert(end.nlinks==1&&end.links[0].node==b->id&&end.links[0].dx==60&&end.links[0].dw==-60);grab_restore();assert(a->s.width.v==640&&b->s.pos[EDGE_L].v==640&&b->s.width.v==640);
    node_destroy(a);node_destroy(b);
}
static void test_zones_cancellation_and_script(void){
    Node *n=window(100,100,320,240);GrabLimits lim=limits();lim.inset=0;lim.nsnap=1;lim.snap[0]=(AxiaSnapDisplay){{0,0,1280,800},{0,28,1280,752}};
    grab_note_pointer(150,120,1);assert(!grab_begin(n->id,0,&lim));move_to(0,400);int zone;AxiaSnapRect rect;assert(grab_zone_take(&zone,&rect)&&zone==0&&rect.w==640);
    grab_note_shift(1);assert(grab_zone_take(&zone,&rect)&&zone==9&&rect.w==427);
    GrabEnd end=finish(0,400);assert(end.zone==9);assert(grab_zone_take(&zone,&rect)&&zone==-1);assert(!grab_zone_take(&zone,&rect));grab_restore();grab_note_shift(0);
    lim=limits();grab_note_pointer(150,120,1);assert(!grab_begin(n->id,0,&lim));node_destroy(n);grab_motion(160,130);assert(!grab_active());
    n=window(100,100,320,240);grab_note_pointer(150,120,0);assert(grab_script_begin(n->id,&lim,30,20,499)==-1);
    assert(!grab_script_begin(n->id,&lim,30,20,1000)&&grab_scripted());grab_script_tick(1000);grab_script_tick(1250);assert(n->s.pos[EDGE_L].v==130);
    assert(!grab_finish(&end)&&!grab_scripted());assert(n->s.pos[EDGE_L].v==100);
    virtual_pointer=1;assert(!grab_script_begin(n->id,&lim,30,20,1000));have_pointer=1;grab_script_tick(2000);assert(n->s.pos[EDGE_L].v==150&&n->s.pos[EDGE_T].v==85);grab_script_tick(2100);assert(n->s.pos[EDGE_L].v==150);grab_cancel();node_destroy(n);
}
int main(void){ui.width=1280;ui.height=800;test_move_restore_and_resize();test_detach_and_linked_resize();test_zones_cancellation_and_script();puts("window grab Rust ABI: move, resize, detach, links, zones, restore and scripts passed");}
