/* Real XWayland peer: import/export INCR, owner exit, encodings and PRIMARY. */
#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <poll.h>
#include <time.h>
#include <unistd.h>
static Display *d;
static Window win;
static Atom targets,utf8,incr,clip,prop,offered;
static unsigned char *payload;
static size_t length,offset;
static Window receiver;
static Atom receiver_prop;
static int exporting;
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec+t.tv_nsec/1e9;}
static void require(int condition,const char *message){if(!condition){fprintf(stderr,"X11 clipboard: %s\n",message);exit(1);}}
static void respond(XSelectionRequestEvent *r){
    Atom p=r->property?r->property:r->target;
    XEvent e={0};e.xselection=(XSelectionEvent){.type=SelectionNotify,.display=d,.requestor=r->requestor,.selection=r->selection,.target=r->target,.property=p,.time=r->time};
    if(r->target==targets){Atom list[]={targets,offered};XChangeProperty(d,r->requestor,p,XA_ATOM,32,PropModeReplace,(unsigned char*)list,2);}
    else if(r->target==offered){
        if(length>65536){unsigned long total=length;receiver=r->requestor;receiver_prop=p;offset=0;exporting=1;XSelectInput(d,receiver,PropertyChangeMask);XChangeProperty(d,receiver,p,incr,32,PropModeReplace,(unsigned char*)&total,1);}
        else XChangeProperty(d,r->requestor,p,offered,8,PropModeReplace,payload,length);
    }else e.xselection.property=None;
    XSendEvent(d,r->requestor,False,0,&e);XFlush(d);
}
static int event(XEvent *out,int ms){
    if(!XPending(d)){struct pollfd p={ConnectionNumber(d),POLLIN,0};if(poll(&p,1,ms)<=0)return 0;}
    if(!XPending(d))return 0;
    XEvent e;XNextEvent(d,&e);
    if(e.type==SelectionRequest){respond(&e.xselectionrequest);return 0;}
    if(e.type==PropertyNotify&&exporting&&e.xproperty.window==receiver&&e.xproperty.atom==receiver_prop&&e.xproperty.state==PropertyDelete){size_t n=length-offset;if(n>65536)n=65536;XChangeProperty(d,receiver,receiver_prop,offered,8,PropModeReplace,payload+offset,n);offset+=n;if(!n)exporting=0;XFlush(d);return 0;}
    *out=e;return 1;
}
static void settle(int ms){double end=now()+ms/1000.;XEvent e;while(now()<end)event(&e,20);}
static void take(Atom selection,Atom target,const unsigned char *expected,size_t expected_len){
    XDeleteProperty(d,win,prop);XConvertSelection(d,selection,target,prop,win,CurrentTime);XFlush(d);
    size_t got=0;int receiving_incr=0,notified=0;double end=now()+10;
    while(now()<end){XEvent e;if(!event(&e,20))continue;
        int inspect=(e.type==SelectionNotify&&e.xselection.selection==selection)||(receiving_incr&&e.type==PropertyNotify&&e.xproperty.window==win&&e.xproperty.atom==prop&&e.xproperty.state==PropertyNewValue);
        if(!inspect)continue;
        if(e.type==SelectionNotify){require(e.xselection.property!=None,"conversion refused");notified=1;}
        Atom type;int format;unsigned long n,left;unsigned char *bytes=NULL;
        XGetWindowProperty(d,win,prop,0,4<<20,True,AnyPropertyType,&type,&format,&n,&left,&bytes);
        if(type==incr){receiving_incr=1;XFree(bytes);XFlush(d);continue;}
        require(notified,"property arrived before notification");require(format==8,"text/PNG property format");require(got+n<=expected_len,"too much data");require(!n||!memcmp(expected+got,bytes,n),"converted bytes differ");got+=n;XFree(bytes);XFlush(d);
        if(!receiving_incr||!n){require(got==expected_len,"incomplete paste");return;}
    }
    require(0,"conversion timed out");
}
static void copy_roundtrip(Atom selection,Atom mime,unsigned char *body,size_t size,Atom request,const unsigned char *expected,size_t expected_len){
    payload=body;length=size;offered=mime;exporting=0;XSetSelectionOwner(d,selection,win,CurrentTime);XFlush(d);
    settle(500);
    require(!exporting,"source INCR did not complete");
    XSetSelectionOwner(d,selection,None,CurrentTime);XFlush(d);
    double end=now()+5;Window owner;
    do{settle(20);owner=XGetSelectionOwner(d,selection);}while((owner==None||owner==win)&&now()<end);
    require(owner!=None&&owner!=win,"compositor did not retain the X11 selection");
    take(selection,request,expected,expected_len);
}
int main(void){
    d=XOpenDisplay(NULL);require(d!=NULL,"connect");win=XCreateSimpleWindow(d,DefaultRootWindow(d),20,20,320,160,0,0,0x315b87);XStoreName(d,win,"Native clipboard X11 peer");XSelectInput(d,win,StructureNotifyMask|PropertyChangeMask|FocusChangeMask);XMapWindow(d,win);XFlush(d);
    targets=XInternAtom(d,"TARGETS",False);utf8=XInternAtom(d,"UTF8_STRING",False);incr=XInternAtom(d,"INCR",False);clip=XInternAtom(d,"CLIPBOARD",False);prop=XInternAtom(d,"AXIA_CLIP_TEST",False);
    settle(800);Window focused;int revert;XGetInputFocus(d,&focused,&revert);require(focused==win,"mapped window lacks focus");
    size_t size=1<<20;unsigned char *large=malloc(size);require(large!=NULL,"allocation");for(size_t i=0;i<size;i++)large[i]='a'+i%26;
    copy_roundtrip(clip,utf8,large,size,utf8,large,size);
    unsigned char latin[]={ 'c','a','f',0xe9 };unsigned char unicode[]={ 'c','a','f',0xc3,0xa9 };
    copy_roundtrip(clip,XA_STRING,latin,sizeof latin,utf8,unicode,sizeof unicode);take(clip,XA_STRING,latin,sizeof latin);
    unsigned char primary[]="primary text";copy_roundtrip(XA_PRIMARY,utf8,primary,sizeof primary-1,utf8,primary,sizeof primary-1);
    free(large);XCloseDisplay(d);puts("X11 clipboard: passed (INCR both directions, retained copy, UTF-8/Latin-1 and PRIMARY)");return 0;
}
