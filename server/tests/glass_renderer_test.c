/* Frozen legacy-C pixel hashes: odd sizes, padding, alpha, fractional corners,
 * clip margins and changing content. Both cold and cached Rust paths must match,
 * including real worker threads. No C blur implementation is shipped here. */
#include "canvas.h"
#include "par.h"
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
extern void axia_glass_reset(void); /* no outstanding job when called */
#ifndef GLASS_REFERENCE
static const uint64_t golden[32] = {
#include "glass_renderer_golden.h"
};
#endif
static uint64_t hash(const uint32_t *p,size_t n) {uint64_t h=1469598103934665603ull;for(size_t i=0;i<n;i++)h=(h^p[i])*1099511628211ull;return h;}
int main(void) {
    par_init(4);int checks=0;
    for(int threaded=0;threaded<2;threaded++) {
        canvas_parallel(threaded?par_rows:NULL);
        for(int k=0;k<32;k++) {
            int w=k<16?67:647,h=k<16?49:401,stride=w+7;
            size_t n=(size_t)stride*h;
            uint32_t *source=malloc(n*4),*px=malloc(n*4);assert(source&&px);
            uint32_t state=0x693ad01u+(unsigned)k;
            for(size_t i=0;i<n;i++){state=state*1664525u+1013904223u;unsigned a=k%3==0?state>>24:255;
                source[i]=a<<24|((((state>>16)&255)*a/255)<<16)|((((state>>8)&255)*a/255)<<8)|((state&255)*a/255);}
            Canvas c;canvas_init(&c,px,w,h,stride);
            if(k%4==1)c.clip=(Rect){5,3,w-8,h-5};
            for(int warm=0;warm<2;warm++){
                memcpy(px,source,n*4);if(!warm)axia_glass_reset();
                canvas_backdrop_blur(&c,k%2?3.25f:-2.5f,k%3?4.5f:-3.0f,w-9.25f,h-7.5f,(float)(k%9), (int[]){2,7,12,24}[k%4]);
                uint64_t result=hash(px,n);
#ifdef GLASS_REFERENCE
                if(!threaded&&!warm)printf("0x%016llxull,\n",(unsigned long long)result);
#else
                if(result!=golden[k]){fprintf(stderr,"glass parity: case %d threaded=%d warm=%d: %016llx != %016llx\n",k,threaded,warm,(unsigned long long)result,(unsigned long long)golden[k]);return 1;}
                checks++;
#endif
            }
            free(source);free(px);
        }
    }
#ifndef GLASS_REFERENCE
    printf("glass renderer: %d frozen C pixel checks passed (serial, threaded, cold, cached)\n",checks);
#endif
    return 0;
}
