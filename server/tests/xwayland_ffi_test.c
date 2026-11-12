/* Real sockets, descriptors and Wayland callbacks in a private temporary tree.
 * Linker wrappers redirect X paths and intercept execve; never starts host Xwayland. */
#include "wl/xwayland.h"
#include "sec/cgroup.h"
#include <wayland-server-core.h>
#include <assert.h>
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <sched.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/eventfd.h>
#include <sys/prctl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static char tree[]="/tmp/axwl-XXXXXX";
static struct wl_display *display;
static struct wl_client *tagged;
static int wmfd=-1, wm_starts, wm_stops, wm_fail, client_fail;
static int binary_present=1, binary_mode=0755, binary_owner;
static int fail_socket, fail_pair, pair_calls, pair_fds[2][2], fail_bind, fail_listen;
static int fake_fork=1, fail_fork, fork_calls, fake_pid=50000000, active_pid, pidfd=-1, pidfd_fail;
static int retained[2]={-1,-1}, reaped, exit_status=1<<8, kills, sleeps, source_fail;
static int listen_fd=-1;
static int managed, scope_fail, exec_fail, unshare_fail, child_reports, last_status;
static time_t clock_now=1000;
static int foreign_lock, missing_env, fd_fail, dup_fail, null_log;
static pid_t fixture_pid;

static const char *path(const char *p,char out[256]) {
    if(!strncmp(p,"/tmp/.X",7)){snprintf(out,256,"%s/%s",tree,p+5);return out;}
    if(!strncmp(p,"/proc/self/",11)){snprintf(out,256,"%s/%s",tree,p+11);return out;}
    return p;
}
int __real_stat(const char*,struct stat*);
int __wrap_stat(const char *p,struct stat *s){
    if(!strcmp(p,"/usr/bin/Xwayland")){
        if(!binary_present){errno=ENOENT;return -1;}memset(s,0,sizeof *s);s->st_mode=S_IFREG|binary_mode;s->st_uid=binary_owner;return 0;
    }char q[256];return __real_stat(path(p,q),s);
}
int __real_lstat(const char*,struct stat*);
int __wrap_lstat(const char *p,struct stat *s){char q[256];int r=__real_lstat(path(p,q),s);if(!r&&foreign_lock&&!strcmp(p,"/tmp/.X0-lock"))s->st_uid=getuid()+1;return r;}
int __real_open(const char*,int,...);
int __wrap_open(const char *p,int flags,...){
    mode_t mode=0;if(flags&O_CREAT){va_list ap;va_start(ap,flags);mode=va_arg(ap,int);va_end(ap);}
    char q[256];return __real_open(path(p,q),flags,mode);
}
int __real_unlink(const char*);
int __wrap_unlink(const char*p){char q[256];return __real_unlink(path(p,q));}
int __real_chmod(const char*,mode_t);
int __wrap_chmod(const char*p,mode_t mode){char q[256];return __real_chmod(path(p,q),mode);}
int __real_mkdir(const char*,mode_t);
int __wrap_mkdir(const char*p,mode_t mode){char q[256];return __real_mkdir(path(p,q),mode);}
int __real_bind(int,const struct sockaddr*,socklen_t);
int __wrap_bind(int fd,const struct sockaddr*a,socklen_t len){
    if(fail_bind){fail_bind--;errno=EADDRINUSE;return -1;}
    if(a->sa_family==AF_UNIX){struct sockaddr_un b=*(const struct sockaddr_un*)a;char q[256];const char *p=path(b.sun_path,q);
        assert(strlen(p)<sizeof b.sun_path);strcpy(b.sun_path,p);return __real_bind(fd,(struct sockaddr*)&b,sizeof b);}
    return __real_bind(fd,a,len);
}
int __real_listen(int,int);
int __wrap_listen(int fd,int backlog){if(fail_listen){fail_listen--;errno=EIO;return -1;}listen_fd=fd;return __real_listen(fd,backlog);}
int __real_socket(int,int,int);
int __wrap_socket(int domain,int type,int protocol){if(fail_socket){fail_socket--;errno=EMFILE;return -1;}return __real_socket(domain,type,protocol);}
int __real_socketpair(int,int,int,int*);
int __wrap_socketpair(int domain,int type,int protocol,int *fds){
    pair_calls++;if(fail_pair==pair_calls){errno=EMFILE;return -1;}
    int r=__real_socketpair(domain,type,protocol,fds);if(!r){pair_fds[(pair_calls-1)%2][0]=fds[0];pair_fds[(pair_calls-1)%2][1]=fds[1];}return r;
}
pid_t __real_fork(void);
pid_t __wrap_fork(void){
    fork_calls++;if(fail_fork){errno=EAGAIN;return -1;}
    if(!fake_fork)return __real_fork();
    int incoming;while((incoming=accept4(listen_fd,NULL,NULL,SOCK_CLOEXEC|SOCK_NONBLOCK))>=0)close(incoming);
    for(int i=0;i<2;i++){retained[i]=fcntl(pair_fds[i][1],F_DUPFD_CLOEXEC,100);assert(retained[i]>=100);}
    active_pid=++fake_pid;reaped=0;return active_pid;
}
pid_t __real_waitpid(pid_t,int*,int);
pid_t __wrap_waitpid(pid_t pid,int *status,int flags){
    if(pid>=50000000){assert(pid==active_pid);if(!reaped)return 0;if(status)*status=exit_status;return pid;}
    int st=0;pid_t r=__real_waitpid(pid,&st,flags);if(r>0)last_status=st;if(status)*status=st;return r;
}
int __real_kill(pid_t,int);
int __wrap_kill(pid_t pid,int sig){
    if(pid>=50000000){if(sig==0){errno=ESRCH;return -1;}assert(pid==active_pid&&sig==SIGTERM);kills++;return 0;}
    return __real_kill(pid,sig);
}
long __real_syscall(long,...);
long __wrap_syscall(long number,...){
    va_list ap;va_start(ap,number);long r;
    if(number==SYS_pidfd_open){int pid=va_arg(ap,int);unsigned flags=va_arg(ap,unsigned);assert(!flags);
        if(pidfd_fail){errno=ENOSYS;r=-1;}else if(pid>=50000000){pidfd=eventfd(0,EFD_CLOEXEC|EFD_NONBLOCK);assert(pidfd>=0);r=pidfd;}
        else r=__real_syscall(number,pid,flags);
    }else if(number==SYS_close_range){unsigned first=va_arg(ap,unsigned),last=va_arg(ap,unsigned),flags=va_arg(ap,unsigned);r=__real_syscall(number,first,last,flags);}
    else{assert(!"unexpected syscall in XWayland fixture");r=-1;}
    va_end(ap);return r;
}
time_t __wrap_time(time_t *out){if(out)*out=clock_now;return clock_now;}
char *__real_getenv(const char*);
char *__wrap_getenv(const char *name){
    if(!strcmp(name,"HOME"))return missing_env?NULL:tree;
    if(!strcmp(name,"XDG_RUNTIME_DIR"))return missing_env?NULL:"/tmp/axia-xwl-runtime";
    if(!strcmp(name,"LORDOS_SESSIOND"))return managed?"1":NULL;
    return __real_getenv(name);
}
int __wrap_usleep(useconds_t usec){assert(usec==10000);sleeps++;return 0;}
int __wrap_unshare(int flags){assert(flags==(CLONE_NEWUSER|CLONE_NEWNET));if(unshare_fail){errno=EPERM;return -1;}return 0;}
int __real_fcntl(int,int,...);
int __wrap_fcntl(int fd,int cmd,...){
    if(cmd==F_GETFD||cmd==F_GETFL)return __real_fcntl(fd,cmd);
    va_list ap;va_start(ap,cmd);int arg=va_arg(ap,int);va_end(ap);
    if(fd_fail&&getpid()!=fixture_pid&&cmd==F_DUPFD_CLOEXEC){errno=EMFILE;return -1;}
    return __real_fcntl(fd,cmd,arg);
}
int __real_dup2(int,int);
int __wrap_dup2(int old,int target){if(dup_fail&&getpid()!=fixture_pid&&target==4){errno=EBADF;return -1;}return __real_dup2(old,target);}

static void check_map(const char *name,const char *expected){char p[256],b[64]={0};snprintf(p,sizeof p,"%s/%s",tree,name);int fd=__real_open(p,O_RDONLY);assert(fd>=0);ssize_t n=read(fd,b,sizeof b-1);close(fd);assert(n>=0&&!strcmp(b,expected));}
int cgroup_enter(const char *name,const CgroupLimits *limits){assert(!strcmp(name,"lordos-xwayland.scope")&&!limits->mem_high_mb&&!limits->mem_max_mb&&!limits->cpu_percent);return scope_fail?-1:0;}
int __wrap_execve(const char *file,char *const argv[],char *const env[]){
    assert(!strcmp(file,"/usr/bin/Xwayland"));
    int incoming;while((incoming=accept4(3,NULL,NULL,SOCK_CLOEXEC|SOCK_NONBLOCK))>=0)close(incoming);
    const char *want[]={"Xwayland",":0","-rootless","-listenfd","3","-wm","5","-nolisten","tcp","-extension","XTEST",NULL};
    for(unsigned i=0;i<12;i++){assert((argv[i]==NULL)==(want[i]==NULL));if(want[i])assert(!strcmp(argv[i],want[i]));}
    char home[600];snprintf(home,sizeof home,"HOME=%s",missing_env?"/":tree);
    assert(!strcmp(env[0],home)&&!strcmp(env[1],missing_env?"XDG_RUNTIME_DIR=/tmp":"XDG_RUNTIME_DIR=/tmp/axia-xwl-runtime"));
    assert(!strcmp(env[2],"PATH=/usr/bin:/bin")&&!strcmp(env[3],"WAYLAND_SOCKET=4")&&!strcmp(env[4],"LANG=C.UTF-8")&&!env[5]);
    for(int fd=0;fd<6;fd++)assert(fcntl(fd,F_GETFD)>=0);
    for(int fd=3;fd<6;fd++)assert(!(fcntl(fd,F_GETFD)&FD_CLOEXEC));
    for(int fd=6;fd<256;fd++){errno=0;assert(fcntl(fd,F_GETFD)==-1&&errno==EBADF);}
    int accepting=0;socklen_t n=sizeof accepting;assert(!getsockopt(3,SOL_SOCKET,SO_ACCEPTCONN,&accepting,&n)&&accepting);
    assert(prctl(PR_GET_NO_NEW_PRIVS,0,0,0,0)==1&&getsid(0)==getpid());
    struct stat output;assert(!fstat(1,&output));if(null_log||missing_env)assert(S_ISCHR(output.st_mode));else assert(S_ISREG(output.st_mode));
    if(!unshare_fail){char map[64];check_map("setgroups","deny");snprintf(map,sizeof map,"%u %u 1",getuid(),getuid());check_map("uid_map",map);snprintf(map,sizeof map,"%u %u 1",getgid(),getgid());check_map("gid_map",map);}
    if(exec_fail){errno=ENOENT;return -1;}
    const char report[]="child argv/env/fds/scope/namespace policy passed";
    assert(write(5,report,sizeof report)==sizeof report);_exit(0);
}

struct wl_client *__real_wl_client_create(struct wl_display*,int);
struct wl_client *__wrap_wl_client_create(struct wl_display*d,int fd){return client_fail?NULL:__real_wl_client_create(d,fd);}
struct wl_event_source *__real_wl_event_loop_add_fd(struct wl_event_loop*,int,uint32_t,wl_event_loop_fd_func_t,void*);
struct wl_event_source *__wrap_wl_event_loop_add_fd(struct wl_event_loop*l,int fd,uint32_t mask,wl_event_loop_fd_func_t fn,void *data){if(source_fail){source_fail--;return NULL;}return __real_wl_event_loop_add_fd(l,fd,mask,fn,data);}
int __real_wl_event_source_remove(struct wl_event_source*);
int __wrap_wl_event_source_remove(struct wl_event_source*s){assert(s);return __real_wl_event_source_remove(s);}
struct wl_display *wl__display(void){return display;}
void wlx_set_client(struct wl_client*c){tagged=c;}
int xwm_start(int fd,struct wl_event_loop *loop){assert(loop==wl_display_get_event_loop(display));assert(wmfd<0);wm_starts++;if(wm_fail){close(fd);return -1;}wmfd=fd;assert(fcntl(fd,F_SETFL,fcntl(fd,F_GETFL)|O_NONBLOCK)==0);return 0;}
void xwm_stop(void){wm_stops++;if(wmfd>=0){char b[128]={0};ssize_t n=read(wmfd,b,sizeof b);if(n>0){assert(!strcmp(b,"child argv/env/fds/scope/namespace policy passed"));child_reports++;}close(wmfd);wmfd=-1;}}

static void pump(void){assert(!wl_event_loop_dispatch(wl_display_get_event_loop(display),0));}
static int connect_x(void){
    int fd=__real_socket(AF_UNIX,SOCK_STREAM|SOCK_CLOEXEC,0);assert(fd>=0);
    char p[256];const char *s=path(xwl_socket_path(),p);struct sockaddr_un a={.sun_family=AF_UNIX};assert(strlen(s)<sizeof a.sun_path);strcpy(a.sun_path,s);
    assert(!connect(fd,(struct sockaddr*)&a,sizeof a));return fd;
}
static void close_retained(void){for(int i=0;i<2;i++)if(retained[i]>=0){close(retained[i]);retained[i]=-1;}}
static void reap_fake(int status){exit_status=status;reaped=1;uint64_t one=1;assert(write(pidfd,&one,sizeof one)==sizeof one);pump();close_retained();pump();assert(!xwl_running()&&!tagged&&wmfd<0);}
static void reset_attempt(void){pair_calls=0;fail_pair=0;fail_fork=0;wm_fail=0;client_fail=0;pidfd_fail=0;}
static void start_fake(void){reset_attempt();int before=fork_calls;int fd=connect_x();pump();close(fd);assert(xwl_running()&&tagged&&wmfd>=0&&fork_calls==before+1);}
static void create(const char*p,const char*contents){int fd=__real_open(p,O_WRONLY|O_CREAT|O_TRUNC,0600);assert(fd>=0);assert(write(fd,contents,strlen(contents))==(ssize_t)strlen(contents));close(fd);}
static void make_path(const char *suffix){char p[256];snprintf(p,sizeof p,"%s/%s",tree,suffix);assert(!__real_mkdir(p,0700));}
static void clean_tree(void){char p[256];const char *files[]={"setgroups","uid_map","gid_map",".cache/axia/xwayland.log"};for(unsigned i=0;i<4;i++){snprintf(p,sizeof p,"%s/%s",tree,files[i]);__real_unlink(p);}snprintf(p,sizeof p,"%s/.cache/axia",tree);rmdir(p);snprintf(p,sizeof p,"%s/.cache",tree);rmdir(p);snprintf(p,sizeof p,"%s/.X11-unix",tree);rmdir(p);assert(!rmdir(tree));}
static int descriptor_count(void){DIR *dir=opendir("/proc/self/fd");assert(dir);int n=0;struct dirent *e;while((e=readdir(dir)))if(e->d_name[0]!='.')n++;closedir(dir);return n;}
static void reservation_checks(void){
    char p[256],pid[32];snprintf(p,sizeof p,"%s/.X11-unix",tree);create(p,"not a directory");assert(xwl_init()==-1);assert(!__real_unlink(p));
    snprintf(pid,sizeof pid,"%d\n",getpid());snprintf(p,sizeof p,"%s/.X0-lock",tree);create(p,pid);
    assert(!xwl_init()&&!strcmp(xwl_display(),":1"));xwl_stop();assert(!access(p,F_OK));assert(!__real_unlink(p));
    create(p,"50000000\n");char sock[256];snprintf(sock,sizeof sock,"%s/.X11-unix/X0",tree);
    int fd=__real_socket(AF_UNIX,SOCK_STREAM|SOCK_CLOEXEC,0);assert(fd>=0);struct sockaddr_un a={.sun_family=AF_UNIX};assert(strlen(sock)<sizeof a.sun_path);strcpy(a.sun_path,sock);assert(!__real_bind(fd,(struct sockaddr*)&a,sizeof a));close(fd);
    assert(!xwl_init()&&!strcmp(xwl_display(),":0"));xwl_stop();assert(access(p,F_OK)&&access(sock,F_OK));
    create(p,"50000000\n");foreign_lock=1;assert(!xwl_init()&&!strcmp(xwl_display(),":1"));xwl_stop();assert(!access(p,F_OK));foreign_lock=0;assert(!__real_unlink(p));
    assert(!symlink("missing-target",p));assert(!xwl_init()&&!strcmp(xwl_display(),":1"));xwl_stop();assert(!__real_unlink(p));
    for(int i=0;i<64;i++){snprintf(p,sizeof p,"%s/.X%d-lock",tree,i);create(p,pid);}assert(xwl_init()==-1);
    for(int i=0;i<64;i++){snprintf(p,sizeof p,"%s/.X%d-lock",tree,i);assert(!__real_unlink(p));}
    source_fail=1;assert(!xwl_init());int before=fork_calls;fd=connect_x();pump();close(fd);assert(fork_calls==before);xwl_stop();
    fail_socket=1;assert(!xwl_init()&&!strcmp(xwl_display(),":0"));xwl_stop();
    fail_bind=1;assert(!xwl_init()&&!strcmp(xwl_display(),":0"));xwl_stop();
}

int main(void){
    fixture_pid=getpid();
    assert(mkdtemp(tree));make_path(".cache");make_path(".cache/axia");
    const char *maps[]={"setgroups","uid_map","gid_map"};for(unsigned i=0;i<3;i++){char p[256];snprintf(p,sizeof p,"%s/%s",tree,maps[i]);create(p,"");}
    assert(xwl_init()==-1&&!xwl_running()&&!xwl_display()[0]);display=wl_display_create();assert(display);
    binary_present=0;assert(xwl_init()==-1);binary_present=1;
    int initial_descriptors=descriptor_count();reservation_checks();assert(descriptor_count()==initial_descriptors);
    assert(!xwl_init()&&!strcmp(xwl_display(),":0")&&!strcmp(xwl_socket_path(),"/tmp/.X11-unix/X0"));
    assert(xwl_init()==-1&&!xwl_running());
    char p[256];struct stat st;assert(!__real_stat(path(xwl_socket_path(),p),&st)&&(st.st_mode&0777)==0600);
    assert(!strcmp(__real_getenv("DISPLAY"),":0")&&!strcmp(__real_getenv("_JAVA_AWT_WM_NONREPARENTING"),"1"));
    start_fake();uint64_t signal=1;assert(write(pidfd,&signal,sizeof signal)==sizeof signal);pump();assert(xwl_running());
    int k=kills;xwl_wm_lost();assert(kills==k+1);reap_fake(0);
    for(int i=0;i<3;i++){start_fake();reap_fake(127<<8);}
    int before=fork_calls,fd=connect_x();pump();close(fd);assert(fork_calls==before&&!xwl_running());clock_now+=61;
    start_fake();xwl_idle();reap_fake(SIGTERM); /* idle is not a failed start */
    for(int policy=0;policy<3;policy++){binary_mode=policy==0?0775:policy==1?0644:0755;binary_owner=policy==2?1:0;int forks=fork_calls;fd=connect_x();pump();close(fd);assert(!xwl_running()&&fork_calls==forks);clock_now+=61;}binary_mode=0755;binary_owner=0;
    reset_attempt();fail_pair=2;before=fork_calls;fd=connect_x();pump();close(fd);assert(fork_calls==before&&!xwl_running());
    reset_attempt();fail_fork=1;fd=connect_x();pump();close(fd);assert(!xwl_running());
    reset_attempt();client_fail=1;fd=connect_x();pump();close(fd);assert(xwl_running()&&!tagged);reap_fake(1<<8);
    reset_attempt();wm_fail=1;fd=connect_x();pump();close(fd);assert(xwl_running()&&tagged&&wmfd<0);reap_fake(1<<8);
    start_fake();struct wl_client *gone=tagged;k=kills;wl_client_destroy(gone);assert(!tagged&&kills==k+1);reap_fake(0);
    reset_attempt();pidfd_fail=1;fd=connect_x();pump();close(fd);assert(xwl_running()&&tagged);xwl_stop();close_retained();wl_display_destroy_clients(display);
    assert(!xwl_init());
    start_fake();xwl_stop();assert(!xwl_running()&&!xwl_display()[0]&&!xwl_socket_path()[0]);close_retained();wl_display_destroy_clients(display);
    assert(!xwl_init());
    /* Real fork; execve and unshare remain controlled. Child uses actual dup/close_range. */
    fake_fork=0;
    for(int test=0;test<10;test++){
        reset_attempt();managed=test==2;scope_fail=test==2||test==6;exec_fail=test==3;unshare_fail=test==1;fd_fail=test==7;dup_fail=test==8;missing_env=test==9;null_log=test==5;
        char log[256];snprintf(log,sizeof log,"%s/.cache/axia/xwayland.log",tree);
        if(test==4){int logfd=__real_open(log,O_WRONLY);assert(logfd>=0&&!ftruncate(logfd,(1<<20)+1));close(logfd);}
        if(test==5){assert(!__real_unlink(log));char guard[256];snprintf(guard,sizeof guard,"%s/log-guard",tree);create(guard,"unchanged");assert(!symlink(guard,log));}
        int reports=child_reports;fd=connect_x();pump();close(fd);
        for(int spin=0;spin<2000&&xwl_running();spin++){pump();struct pollfd wait={.fd=wl_event_loop_get_fd(wl_display_get_event_loop(display)),.events=POLLIN};poll(&wait,1,2);}
        assert(!xwl_running()&&!tagged&&wmfd<0);
        int code=test==2||test==7||test==8?126:test==3?127:0;
        assert(WIFEXITED(last_status)&&WEXITSTATUS(last_status)==code);
        assert(child_reports==reports+(code==0));
        if(test==4){assert(!__real_stat(log,&st)&&st.st_size==0);}
        if(test==5){char guard[256],value[16]={0};snprintf(guard,sizeof guard,"%s/log-guard",tree);int g=__real_open(guard,O_RDONLY);assert(g>=0&&read(g,value,sizeof value)==9&&!strcmp(value,"unchanged"));close(g);assert(!__real_unlink(guard)&&!__real_unlink(log));create(log,"");}
    }
    fd_fail=dup_fail=missing_env=0;
    xwl_stop();wl_display_destroy_clients(display);assert(descriptor_count()==initial_descriptors);
    xwl_stop();wl_display_destroy_clients(display);wl_display_destroy(display);display=NULL;
    assert(!xwl_running()&&!xwl_display()[0]&&!xwl_socket_path()[0]);clean_tree();
    printf("XWayland lifecycle ABI: reservations, lazy startup, restart budget, failure cleanup and %d verified child exec plans passed\n",child_reports);
}
