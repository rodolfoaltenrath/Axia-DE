/* Public portal protocol with captured transport; never touches the user's bus. */
#include "../src/ui/portal_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static unsigned calls,closed,errors,replies,signals,id;
static char mode[32],patterns[512],handle[512];
static DConn conn;
int js_portal_pick(unsigned request,const char *kind,const char *title,const char *folder,const char *name,int multiple,const char *filter) {
    (void)title;(void)folder;(void)name;(void)multiple;
    calls++;id=request;snprintf(mode,sizeof mode,"%s",kind);snprintf(patterns,sizeof patterns,"%s",filter);return 1;
}
void js_portal_close(unsigned request) {assert(request>0);closed++;}
uint32_t __wrap_dbus_reply(DConn *c,const DMsg *m,const char *sig,const DBuf *b) {
    (void)c;(void)m;replies++;
    if (sig) {assert(dbus_body_valid(sig,b->p,b->n,0));DMsg v={.body=b->p,.body_len=b->n};DRd r;dr_init(&r,&v);snprintf(handle,sizeof handle,"%s",dr_str(&r,NULL));}
    return 1;
}
uint32_t __wrap_dbus_error(DConn *c,const DMsg *m,const char *name,const char *message) {(void)c;(void)m;(void)name;(void)message;errors++;return 1;}
uint32_t __wrap_dbus_send(DConn *c,DHdr *h,const DBuf *b) {
    (void)c;signals++;assert(h->type==DBUS_SIGNAL && !strcmp(h->dest,":1.42"));
    assert(!strcmp(h->path,handle));assert(!strcmp(h->sig,"ua{sv}"));assert(dbus_body_valid(h->sig,b->p,b->n,0));
    DMsg v={.body=b->p,.body_len=b->n};DRd r;dr_init(&r,&v);assert(dr_u32(&r)==0);return 1;
}
static DBuf options(const char *token) {
    DBuf b;dw_init(&b);dw_str(&b,"");dw_str(&b,"Choose image");size_t a=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"handle_token");dw_variant(&b,"s");dw_str(&b,token);
    dw_struct(&b);dw_str(&b,"filters");dw_variant(&b,"a(sa(us))");size_t f=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"Images");size_t p=dw_arr_begin(&b,8);dw_struct(&b);dw_u32(&b,0);dw_str(&b,"*.png");dw_arr_end(&b,p);dw_arr_end(&b,f);dw_arr_end(&b,a);return b;
}
int main(void) {
    DBuf b=options("test");
    DMsg m={.type=DBUS_CALL,.path="/org/freedesktop/portal/desktop",.iface="org.freedesktop.portal.FileChooser",.member="OpenFile",.sender=":1.42",.sig="ssa{sv}",.body=b.p,.body_len=b.n};
    assert(dbus_body_valid(m.sig,m.body,m.body_len,0));
    assert(portal_files_handle(&conn,&m));assert(calls==1 && replies==1 && !errors);assert(!strcmp(mode,"open") && !strcmp(patterns,"g:*.png\n"));
    assert(!strcmp(handle,"/org/freedesktop/portal/desktop/request/1_42/test"));
    const char *uris[]={"file:///home/test/foto%20%231.png"};portal_files_result(id,uris,1);assert(signals==1);
    portal_files_result(id,uris,1);assert(signals==1); // completed replies are consumed
    portal_files_handle(&conn,&m);assert(calls==2);
    DMsg close={.type=DBUS_CALL,.path=handle,.iface="org.freedesktop.portal.Request",.member="Close",.sender=":1.99"};
    portal_files_handle(&conn,&close);assert(errors==1 && closed==0);
    close.sender=":1.42";portal_files_handle(&conn,&close);assert(closed==1);
    dw_free(&b);b=options("bad/token");m.body=b.p;m.body_len=b.n;
    portal_files_handle(&conn,&m);assert(errors==2 && calls==2);dw_free(&b);
    b=options("disconnect");m.body=b.p;m.body_len=b.n;portal_files_handle(&conn,&m);
    assert(calls==3);
    portal_files_handle(&conn,&m);assert(errors==3 && calls==3); // duplicate live token
    DBuf gone;dw_init(&gone);dw_str(&gone,":1.42");dw_str(&gone,":1.42");dw_str(&gone,"");
    DMsg signal={.type=DBUS_SIGNAL,.iface="org.freedesktop.DBus",.member="NameOwnerChanged",.sender="org.freedesktop.DBus",.sig="sss",.body=gone.p,.body_len=gone.n};
    portal_files_handle(&conn,&signal);assert(closed==2);
    portal_files_result(id,uris,1);assert(signals==1); // caller disappeared
    dw_free(&b);dw_free(&gone);
    puts("portal-files: valid filters, request handles, one-shot responses, cancellation ownership and invalid tokens passed");
}
