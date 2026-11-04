/* GPU glass updates must paint without laying out every native app again. */
#define main axia_session_main
#include "../src/main.c"
#undef main
#include <assert.h>
Ui ui;
static int layouts,paints;
int js_wants_frame(void){return 0;}
void js_frame(double t){(void)t;}
void layout_root(Node *n,float w,float h){(void)n;(void)w;(void)h;layouts++;}
int paint_damage(Node *n){(void)n;paints++;return !box_empty(ui.damage);}
int paint_layer_node(void){return 0;}
Node *node_get(int id){(void)id;return NULL;}
void paint_layer_view(int view){(void)view;}
int paint_damage_rects(Box *out,int max){if(max>0)out[0]=ui.damage;return max>0;}
void dlist_add(DamageList *list,Box b){if(list->n<DAMAGE_MAX)list->r[list->n++]=b;}
int main(void){
    Server s={.headless=1,.W=800,.H=600};s.out[0].on=1;
    ui.damage=(Box){100,100,200,150};assert(ahead_work(&s.out[0]));
    Box b=paint_ui(&s);assert(layouts==0 && paints==1 && !box_empty(b));
    assert(s.out[0].pend.x0==100 && s.out[0].pend.x1==200 && s.out[0].pl.n==1);
    assert(box_empty(ui.damage));paint_ui(&s);assert(paints==1 && layouts==0);
    ui.dirty=1;paint_ui(&s);assert(layouts==1 && paints==2 && !ui.dirty);
    puts("ui-damage: external glass paints without layout; native UI changes still relayout");
}
