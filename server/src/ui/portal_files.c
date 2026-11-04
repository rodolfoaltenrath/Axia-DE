/* FileChooser wire transport only. User interface and path validation live in Rust. */
#include "portal_files.h"
#include "js_wm.h"
#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define FILES "org.freedesktop.portal.FileChooser"
#define REQUEST "org.freedesktop.portal.Request"
#define ROOT "/org/freedesktop/portal/desktop"
static struct Pick { unsigned id; char owner[256],path[512]; } picks[8];
static unsigned sequence;
static DConn *bus;
static int copy(char *out,size_t size,const char *in) {
    if (!in || strlen(in)>=size) return 0;
    memcpy(out,in,strlen(in)+1);return 1;
}
static int bytes(DRd *r,char *out,size_t size) {
    uint32_t n=0;const uint8_t *p=dr_bytes(r,&n);
    if (r->err || !n || n>size || p[n-1] || memchr(p,0,n-1)) return 0;
    memcpy(out,p,n);return 1;
}
static int pattern(char *out,size_t size,const char *prefix,const char *value) {
    if (strchr(value,'\n') || strlen(value)>1024 || strlen(out)+strlen(value)+4>=size) return 0;
    strcat(out,prefix);strcat(out,value);strcat(out,"\n");return 1;
}
static int filter(DRd *r,char *out,size_t size) {
    dr_struct(r);dr_str(r,NULL);size_t end=dr_arr(r,8);unsigned count=0;
    while (dr_more(r,end)) {
        dr_struct(r);unsigned type=dr_u32(r);const char *value=dr_str(r,NULL);
        if (r->err || ++count>128 || type>1 || !pattern(out,size,type?"m:":"g:",value)) return 0;
    }
    return !r->err;
}
static void open_file(DConn *c,const DMsg *m) {
    if (!m->sig || strcmp(m->sig,"ssa{sv}") || !m->sender || m->body_len>65536) {
        dbus_error(c,m,"org.freedesktop.DBus.Error.InvalidArgs","Invalid file chooser request");return;
    }
    unsigned owned=0;struct Pick *p=NULL;
    for (unsigned i=0;i<8;i++) { if (!picks[i].id) p=&picks[i];else if (!strcmp(picks[i].owner,m->sender)) owned++; }
    if (!p || owned>=2) {dbus_error(c,m,"org.freedesktop.portal.Error.Failed","Too many pending dialogs");return;}
    DRd r;dr_init(&r,m);dr_str(&r,NULL);const char *title=dr_str(&r,NULL);
    char token[129]="",folder[4096]="",name[16384]="",patterns[16384]="",current[16384]="";
    int multiple=0,directory=0,valid=title && strlen(title)<=1024,unsupported=0;
    size_t end=dr_arr(&r,8);
    while (valid && dr_more(&r,end)) {
        dr_struct(&r);const char *key=dr_str(&r,NULL),*sig=dr_variant(&r);
        if (r.err) break;
        if (!strcmp(key,"handle_token") && !strcmp(sig,"s")) valid=copy(token,sizeof token,dr_str(&r,NULL));
        else if (!strcmp(key,"multiple") && !strcmp(sig,"b")) multiple=dr_bool(&r);
        else if (!strcmp(key,"directory") && !strcmp(sig,"b")) directory=dr_bool(&r);
        else if (!strcmp(key,"current_name") && !strcmp(sig,"s")) valid=copy(name,sizeof name,dr_str(&r,NULL));
        else if (!strcmp(key,"current_folder") && !strcmp(sig,"ay")) valid=bytes(&r,folder,sizeof folder);
        else if (!strcmp(key,"current_file") && !strcmp(sig,"ay")) {
            char path[4096];valid=bytes(&r,path,sizeof path);
            char *slash=valid?strrchr(path,'/'):NULL;
            if (slash) {valid=copy(name,sizeof name,slash+1);*slash=0;valid=valid && copy(folder,sizeof folder,path[0]?path:"/");}
        } else if (!strcmp(key,"filters") && !strcmp(sig,"a(sa(us))")) {
            size_t last=dr_arr(&r,8);unsigned n=0;
            while (valid && dr_more(&r,last)) {if (++n>64) {valid=0;break;}valid=filter(&r,patterns,sizeof patterns);}
        } else if (!strcmp(key,"current_filter") && !strcmp(sig,"(sa(us))")) valid=filter(&r,current,sizeof current);
        else if (!strcmp(key,"files") && !strcmp(sig,"aay")) {
            size_t last=dr_arr(&r,4);unsigned n=0;name[0]=0;
            while (valid && dr_more(&r,last)) {
                char file[256];valid=++n<=128 && bytes(&r,file,sizeof file);
                if (valid && (strchr(file,'/') || strchr(file,'\n') || strlen(name)+strlen(file)+2>=sizeof name)) valid=0;
                if (valid) {if (name[0]) strcat(name,"\n");strcat(name,file);}
            }
        } else if (!strcmp(key,"choices") && !strcmp(sig,"a(ssa(ss)s)")) {
            size_t last=dr_arr(&r,8);unsupported=dr_more(&r,last);r.pos=last;
        } else dr_skip(&r,sig);
    }
    if (!token[0]) snprintf(token,sizeof token,"axia%u",sequence+1);
    for (const char *q=token;*q;q++) if (!isalnum((unsigned char)*q) && *q!='_') valid=0;
    if (!valid || r.err || unsupported) {
        dbus_error(c,m,unsupported?"org.freedesktop.portal.Error.NotSupported":"org.freedesktop.DBus.Error.InvalidArgs",unsupported?"Application-specific choices are not supported":"Invalid file chooser options");return;
    }
    char sender[256];if (!copy(sender,sizeof sender,m->sender+1)) return;
    for (char *q=sender;*q;q++) if (*q=='.') *q='_';
    p->id=++sequence;if (!p->id) p->id=++sequence;
    copy(p->owner,sizeof p->owner,m->sender);
    snprintf(p->path,sizeof p->path,ROOT "/request/%s/%s",sender,token);
    for (unsigned i=0;i<8;i++) if (&picks[i]!=p && picks[i].id && !strcmp(picks[i].path,p->path)) {
        p->id=0;dbus_error(c,m,"org.freedesktop.DBus.Error.InvalidArgs","Request token is already in use");return;
    }
    const char *mode=!strcmp(m->member,"SaveFiles")?"saveFiles":!strcmp(m->member,"SaveFile")?"save":directory?"folder":"open";
    /* A queued UI cannot answer until this event returns, so return the handle first. */
    DBuf reply;dw_init(&reply);dw_str(&reply,p->path);dbus_reply(c,m,"o",&reply);dw_free(&reply);
    bus=c;
    if (!js_portal_pick(p->id,mode,title,folder,name,multiple,current[0]?current:patterns)) portal_files_result(p->id,NULL,0);
}
void portal_files_result(unsigned id,const char *const *uris,unsigned count) {
    if (!bus) return;
    for (unsigned i=0;i<8;i++) if (picks[i].id==id) {
        DBuf b;dw_init(&b);dw_u32(&b,count?0:1);size_t dict=dw_arr_begin(&b,8);
        if (count) {
            dw_struct(&b);dw_str(&b,"uris");dw_variant(&b,"as");size_t list=dw_arr_begin(&b,4);
            for (unsigned j=0;j<count;j++) dw_str(&b,uris[j]);
            dw_arr_end(&b,list);
        }
        dw_arr_end(&b,dict);
        DHdr h={.type=DBUS_SIGNAL,.path=picks[i].path,.iface=REQUEST,.member="Response",.dest=picks[i].owner,.sig="ua{sv}"};
        dbus_send(bus,&h,&b);dw_free(&b);picks[i].id=0;return;
    }
}
int portal_files_handle(DConn *c,const DMsg *m) {
    if (m->type==DBUS_SIGNAL && m->sender && !strcmp(m->sender,"org.freedesktop.DBus") && m->iface && !strcmp(m->iface,"org.freedesktop.DBus") && m->member && !strcmp(m->member,"NameOwnerChanged") && m->sig && !strcmp(m->sig,"sss")) {
        DRd r;dr_init(&r,m);const char *name=dr_str(&r,NULL);dr_str(&r,NULL);const char *owner=dr_str(&r,NULL);
        if (!r.err && !owner[0]) for (unsigned i=0;i<8;i++) if (picks[i].id && !strcmp(picks[i].owner,name)) {js_portal_close(picks[i].id);picks[i].id=0;}
        return 1;
    }
    if (m->type!=DBUS_CALL || !m->path || !m->member) return 0;
    if (m->iface && !strcmp(m->iface,REQUEST) && !strcmp(m->member,"Close")) {
        for (unsigned i=0;i<8;i++) if (picks[i].id && !strcmp(picks[i].path,m->path) && m->sender && !strcmp(picks[i].owner,m->sender)) {
            js_portal_close(picks[i].id);picks[i].id=0;dbus_reply(c,m,NULL,NULL);return 1;
        }
        dbus_error(c,m,"org.freedesktop.DBus.Error.UnknownObject","Request no longer exists");return 1;
    }
    if (!strcmp(m->path,ROOT) && m->iface && !strcmp(m->iface,FILES)) {
        if (!strcmp(m->member,"OpenFile") || !strcmp(m->member,"SaveFile") || !strcmp(m->member,"SaveFiles")) open_file(c,m);
        else dbus_error(c,m,"org.freedesktop.DBus.Error.UnknownMethod","Unknown file chooser method");
        return 1;
    }
    return 0;
}
