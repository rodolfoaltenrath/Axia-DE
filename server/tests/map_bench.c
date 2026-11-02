/* map_bench.c — render time of the Monitor's vector map (src/monitor/map.c).
 *
 *   build/map-bench [assets folder] [width height]
 *
 * Requests frames like the app's (whole world, Europe and South America zoomed in) and measures how
 * long the map thread takes for each. It also writes build/map-*.ppm so you can look at them. */
#include "monitor/map.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void wait_frame(uint64_t want, MapReq *v, uint8_t **px, double *ms) {
    static uint64_t seq;
    for (;;) {
        uint8_t *p = NULL;
        if (map_take(&seq, &p, v, ms) && seq >= want) { *px = p; return; }
        free(p);
        struct timespec ts = { 0, 500000 };
        nanosleep(&ts, NULL);
    }
}

static void save(const char *name, const MapReq *v, const uint8_t *px) {
    FILE *f = fopen(name, "wb");
    if (!f) return;
    fprintf(f, "P6\n%d %d\n255\n", v->w, v->h);
    for (int i = 0; i < v->w * v->h; i++) {             /* sobre fondo oscuro */
        const uint8_t *p = px + i * 4;
        unsigned char rgb[3];
        for (int k = 0; k < 3; k++) rgb[k] = (unsigned char)((p[k] * p[3] + 0x26 * (255 - p[3])) / 255);
        fwrite(rgb, 1, 3, f);
    }
    fclose(f);
}

int main(int argc, char **argv) {
    map_init(argc > 1 ? argv[1] : "../assets");
    int w = argc > 3 ? atoi(argv[2]) : 800, h = argc > 3 ? atoi(argv[3]) : 560;
    struct { const char *name; double lon, lat, ppd; } views[] = {
        { "mundo", 0, 12.5, 0 }, { "europa", 12, 50, 0 }, { "sudamerica", -60, -22, 0 },
    };
    views[0].ppd = w / 360.0 > h / 145.0 ? w / 360.0 : h / 145.0;
    views[1].ppd = views[0].ppd * 6;
    views[2].ppd = views[0].ppd * 4;
    MapReq q;
    memset(&q, 0, sizeof q);
    q.w = w; q.h = h;
    q.land = 0x8a93a84d; q.border = 0xcfd6e66b; q.accent = 0xbf5af2ff; q.home_color = 0x30d158ff; q.sel_color = 0xffd60aff;
    q.has_home = 1; q.hlon = -58.4; q.hlat = -34.6;
    const float dots[][2] = { { -98, 39 }, { 10.4, 51 }, { -2, 53 }, { 138, 36 }, { 134, -25 }, { -53, -10 }, { 25, -29 }, { 127.8, 36.4 }, { -106, 56 } };
    q.ndots = 9;
    for (int i = 0; i < 9; i++) { q.dots[i].lon = dots[i][0]; q.dots[i].lat = dots[i][1]; q.dots[i].r = 5; }
    q.dots[1].sel = 1;
    if (getenv("NODOTS")) { q.ndots = 0; q.has_home = 0; }   /* land only */
    for (int vi = 0; vi < 3; vi++) {
        double sum = 0, worst = 0;
        int n = 12;
        for (int k = 0; k < n; k++) {
            q.ppd = views[vi].ppd;
            q.lon0 = views[vi].lon - w / 2.0 / q.ppd + k * 0.3;        /* like dragging */
            q.lat0 = views[vi].lat + h / 2.0 / q.ppd;
            uint64_t s = map_request(&q);
            MapReq v;
            uint8_t *px;
            double ms;
            wait_frame(s, &v, &px, &ms);
            if (k) { sum += ms; if (ms > worst) worst = ms; }         /* the first one includes loading land.bin */
            if (k == 0) { char f[64]; snprintf(f, sizeof f, "build/map-%s.ppm", views[vi].name); save(f, &v, px); }
            free(px);
        }
        printf("%-11s %dx%d, %5.1f px/grado: %.1f ms promedio, %.1f ms peor\n", views[vi].name, w, h, views[vi].ppd, sum / (n - 1), worst);
    }
    return 0;
}
