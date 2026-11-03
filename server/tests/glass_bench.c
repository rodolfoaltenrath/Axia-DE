/* Standalone repeatable backdrop workload. Compile against the reference canvas.c
 * and the new renderer; matching checksums guard against a faster, incorrect result.
 * Timings exclude setup and checksum; no input event is generated. */
#include "canvas.h"
#include <stdio.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
static double now(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t); return t.tv_sec+t.tv_nsec/1e9; }
int main(int argc,char **argv) {
    int dynamic=argc>1 && !strcmp(argv[1],"changing");
    enum { W=1366,H=220,N=400 };
    uint32_t *base=malloc(W*H*4),*pixels=malloc(W*H*4);
    if (!base || !pixels) return 1;
    for (int y=0;y<H;y++) for (int x=0;x<W;x++) base[y*W+x]=0xff000000u | ((x*531+y*173)&0xffffff);
    Canvas c;canvas_init(&c,pixels,W,H,W);
    double start=now();
    for (int i=0;i<N;i++) {
        memcpy(pixels,base,W*H*4);
        if (dynamic) for (int y=40;y<90;y++) for (int x=300;x<350;x++) pixels[y*W+x]=0xff000000u | (i*0x3a91);
        canvas_backdrop_blur(&c,180,36,1000,136,20,24);
    }
    double elapsed=now()-start;
    uint64_t hash=1469598103934665603ull;
    for (int i=0;i<W*H;i++) hash=(hash^pixels[i])*1099511628211ull;
    printf("%s: %.3f ms/frame checksum=%016llx\n",dynamic?"changing":"unchanged",elapsed*1000/N,(unsigned long long)hash);
    free(base);free(pixels);return 0;
}
