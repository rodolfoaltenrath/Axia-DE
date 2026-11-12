/* Rust gesture router through real driver callbacks and the borrowed UI ABI.
 * Wayland is a real local socketpair connection; JS and painter endpoints are recorders. */
#include "tp/tp.h"
#include "tp/tp_route.h"
#include "ui/ui.h"
#include "wayland_input_fixture.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

static int node_id=10, js_count, axis_count, pinch_count, swipe_count;
static int moves, warps, buttons, flushes, touches, touch_frames;
static int last_node, last_source, last_stop, last_natural, last_fingers, last_phase;
static double a,b,c,d,last_x,last_y;
static char last_kind[32],last_name[32];
static Node root,hit;
Node *hit_test(Node *r,float x,float y){(void)x;(void)y;assert(r==&root);hit.id=node_id;return &hit;}
static void receive(void) {
    static unsigned seen;
    wli_pump();
    for (; seen < wli_nevents; seen++) {
        const WliEvent *e = &wli_events[seen];
        if (e->peer != 0) continue;
        if (e->object == 10) {
            if (e->opcode == 6) { axis_count++; last_source=e->args[0]==1?0:e->args[0]==0?1:2; last_stop=0; }
            else if (e->opcode == 7) last_stop=1;
            else if (e->opcode == 10) last_natural=e->args[1];
            else if (e->opcode == 9) { last_phase=99; if(e->args[0])b=e->args[1];else a=-e->args[1]; }
            else if (e->opcode == 4 && last_source!=1) { if(e->args[1])a=e->args[2]/256.;else b=e->args[2]/256.; }
        } else if (e->object == 13 || e->object == 12) {
            if(e->object==13)pinch_count++;else swipe_count++;
            if(e->opcode==0) { last_phase=TP_BEGIN; last_fingers=e->args[3]; if(e->object==13)d=0; }
            else if(e->opcode==1) { last_phase=TP_UPDATE; a=e->args[1]/256.; b=e->args[2]/256.;
                if(e->object==13){c=e->args[3]/256.;d=e->args[4]/256.;} }
            else last_phase=e->args[2]?TP_CANCEL:TP_END;
        } else if (e->object == 15) {
            if(e->opcode==3){touch_frames++;continue;}
            touches++;
            if(e->opcode==0){last_phase=TP_BEGIN;last_fingers=e->args[3];last_x=e->args[4]/256.;last_y=e->args[5]/256.;}
            else if(e->opcode==1){last_phase=TP_END;last_fingers=e->args[2];}
            else if(e->opcode==2){last_phase=TP_UPDATE;last_fingers=e->args[1];last_x=e->args[2]/256.;last_y=e->args[3]/256.;}
            else last_phase=TP_CANCEL;
        }
    }
}
/* Drain real wire events before checking each public router operation. */
#define tp_route_scroll(...) (tp_route_scroll(__VA_ARGS__), receive())
#define tp_route_pinch(...) (tp_route_pinch(__VA_ARGS__), receive())
#define tp_route_wheel_flush(...) (tp_route_wheel_flush(__VA_ARGS__), receive())
#define tp_tick(...) (tp_tick(__VA_ARGS__), receive())
void js_gesture(const char *kind,const char *phase,int node,float x,float y,double va,double vb,double vc,double vd){
    js_count++;snprintf(last_kind,sizeof last_kind,"%s",kind);snprintf(last_name,sizeof last_name,"%s",phase);
    last_node=node;last_x=x;last_y=y;a=va;b=vb;c=vc;d=vd;
    /* This query would deadlock if routing held the driver lock across JS. */
    TpConfig config=*tp_config();assert(config.speed>=0&&config.speed<=1);
    tp_set_config(&config);
    char devices[4096]; assert(tp_list_json(devices, sizeof devices) > 0); assert(tp_count(0) <= 16);
}
static void position(double *x,double *y,int *w,int *h){*x=12;*y=20;*w=1000;*h=600;}
static void move(double dx,double dy,double rx,double ry){assert(dx||dy||rx||ry);moves++;}
static void warp(double x,double y){last_x=x;last_y=y;warps++;}
static void button(int button,int down){assert(button>=0&&button<=2);buttons++;last_stop=down;}
static void flush(void){flushes++;}
static const TpHost host={.move=move,.warp=warp,.button=button,.position=position,.flush=flush};
static void test_scroll_and_wheel(void){
    wli_focus=-1;node_id=10;tp_route_scroll(TP_BEGIN,1,2,TPR_FINGER);assert(js_count==1&&last_node==10&&!strcmp(last_name,"begin"));
    wli_focus=0;node_id=20;tp_route_scroll(TP_UPDATE,3,4,TPR_FINGER);assert(js_count==2&&last_node==10);
    tp_route_scroll(TP_END,0,0,TPR_FINGER);int before=js_count;
    tp_route_scroll(TP_MOMENTUM,2,3,TPR_FINGER);assert(js_count==before+1&&last_node==10);
    tp_route_scroll(TP_BEGIN,4,5,TPR_CONTINUOUS);assert(axis_count==1&&last_source==2&&last_natural==1);
    wli_focus=-1;tp_route_scroll(TP_UPDATE,6,7,TPR_FINGER);assert(axis_count==1);
    tp_route_scroll(TP_CANCEL,0,0,TPR_FINGER);assert(axis_count==1);
    before=js_count;tp_route_scroll(TP_MOMENTUM,1,2,TPR_FINGER);assert(js_count==before);
    node_id=21;tp_route_wheel120(60,-120);tp_route_wheel120(60,0);tp_route_wheel_flush();
    assert(last_node==21&&a==-48&&b==-48&&c==TPR_WHEEL);
    before=js_count;tp_route_wheel_flush();assert(js_count==before);
    wli_focus=0;tp_route_wheel120(-120,240);tp_route_wheel_flush();assert(last_phase==99&&a==-120&&b==240);
}
static void test_pinch_latching_and_rotation_deltas(void){
    wli_focus=0;tp_route_pinch(TP_BEGIN,2,0,0,1,0);assert(last_phase==TP_BEGIN&&d==0);
    wli_focus=-1;tp_route_pinch(TP_UPDATE,2,1,2,1.5,30);assert(pinch_count==2&&d==30);
    tp_route_pinch(TP_UPDATE,2,2,3,2,45);assert(d==15);
    tp_route_pinch(TP_END,2,0,0,2,55);assert(last_phase==TP_END&&d==15);
    node_id=30;tp_route_pinch(TP_BEGIN,2,4,5,1.2,6);
    assert(!strcmp(last_kind,"pinch")&&last_node==30&&a==1.2&&b==6&&c==4&&d==5);
    tp_route_pinch(TP_END,2,0,0,1,0);
}
static TpCaps caps(int kind){TpCaps c={0};c.kind=kind;c.nslots=5;c.max_fingers=5;
    c.x=(struct input_absinfo){.maximum=10000,.resolution=100};c.y=(struct input_absinfo){.maximum=6000,.resolution=100};
    return c;
}
static void event(TpDevice *device,int type,int code,int value,double time){struct input_event ev={.type=type,.code=code,.value=value};tp_event_at(device,&ev,time);}
static void finger(TpDevice *device,int index,int down,int x,int y,double time){
    event(device,EV_ABS,ABS_MT_SLOT,index,time);event(device,EV_ABS,ABS_MT_TRACKING_ID,down?index+1:-1,time);
    event(device,EV_ABS,ABS_MT_POSITION_X,x,time);event(device,EV_ABS,ABS_MT_POSITION_Y,y,time);
}
static void report(TpDevice *device,double time){event(device,EV_SYN,SYN_REPORT,0,time);receive();}
static void swipe(int fingers,double time){
    TpCaps c=caps(TP_KIND_TOUCHPAD);TpDevice *device=tp_new(&c);assert(device);
    for(int i=0;i<fingers;i++){finger(device,i,1,2500+i*1000,2000,time);}report(device,time);
    for(int i=0;i<fingers;i++){finger(device,i,1,3200+i*1000,2000,time+12);}report(device,time+12);
    for(int i=0;i<fingers;i++){finger(device,i,0,3200+i*1000,2000,time+24);}report(device,time+24);
    tp_free(device);
}
static void test_system_swipes_and_touchscreen(void){
    wli_focus=0;node_id=40;int before=swipe_count;swipe(3,10000);
    assert(swipe_count==before&&!strcmp(last_kind,"swipe")&&last_node==40&&d==1);
    swipe(4,11000);assert(swipe_count>before&&last_fingers==4);
    TpConfig config=*tp_config();config.three_drag=1;tp_set_config(&config);before=swipe_count;
    swipe(4,12000);assert(swipe_count==before&&d==1);
    TpCaps c=caps(TP_KIND_TOUCHSCREEN);TpDevice *device=tp_new(&c);assert(device);
    wli_hit=0;int previous_warps=warps;
    finger(device,0,1,5000,3000,20000);report(device,20000);
    assert(touches>0&&last_phase==TP_BEGIN&&last_x==400&&last_y==250&&touch_frames>0&&warps==previous_warps);
    finger(device,0,0,5000,3000,20012);report(device,20012);assert(last_phase==TP_END);tp_free(device);
    device=tp_new(&c);wli_hit=-1;
    finger(device,0,1,5000,3000,21000);report(device,21000);assert(fabs(last_x-499.5)<1e-9&&fabs(last_y-299.5)<1e-9);
    int before_buttons=buttons;tp_tick(21091);assert(buttons==before_buttons+1&&last_stop==1&&flushes>0);
    finger(device,0,0,5000,3000,21100);report(device,21100);assert(last_stop==0);tp_free(device);
    c=caps(TP_KIND_TOUCHPAD);device=tp_new(&c);
    finger(device,0,1,5000,2000,30000);report(device,30000);
    finger(device,0,1,5200,2000,30012);report(device,30012);assert(moves>0);tp_free(device);
}
int main(void){wli_init();ui.root=&root;ui.width=1000;ui.height=600;tp_route_init(&host);
    test_scroll_and_wheel();test_pinch_latching_and_rotation_deltas();test_system_swipes_and_touchscreen();ui.root=NULL;wli_finish();
    puts("touch router Rust ABI: latches, momentum, wheel120, rotation, swipes and real Wayland touch delivery passed");
}
