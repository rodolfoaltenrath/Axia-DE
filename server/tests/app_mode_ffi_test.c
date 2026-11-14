/* Real Rust app ABI and QuickJS bridge, driven by a deterministic compositor.
 * No desktop, sandbox changes or real child applications are started. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include "app/app.h"
#include "lpk.h"
#include "keyboard.h"
#include <dirent.h>
#include "app/apphost_bc.h"
#include "ui/js_app_host.h"
#include "ui/js_broker.h"
#include "ui/ui.h"
#include "sec/sandbox.h"
#include "sec/broker.h"
#include <ftw.h>
#include "pkg/crypto.h"
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

/* The generated client protocol ABI: test descriptors never dispatch through
 * libwayland. Requests and listener calls still cross the real Rust C ABI. */
_Static_assert(sizeof(BrokerHdr)==12 && BOP_FS_READ==2 && BOP_CLIP_GET==5 && BOP_VAULT_FIDO==40 && BOP_MAIL==43 && BOP_PROBE==45,"Rust probe broker ABI");

typedef struct Interface { const char *name; int version, method_count; const void *methods; int event_count; const void *events; } Interface;
#define IFACE(n) __attribute__((weak)) const Interface n##_interface = {#n, 6, 0, NULL, 0, NULL}
IFACE(wl_registry); IFACE(wl_compositor); IFACE(wl_surface); IFACE(wl_shm);
IFACE(wl_shm_pool); IFACE(wl_buffer); IFACE(wl_callback); IFACE(wl_seat);
IFACE(wl_pointer); IFACE(wl_keyboard); IFACE(xdg_wm_base); IFACE(xdg_surface);
IFACE(xdg_toplevel); IFACE(zxdg_decoration_manager_v1); IFACE(zxdg_toplevel_decoration_v1);
IFACE(zwp_pointer_gestures_v1); IFACE(zwp_pointer_gesture_pinch_v1); IFACE(zwp_pointer_gesture_swipe_v1);
typedef struct Proxy { const Interface *iface; void (**cb)(void); void *data; int live, configured, version; } Proxy;
static Proxy proxies[4096]; static int nproxy;
static int display_pipe[2], announced, sandboxed, paints, commits, releases, ticks, events, minw, minh, firstw, firsth, shutdowns;
static int mode, network, recovered, gesture_updates, pointer_downs, key_events, keymaps, input_sent;
static int fake_launch, existing_window, register_fail, channel_fd=-1, pid_read=-1, spawn_pid, vault_calls, vault_marks, system_marks, window_requests;
pid_t __real_fork(void);
pid_t __wrap_fork(void) { if(!fake_launch)return __real_fork();if(fake_launch==2){errno=EAGAIN;return -1;}return 1234; }
int __real_pipe2(int fd[2],int flags);
int __wrap_pipe2(int fd[2],int flags) {int r=__real_pipe2(fd,flags);if(!r&&fake_launch)pid_read=fd[0];return r;}
pid_t __real_waitpid(pid_t pid,int *status,int options);
pid_t __wrap_waitpid(pid_t pid,int *status,int options) {if(fake_launch){assert(pid==1234);return pid;}return __real_waitpid(pid,status,options);}
ssize_t __real_read(int fd,void *bytes,size_t size);
ssize_t __wrap_read(int fd,void *bytes,size_t size) {if(fake_launch&&fd==pid_read){pid_t pid=4321;assert(size==sizeof pid);memcpy(bytes,&pid,sizeof pid);return sizeof pid;}return __real_read(fd,bytes,size);}

static char assets[4096], fonts[4096], bundle[4096], home[4096];
static JSRuntime *runtime; static JSContext *context;
Ui ui; static Node root_node, second_node;
static Proxy *proxy(const Interface *iface) { assert(nproxy < 4096); Proxy *p=&proxies[nproxy++]; *p=(Proxy){.iface=iface,.live=1,.version=1};return p; }
static int is(Proxy *p,const char *name) { return p && p->iface && !strcmp(p->iface->name,name); }
void *wl_display_connect(const char *name) { (void)name;assert(!sandboxed);assert(!pipe2(display_pipe,O_NONBLOCK|O_CLOEXEC));return (void *)0x1000; }
void wl_display_disconnect(void *display) { assert(display);close(display_pipe[0]);close(display_pipe[1]); }
int wl_display_get_fd(void *display) { (void)display;return display_pipe[0]; }
uint32_t wl_proxy_get_version(void *p) { return p==(void *)0x1000?1u:(uint32_t)((Proxy *)p)->version; }
void *wl_proxy_marshal_flags(void *parent,uint32_t op,const Interface *iface,uint32_t version,uint32_t flags,...) {
    Proxy *p=parent;va_list args;va_start(args,flags);
    if (flags) { assert(p && p->live);p->live=0;va_end(args);return NULL; }
    if (iface) {
        Proxy *out=proxy(iface);out->version=(int)version;
        if (is(out,"wl_shm_pool")) { (void)va_arg(args,void *);int fd=va_arg(args,int),size=va_arg(args,int);assert(fd>=0 && size>0); }
        if (is(out,"wl_buffer")) { (void)va_arg(args,void *);assert(va_arg(args,int)==0);int w=va_arg(args,int),h=va_arg(args,int),stride=va_arg(args,int);assert(w>0&&h>0&&stride==w*4);assert(va_arg(args,unsigned)==0); }
        va_end(args);return out;
    }
    assert(p && p->live);
    if (is(p,"xdg_toplevel") && op==8) { minw=va_arg(args,int);minh=va_arg(args,int); }
    if (is(p,"wl_surface") && op==6) commits++;
    va_end(args);return NULL;
}
int wl_proxy_add_listener(void *proxy_value,void (**callbacks)(void),void *data) { Proxy *p=proxy_value;assert(p&&p->live);p->cb=callbacks;p->data=data;return 0; }
void wl_proxy_destroy(void *proxy_value) { Proxy *p=proxy_value;assert(p&&p->live);p->live=0; }
static void configure(void) {
    for (int i=0;i<nproxy;i++) { Proxy *p=&proxies[i];if (!p->live||p->configured||!p->cb)continue;
        if (is(p,"xdg_surface")) { p->configured=1;((void(*)(void *,void *,uint32_t))p->cb[0])(p->data,p,5); }
        if (is(p,"zxdg_toplevel_decoration_v1")) { p->configured=1;((void(*)(void *,void *,uint32_t))p->cb[0])(p->data,p,2); }
    }
}
int wl_display_roundtrip(void *display) {
    (void)display;
    if (!announced) {
        announced=1;Proxy *reg=NULL;for(int i=0;i<nproxy;i++)if(is(&proxies[i],"wl_registry"))reg=&proxies[i];assert(reg&&reg->cb);
        const char *names[]={"wl_compositor","wl_shm","xdg_wm_base","wl_seat","zxdg_decoration_manager_v1","zwp_pointer_gestures_v1"};
        for (unsigned i=0;i<6;i++)((void(*)(void *,void *,uint32_t,const char *,uint32_t))reg->cb[0])(reg->data,reg,i+1,names[i],7);
        for (int i=0;i<nproxy;i++) {Proxy *p=&proxies[i];if(is(p,"wl_seat")&&p->cb)((void(*)(void *,void *,uint32_t))p->cb[0])(p->data,p,3);}
    } else configure();
    return 0;
}
int wl_display_flush(void *display) { (void)display;assert(write(display_pipe[1],"x",1)==1);return 0; }
int wl_display_prepare_read(void *display) { (void)display;return 0; }
void wl_display_cancel_read(void *display) { (void)display; }
int wl_display_read_events(void *display) { (void)display;char bytes[256];while(read(display_pipe[0],bytes,sizeof bytes)>0){}return 0; }
int wl_display_dispatch_pending(void *display) {
    (void)display;configure();
    for (int i=0;i<nproxy;i++) {Proxy *p=&proxies[i];if(!p->live||!p->cb)continue;
        if(is(p,"wl_callback"))((void(*)(void *,void *,uint32_t))p->cb[0])(p->data,p,10);
        if(is(p,"wl_buffer")){((void(*)(void *,void *))p->cb[0])(p->data,p);releases++;}
    }
    if(!input_sent && ui.root) {
        input_sent=1;Proxy *surface=NULL;
        for(int i=0;i<nproxy;i++)if(proxies[i].live&&is(&proxies[i],"wl_surface")){surface=&proxies[i];break;}
        assert(surface);
        for(int i=0;i<nproxy;i++) {Proxy *p=&proxies[i];if(!p->live||!p->cb)continue;
            if(is(p,"wl_pointer")) {
                ((void(*)(void *,void *,uint32_t,void *,int32_t,int32_t))p->cb[0])(p->data,p,1,surface,2560,5120);
                ((void(*)(void *,void *,uint32_t,int32_t,int32_t))p->cb[2])(p->data,p,1,2560,5120);
                ((void(*)(void *,void *,uint32_t,uint32_t,uint32_t,uint32_t))p->cb[3])(p->data,p,1,1,0x110,1);
                ((void(*)(void *,void *,uint32_t))p->cb[6])(p->data,p,1);
                ((void(*)(void *,void *,uint32_t,uint32_t,int32_t))p->cb[4])(p->data,p,1,0,128);
                ((void(*)(void *,void *,uint32_t,uint32_t))p->cb[7])(p->data,p,1,0);
                ((void(*)(void *,void *))p->cb[5])(p->data,p);
            }
            if(is(p,"wl_keyboard")) {
                char *map=kb_keymap_string();assert(map);size_t map_size=strlen(map)+1;int fd=memfd_create("keymap",MFD_CLOEXEC);assert(fd>=0&&write(fd,map,map_size)==(ssize_t)map_size);free(map);
                ((void(*)(void *,void *,uint32_t,int32_t,uint32_t))p->cb[0])(p->data,p,1,fd,(uint32_t)map_size);assert(fcntl(fd,F_GETFD)==-1&&kb_ready());keymaps++;
                ((void(*)(void *,void *,uint32_t,void *,void *))p->cb[1])(p->data,p,1,surface,NULL);
                ((void(*)(void *,void *,uint32_t,uint32_t,uint32_t,uint32_t))p->cb[3])(p->data,p,1,1,30,1);
            }
            if(is(p,"zwp_pointer_gesture_pinch_v1")) {
                ((void(*)(void *,void *,uint32_t,uint32_t,void *,uint32_t))p->cb[0])(p->data,p,1,1,surface,2);
                ((void(*)(void *,void *,uint32_t,int32_t,int32_t,int32_t,int32_t))p->cb[1])(p->data,p,1,256,512,512,128);
                ((void(*)(void *,void *,uint32_t,uint32_t,int32_t))p->cb[2])(p->data,p,1,1,0);
            }
            if(is(p,"zwp_pointer_gesture_swipe_v1")) {
                ((void(*)(void *,void *,uint32_t,uint32_t,void *,uint32_t))p->cb[0])(p->data,p,1,1,surface,3);
                ((void(*)(void *,void *,uint32_t,int32_t,int32_t))p->cb[1])(p->data,p,1,256,512);
                ((void(*)(void *,void *,uint32_t,uint32_t,int32_t))p->cb[2])(p->data,p,1,1,1);
            }
        }
    }
    return 0;
}
Node *node_get(int id) { return id==1?&root_node:id==2?&second_node:NULL; }
int layout_root(Node *root,float w,float h) {assert(root);if(!firstw){firstw=(int)w;firsth=(int)h;}return 1;}
void canvas_init(Canvas *c,uint32_t *pixels,int w,int h,int stride) { assert(pixels);*c=(Canvas){.px=pixels,.w=w,.h=h,.stride=stride}; }
void paint_root_clear(Canvas *c,Node *root,uint32_t color) {assert(root&&color==0&&ui.damage.x1==0&&ui.damage.y1==0);c->px[0]=0xff123456;paints++;}
Node *hit_test(Node *root,float x,float y) {assert(x>=0&&y>=0);return root;}
int text_init(const char *path) {assert(!strcmp(path,fonts)&&!sandboxed);return 0;}
void js_sys_set_assets(const char *path) {assert(strstr(path,"fonts/.."));}
const char *js_sys_assets(void) {return assets;}
int codesig_init(const char *path) {assert(!strcmp(path,assets));return 1;}
int codesig_state(void) {return 1;}
static const char host_source[]="globalThis.answer = 42;";
char *codesig_load(const char *path,size_t max,size_t *len,char *why,size_t whylen) {(void)path;(void)why;(void)whylen;assert(max>sizeof host_source);*len=strlen(host_source);return strdup(host_source);}
int sandbox_apply(const SandboxConfig *cfg) {assert(!sandboxed&&cfg->ro_extra&&cfg->ro_extra[0]&&cfg->ro_extra[3]);assert(!strcmp(cfg->id,"notes"));assert(!strcmp(cfg->ro_extra[0],fonts));assert(strstr(cfg->data_dir,"appdata/notes"));assert(cfg->network==1);sandboxed=1;return 63;}
int sandbox_apply_helper(const SandboxHelperCfg *cfg) {(void)cfg;abort();}
void js_broker_set_pkgdir(const char *dir,int system) {assert(!sandboxed&&system==1&&strstr(dir,"sysapps/notes"));network=1;}
int js_broker_wants_network(void) {assert(network);return 1;}
int js_broker_code_refused(void) {return mode==2&&recovered;}
int js_broker_fd(void) {return -1;}
int js_broker_pending(void) {return 0;}
void js_broker_poll(void) {abort();}
void js_broker_app_event(const char *kind,const char *json,size_t len) {assert(!strcmp(kind,"appwindow")&&len&&memmem(json,len,"key",3));events++;}
static void script(const char *source) {JSValue v=JS_Eval(context,source,strlen(source),"fixture",JS_EVAL_TYPE_GLOBAL);if(JS_IsException(v)){JSValue ex=JS_GetException(context);const char *s=JS_ToCString(context,ex);fprintf(stderr,"fixture: %s\n",s);abort();}JS_FreeValue(context,v);}
int js_init_app(const char *host,const char *id) {
    assert(sandboxed&&!strcmp(host,bundle)&&!strcmp(id,"notes"));
    ui.root=&root_node;ui.dirty=1;root_node.id=1;second_node.id=2;
    if(mode==1)return -2;
    runtime=JS_NewRuntime();context=JS_NewContext(runtime);assert(context);
    JSValue global=JS_GetGlobalObject(context),lord=JS_NewObject(context);app_windows_register(context,lord);JS_SetPropertyStr(context,global,"lord",lord);JS_FreeValue(context,global);
    script("let k=lord.appWindows.open('Second',700,500);if(k!==1)throw Error('key');if(!lord.appWindows.attach(0,1)||!lord.appWindows.attach(k,2))throw Error('attach');if(lord.appWindows.attach(k,999))throw Error('invalid node');lord.appWindows.setTitle(k,'Renamed');let keys=[];for(let i=0;i<14;i++)keys.push(lord.appWindows.open('extra',0,99999));if(keys.some(k=>k<0)||lord.appWindows.open('full',640,480)!==-1)throw Error('limit');for(const k of keys)lord.appWindows.close(k);");
    return 0;
}
int js_app_recovery(void) {recovered=1;axia_app_window_close(0);assert(axia_app_window_attach(0,1));ui.root=&root_node;ui.dirty=1;return 0;}
void js_shutdown(void) {shutdowns++;if(context){JS_FreeContext(context);JS_FreeRuntime(runtime);context=NULL;runtime=NULL;}}
double js_tick(double now) {(void)now;ticks++;if(mode==2&&ticks==2)recovered=1;if(ticks>=4)ui.quit_requested=1;return -1;}
int js_frame_ready(double now,int waiting,unsigned epoch) {(void)now;(void)waiting;(void)epoch;return 0;}
void js_frame(double now) {(void)now;abort();}
double js_frame_wait(double now,int waiting,unsigned epoch) {(void)now;(void)waiting;(void)epoch;return -1;}
int js_timers_changed(void) {return 0;}
void js_pointer(const char *kind,int target,float x,float y,int button) {(void)target;(void)x;(void)y;(void)button;if(!strcmp(kind,"down"))pointer_downs++;}
void js_gesture(const char *kind,const char *phase,int target,float x,float y,double a,double b,double c,double d) {(void)kind;(void)target;(void)x;(void)y;(void)a;(void)b;(void)c;(void)d;if(!strcmp(phase,"update"))gesture_updates++;}
void js_key(int code,int value) {(void)code;(void)value;key_events++;}
int brokersrv_new_window(const char *id) {assert(id&&fake_launch);window_requests++;return existing_window?0:-1;}
int brokersrv_add(int fd,const char *id,int pid) {assert(id&&fake_launch);channel_fd=fd;spawn_pid=pid;return register_fail?-1:0;}
void brokersrv_set_system(int fd) {assert(fd==channel_fd);system_marks++;}
void brokersrv_set_vault(int fd) {assert(fd==channel_fd);vault_marks++;}
int vc_attach_app(void) {vault_calls++;return open("/dev/null",O_RDONLY|O_CLOEXEC);}
static int fd_count(void) {DIR *d=opendir("/proc/self/fd");assert(d);int n=0;while(readdir(d))n++;closedir(d);return n;}
static void launch_test(void) {
    fake_launch=1;assert(app_spawn("../bad")==-1&&window_requests==0);
    existing_window=1;assert(app_spawn("notes")==0&&window_requests==1);existing_window=0;
    int before=fd_count();fake_launch=2;assert(app_spawn("notes")==-1);assert(fd_count()==before);fake_launch=1;
    assert(app_spawn("notes")==0&&spawn_pid==4321&&system_marks==1&&vault_calls==0&&vault_marks==0);assert(fcntl(channel_fd,F_GETFD)>=0);close(channel_fd);assert(fd_count()==before);
    register_fail=1;assert(app_spawn("notes")==-1);assert(fcntl(channel_fd,F_GETFD)==-1);assert(fd_count()==before);register_fail=0;
    char p[8192];assert(snprintf(p,sizeof p,"%s/passwords",lpk_root())<(int)sizeof p);assert(!mkdir(p,0700));
    assert(app_spawn("passwords")==0&&vault_calls==0&&system_marks==1);close(channel_fd);
    assert(snprintf(p,sizeof p,"%s/sysapps/passwords",assets)<(int)sizeof p);assert(!mkdir(p,0700));assert(snprintf(p,sizeof p,"%s/sysapps/passwords/manifest.json",assets)<(int)sizeof p);FILE *f=fopen(p,"w");assert(f);fputs("{}",f);fclose(f);
    assert(app_spawn("passwords")==0&&vault_calls==1&&vault_marks==1&&system_marks==2);close(channel_fd);assert(fd_count()==before);
    fake_launch=0;
}

static int cache(const char *src,const uint8_t *bc,size_t len,int sealed) {
    struct {char magic[8];uint64_t size;uint8_t hash[32];uint64_t len;} h;
    _Static_assert(sizeof h==56,"cache header ABI");memcpy(h.magic,"LORDQBC1",8);h.size=strlen(src);h.len=len;sha256(src,strlen(src),h.hash);
    int fd=memfd_create("fixture",MFD_CLOEXEC|MFD_ALLOW_SEALING);assert(fd>=0&&write(fd,&h,sizeof h)==sizeof h&&write(fd,bc,len)==(ssize_t)len);
    if(sealed) { assert(!fcntl(fd,F_ADD_SEALS,F_SEAL_SHRINK|F_SEAL_GROW|F_SEAL_WRITE|F_SEAL_SEAL)); }
    return fd;
}
static void bytecode_test(void) {
    JSRuntime *rt=JS_NewRuntime();JSContext *ctx=JS_NewContext(rt);assert(ctx);
    const char *src="globalThis.answer = 42;";size_t len=0;uint8_t *bc=axia_app_qjs_compile(src,strlen(src),"fixture",&len);assert(bc&&len);
    int fd=cache(src,bc,len,1),ok=0;assert(apphost_bc_eval(ctx,src,strlen(src),fd,&ok)==0&&ok==1);
    JSValue value=JS_Eval(ctx,"answer",6,"fixture",0);int32_t answer=0;assert(!JS_ToInt32(ctx,&answer,value)&&answer==42);JS_FreeValue(ctx,value);
    assert(apphost_bc_eval(ctx,"globalThis.answer = 43;",strlen(src),fd,&ok)==-1&&ok==0);close(fd);
    fd=cache(src,bc,len,0);assert(apphost_bc_eval(ctx,src,strlen(src),fd,&ok)==-1);
    FILE *ordinary=tmpfile();assert(ordinary);off_t length=lseek(fd,0,SEEK_END);assert(length>0);uint8_t *copy=malloc((size_t)length);assert(copy&&pread(fd,copy,(size_t)length,0)==length);assert(fwrite(copy,1,(size_t)length,ordinary)==(size_t)length);fflush(ordinary);assert(apphost_bc_eval(ctx,src,strlen(src),fileno(ordinary),&ok)==-1);free(copy);fclose(ordinary);close(fd);free(bc);
    src="throw Error('expected');";bc=axia_app_qjs_compile(src,strlen(src),"fixture",&len);assert(bc);fd=cache(src,bc,len,1);assert(apphost_bc_eval(ctx,src,strlen(src),fd,&ok)==1&&ok==0);JS_FreeValue(ctx,JS_GetException(ctx));close(fd);free(bc);
    uint8_t bad[]={0xff,0xff};fd=cache(src,bad,sizeof bad,1);assert(apphost_bc_eval(ctx,src,strlen(src),fd,&ok)==-1);close(fd);
    assert(apphost_bc_dup_ro()==-1);apphost_bc_start("verified-fixture");fd=-1;for(int i=0;i<600&&fd<0;i++){usleep(10000);fd=apphost_bc_dup_ro();}assert(fd>=0);assert((fcntl(fd,F_GETFL)&O_ACCMODE)==O_RDONLY);assert(write(fd,"x",1)==-1);assert(apphost_bc_eval(ctx,host_source,strlen(host_source),fd,&ok)==0&&ok==1);close(fd);
    JS_FreeContext(ctx);JS_FreeRuntime(rt);
}
static void prepare(void) {
    char tmp[]="/tmp/axia-app-ffi-XXXXXX";assert(mkdtemp(tmp));assert(snprintf(home,sizeof home,"%s",tmp)<(int)sizeof home);assert(!setenv("HOME",home,1));
    assert(snprintf(assets,sizeof assets,"%s/assets",tmp)<(int)sizeof assets);assert(!mkdir(assets,0700));
    assert(snprintf(fonts,sizeof fonts,"%s/fonts",assets)<(int)sizeof fonts);assert(!mkdir(fonts,0700));assert(snprintf(bundle,sizeof bundle,"%s/app-host.js",assets)<(int)sizeof bundle);
    char p[8192];assert(snprintf(p,sizeof p,"%s/sysapps",assets)<(int)sizeof p);assert(!mkdir(p,0700));assert(snprintf(p,sizeof p,"%s/sysapps/notes",assets)<(int)sizeof p);assert(!mkdir(p,0700));assert(snprintf(p,sizeof p,"%s/sysapps/notes/manifest.json",assets)<(int)sizeof p);
    FILE *f=fopen(p,"w");assert(f);fputs("{\"size\":[860,560],\"minSize\":[560,360]}",f);fclose(f);
    char out[4096];assert(app_package_dir("notes",assets,out,sizeof out)==1);assert(app_package_dir("../notes",assets,out,sizeof out)==-1);assert(app_package_dir("notes",assets,out,2)==-1);
}
static void run_client(int which) {
    pid_t pid=fork();assert(pid>=0);if(!pid){mode=which;assert(app_main("notes",bundle,fonts)==0);assert(sandboxed&&minw==560&&minh==360);assert(pointer_downs==1&&gesture_updates==3&&key_events==1&&keymaps==1&&events>=1);assert(firstw==860&&firsth==560&&paints>0&&commits>0&&releases>0&&shutdowns==1);if(mode==2)assert(recovered);for(int i=0;i<nproxy;i++)if(is(&proxies[i],"wl_callback")||is(&proxies[i],"wl_buffer"))assert(!proxies[i].live);_exit(0);}int status=0;assert(waitpid(pid,&status,0)==pid&&WIFEXITED(status)&&WEXITSTATUS(status)==0);
}
static int cleanup(const char *path,const struct stat *st,int type,struct FTW *walk) {(void)st;(void)type;(void)walk;return remove(path);}
int main(void) {prepare();launch_test();assert(app_main("../bad",bundle,fonts)==2);run_client(0);run_client(1);run_client(2);bytecode_test();assert(nftw(home,cleanup,16,FTW_DEPTH|FTW_PHYS)==0);puts("app-mode-ffi-test: Rust window ABI, configure/render/release, multiwindow, recovery, sandbox order and verified bytecode passed");return 0;}
