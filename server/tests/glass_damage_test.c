#include "../src/ui/paint.c"
#include <assert.h>
Ui ui;
static Box last;
static int calls;
void damage_add(Box b) {last=b;calls++;}
int main(void) {
    Node root={.w=800,.h=600},glass={.w=200,.h=100,.prev_box={100,100,300,200}},hidden={.w=800,.h=600};
    root.first=&glass;glass.next=&hidden;glass.s.backdrop_blur=12;hidden.s.display_none=1;hidden.s.backdrop_blur=32;
    ui.root=&root;ui.width=800;ui.height=600;
    assert(!paint_external_glass_damage((Box){600,400,800,600}));assert(calls==0);
    assert(paint_external_glass_damage((Box){90,90,110,110}));assert(calls==1);
    assert(last.x0==75 && last.y0==75 && last.x1==325 && last.y1==225);
    glass.s.display_none=1;assert(!paint_external_glass_damage((Box){0,0,800,600}));
    puts("glass-damage: only intersecting visible glass repaints, including its sample margin");
}
