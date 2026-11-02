/* mandel.c — Mandelbrot set for the Fractal sample app (compiled to WebAssembly).
 * clang --target=wasm32 -O2 -nostdlib -Wl,--no-entry -Wl,--export-dynamic -o ../assets/mandel.wasm mandel.c
 *
 * render() writes w×h RGBA pixels into the module's memory (from pixels()) and returns the
 * total number of iterations (for measuring). */
#define MAXW 1024
#define MAXH 768

static unsigned char buf[MAXW * MAXH * 4];

__attribute__((export_name("pixels"))) unsigned char *pixels(void) { return buf; }

static unsigned color(int i, int max) {
    if (i >= max) return 0xff000000u;
    float t = (float)i / (float)max;
    int r = (int)(9 * (1 - t) * t * t * t * 255), g = (int)(15 * (1 - t) * (1 - t) * t * t * 255),
        b = (int)(8.5f * (1 - t) * (1 - t) * (1 - t) * t * 255);
    return 0xff000000u | (unsigned)b << 16 | (unsigned)g << 8 | (unsigned)r;
}

__attribute__((export_name("render")))
long long render(int w, int h, double cx, double cy, double scale, int max) {
    if (w > MAXW) w = MAXW;
    if (h > MAXH) h = MAXH;
    long long total = 0;
    unsigned *px = (unsigned *)buf;
    for (int y = 0; y < h; y++) {
        double ci = cy + (y - h / 2.0) * scale;
        for (int x = 0; x < w; x++) {
            double cr = cx + (x - w / 2.0) * scale, zr = 0, zi = 0;
            int i = 0;
            while (i < max && zr * zr + zi * zi < 4) {
                double t = zr * zr - zi * zi + cr;
                zi = 2 * zr * zi + ci;
                zr = t;
                i++;
            }
            total += i;
            px[y * w + x] = color(i, max);
        }
    }
    return total;
}
