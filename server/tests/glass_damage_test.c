#include "../src/ui/paint.c"
#include <assert.h>
Ui ui;
static Box last;
static int calls;
/* dlist_add's rule, without the rest of node.c: overlapping rectangles become their union */
void dlist_add(DamageList *l,Box b) {
    for(int i=0;i<l->n;i++) if(box_meets(l->r[i],b)) {
        b=(Box){fminf(b.x0,l->r[i].x0),fminf(b.y0,l->r[i].y0),fmaxf(b.x1,l->r[i].x1),fmaxf(b.y1,l->r[i].y1)};
        l->r[i--]=l->r[--l->n];
    }
    l->r[l->n++]=b;
}
void damage_add(Box b) {last=b;calls++;dlist_add(&ui.dlist,b);}
/* glass_pacing.rs's contract (its rules are tested in Rust; the Rust archive also defines ui and
 * dlist_add): the first app frame refreshes now, the next ones within 1/30 s wait for the tick */
static int held;
static double paced=-1e9;
int axia_glass_pacing_request(double now) { if(now<paced||now-paced>=1000.0/30) {paced=now;held=0;return 1;} held=1;return 0; }
int axia_glass_pacing_tick(double now) { if(held&&(now<paced||now-paced>=1000.0/30)) {held=0;paced=now;return 1;} return 0; }
double axia_glass_pacing_timeout(double now) { return held?fmax(0,paced+1000.0/30-now):-1; }
int main(void) {
    Node root={.w=800,.h=600},glass={.w=200,.h=100,.prev_box={100,100,300,200}},hidden={.w=800,.h=600};
    root.first=&glass;glass.next=&hidden;glass.s.backdrop_blur=12;hidden.s.display_none=1;hidden.s.backdrop_blur=32;
    ui.root=&root;ui.width=800;ui.height=600;
    assert(!paint_external_glass_damage((Box){600,400,800,600}));assert(calls==0);
    assert(paint_external_glass_damage((Box){90,90,110,110}));assert(calls==1);
    assert(last.x0==75 && last.y0==75 && last.x1==325 && last.y1==225);
    glass.s.display_none=1;assert(!paint_external_glass_damage((Box){0,0,800,600}));
    /* a frame that only refreshes glass: bar and Dock stay apart; anything else is one box */
    Node bar={.w=800,.h=30,.prev_box={0,0,800,30}},dock={.w=400,.h=60,.prev_box={200,530,600,590}},top={.w=800,.h=600};
    bar.s.backdrop_blur=24;dock.s.backdrop_blur=24;bar.next=&dock;top.first=&bar;ui.root=&top;
    nglass_refresh=0;ui.dlist.n=0;assert(!glass_only_frame());
    /* app frames come faster than the pacing (glass_pacing.rs): this one waits, then goes in the tick */
    assert(paint_external_glass_damage((Box){0,0,800,600}));assert(nglass_refresh==0);assert(nglass_wait==2);
    assert(paint_glass_timeout(clk())>=0);
    paint_glass_tick(clk()+1000);assert(!nglass_wait);assert(paint_glass_timeout(clk())<0);
    assert(nglass_refresh==2);assert(glass_only_frame());
    assert(ui.dlist.n==2);
    dlist_add(&ui.dlist,(Box){300,300,310,310});assert(!glass_only_frame()); /* a window's own change */
    nglass_refresh=0;ui.dlist.n=0;
    assert(paint_external_glass_damage((Box){0,0,800,10}));assert(nglass_refresh==1);assert(glass_only_frame());
    /* the same glass again within the pacing's interval: kept once, however many frames */
    assert(paint_external_glass_damage((Box){0,0,800,10}));assert(paint_external_glass_damage((Box){0,0,800,10}));
    assert(nglass_refresh==1);assert(nglass_wait==1);
    paint_glass_tick(clk()+1000);assert(nglass_refresh==2);assert(glass_only_frame()); /* twice: one rectangle */
    nglass_refresh=0;ui.dlist.n=0;glass_refresh_over=1;assert(!glass_only_frame());glass_refresh_over=0;
    assert(!paint_external_glass_damage((Box){300,300,310,310}));assert(!nglass_refresh); /* the middle of the screen: no glass */
    puts("glass-damage: only intersecting visible glass repaints, including its sample margin, at most 30 times a second for app frames");
}
