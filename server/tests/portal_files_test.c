/* Public portal protocol with captured transport; never touches the user's bus. */
#include "../src/ui/portal_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static unsigned calls,closed,errors,replies,signals,id,last_status,last_closed;
static char mode[32],patterns[16384],handle[512],signal_path[512],signal_to[256];
static uint8_t signal_body[8192];static size_t signal_len;
static DConn conn;
int js_portal_pick(unsigned request,const char *kind,const char *title,const char *folder,const char *name,int multiple,const char *filter) {
    (void)title;(void)folder;(void)name;(void)multiple;
    calls++;id=request;snprintf(mode,sizeof mode,"%s",kind);snprintf(patterns,sizeof patterns,"%s",filter);return 1;
}
void js_portal_close(unsigned request) {assert(request>0);closed++;last_closed=request;}
uint32_t __wrap_dbus_reply(DConn *c,const DMsg *m,const char *sig,const DBuf *b) {
    (void)c;(void)m;replies++;
    if (sig) {assert(dbus_body_valid(sig,b->p,b->n,0));DMsg v={.body=b->p,.body_len=b->n};DRd r;dr_init(&r,&v);snprintf(handle,sizeof handle,"%s",dr_str(&r,NULL));}
    return 1;
}
uint32_t __wrap_dbus_error(DConn *c,const DMsg *m,const char *name,const char *message) {(void)c;(void)m;(void)name;(void)message;errors++;return 1;}
uint32_t __wrap_dbus_send(DConn *c,DHdr *h,const DBuf *b) {
    (void)c;signals++;assert(h->type==DBUS_SIGNAL && !strcmp(h->sig,"ua{sv}") && dbus_body_valid(h->sig,b->p,b->n,0));
    snprintf(signal_path,sizeof signal_path,"%s",h->path);snprintf(signal_to,sizeof signal_to,"%s",h->dest);
    assert(b->n<=sizeof signal_body);memcpy(signal_body,b->p,b->n);signal_len=b->n;
    DMsg v={.body=b->p,.body_len=b->n};DRd r;dr_init(&r,&v);last_status=dr_u32(&r);return 1;
}
static int signal_has(const char *text) {
    size_t n=strlen(text);
    for (size_t i=0;i+n<=signal_len;i++) if (!memcmp(signal_body+i,text,n)) return 1;
    return 0;
}
static DBuf options(const char *token) {
    DBuf b;dw_init(&b);dw_str(&b,"");dw_str(&b,"Choose image");size_t a=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"handle_token");dw_variant(&b,"s");dw_str(&b,token);
    dw_struct(&b);dw_str(&b,"filters");dw_variant(&b,"a(sa(us))");size_t f=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"Images");size_t p=dw_arr_begin(&b,8);dw_struct(&b);dw_u32(&b,0);dw_str(&b,"*.png");dw_arr_end(&b,p);dw_arr_end(&b,f);dw_arr_end(&b,a);return b;
}
static DBuf large(const char *token) {
    DBuf b;dw_init(&b);dw_str(&b,"");dw_str(&b,"Open");size_t a=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"handle_token");dw_variant(&b,"s");dw_str(&b,token);
    dw_struct(&b);dw_str(&b,"filters");dw_variant(&b,"a(sa(us))");size_t f=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"All Formats");size_t p=dw_arr_begin(&b,8);
    for (int i=0;i<213;i++) {char glob[32];snprintf(glob,sizeof glob,"*.ext%d",i);dw_struct(&b);dw_u32(&b,0);dw_str(&b,glob);}
    dw_arr_end(&b,p);dw_arr_end(&b,f);
    dw_struct(&b);dw_str(&b,"choices");dw_variant(&b,"a(ssa(ss)s)");size_t ch=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"encoding");dw_str(&b,"Encoding");size_t o=dw_arr_begin(&b,8);
    dw_struct(&b);dw_str(&b,"utf8");dw_str(&b,"UTF-8");dw_arr_end(&b,o);dw_str(&b,"utf8");
    dw_arr_end(&b,ch);dw_arr_end(&b,a);return b;
}
static void send_request(const char *sender,DBuf *b) {
    DMsg m={.type=DBUS_CALL,.path="/org/freedesktop/portal/desktop",.iface="org.freedesktop.portal.FileChooser",.member="OpenFile",.sender=sender,.sig="ssa{sv}",.body=b->p,.body_len=b->n};
    assert(dbus_body_valid(m.sig,m.body,m.body_len,0));
    assert(portal_files_handle(&conn,&m));
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
    /* a token that cannot be a path element is replaced, not refused */
    dw_free(&b);b=options("bad/token");m.body=b.p;m.body_len=b.n;
    portal_files_handle(&conn,&m);assert(errors==1 && calls==3 && strstr(handle,"/request/1_42/axia"));dw_free(&b);
    b=options("disconnect");m.body=b.p;m.body_len=b.n;portal_files_handle(&conn,&m);
    assert(calls==4);
    /* the same token again: the old dialog closes without an answer, which the new one would get */
    portal_files_handle(&conn,&m);assert(errors==1 && calls==5 && closed==2 && signals==1);
    DBuf gone;dw_init(&gone);dw_str(&gone,":1.42");dw_str(&gone,":1.42");dw_str(&gone,"");
    DMsg signal={.type=DBUS_SIGNAL,.iface="org.freedesktop.DBus",.member="NameOwnerChanged",.sender="org.freedesktop.DBus",.sig="sss",.body=gone.p,.body_len=gone.n};
    portal_files_handle(&conn,&signal);assert(closed==4);
    portal_files_result(id,uris,1);assert(signals==1); // caller disappeared
    dw_free(&b);dw_free(&gone);

    /* a very large filter and application choices open the dialog; the choices are answered */
    b=large("photocraft");send_request(":1.7",&b);dw_free(&b);
    assert(errors==1 && calls==6 && strstr(patterns,"g:*.ext0\n") && strstr(patterns,"g:*.ext212\n"));
    unsigned first=id;
    portal_files_result(first,uris,1);assert(signals==2 && last_status==0 && signal_has("choices") && signal_has("encoding") && signal_has("utf8"));

    /* a third dialog of one caller replaces its oldest, which hears that it was cancelled */
    DBuf one=options("one"),two=options("two"),three=options("three");
    send_request(":1.8",&one);unsigned oldest_id=id;send_request(":1.8",&two);send_request(":1.8",&three);
    assert(calls==9 && errors==1 && signals==3 && last_status==1 && last_closed==oldest_id);
    assert(strstr(signal_path,"/1_8/one") && !strcmp(signal_to,":1.8"));
    /* with every slot taken, the oldest dialog of all makes room */
    for (int i=0;i<8;i++) {char who[16];snprintf(who,sizeof who,":2.%d",i);send_request(who,&one);}
    assert(calls==17 && errors==1 && signals==5 && last_status==1);
    dw_free(&one);dw_free(&two);dw_free(&three);
    puts("portal-files: filters, request handles, one-shot responses, cancellation ownership, large requests and full queues passed");
}
