/* Real GLES shaders in a surfaceless Mesa context; no DRM master or live session. */
#include "../src/gpu/gpu.c"
#include <assert.h>

enum { W = 96, H = 64 };
static unsigned char pixels[W * H * 4];
static int red(int x, int y) { return pixels[((H - 1 - y) * W + x) * 4]; }

static void draw(GpuTex *texture, const GpuRoundClip *round) {
    glClearColor(0, 0, 1, 1);
    glClear(GL_COLOR_BUFFER_BIT);
    gpu_draw_clipped(texture, 0, 0, W, H, 0, 0, 1, 1, (Rect){0, 0, W, H}, 1, round);
    glReadPixels(0, 0, W, H, GL_RGBA, GL_UNSIGNED_BYTE, pixels);
    assert(glGetError() == GL_NO_ERROR);
}

int main(void) {
    setenv("EGL_PLATFORM", "surfaceless", 1);
    EGLDisplay display = eglGetDisplay(EGL_DEFAULT_DISPLAY);
    assert(eglInitialize(display, NULL, NULL));
    assert(eglBindAPI(EGL_OPENGL_ES_API));
    EGLint attrs[] = {EGL_SURFACE_TYPE, EGL_PBUFFER_BIT, EGL_RENDERABLE_TYPE, EGL_OPENGL_ES2_BIT,
        EGL_RED_SIZE, 8, EGL_GREEN_SIZE, 8, EGL_BLUE_SIZE, 8, EGL_ALPHA_SIZE, 8, EGL_NONE};
    EGLConfig config; EGLint count;
    assert(eglChooseConfig(display, attrs, &config, 1, &count) && count);
    EGLint size[] = {EGL_WIDTH, W, EGL_HEIGHT, H, EGL_NONE};
    EGLSurface surface = eglCreatePbufferSurface(display, config, size);
    EGLint version[] = {EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE};
    EGLContext context = eglCreateContext(display, config, EGL_NO_CONTEXT, version);
    assert(eglMakeCurrent(display, surface, surface, context));
    G.on = 1; G.gen = 1; G.cur = &G.out[0]; G.cur->W = W; G.cur->H = H;
    assert(!make_prog(0, fs_2d));
    if (strstr((const char *)glGetString(GL_EXTENSIONS), "GL_OES_EGL_image_external"))
        assert(!make_prog(1, fs_ext));
    glViewport(0, 0, W, H);
    glEnable(GL_BLEND); glBlendFunc(GL_ONE, GL_ONE_MINUS_SRC_ALPHA);
    uint32_t color = 0xffff0000;
    GpuTex *texture = gpu_tex_pixels(NULL, &color, 1, 1, 1); assert(texture);
    GpuRoundClip round = {0, 0, W, H, 12, 12};
    draw(texture, &round);
    assert(red(0, 0) == 255 && red(W - 1, 0) == 255); /* square top */
    assert(red(0, H - 1) == 0 && red(W - 1, H - 1) == 0);
    assert(red(W / 2, H - 1) == 255); /* bottom edge keeps its full size */
    int soft = 0;
    for (int y = H - 12; y < H; y++) for (int x = 0; x < 12; x++)
        soft += red(x, y) > 0 && red(x, y) < 255;
    assert(soft); /* antialiasing */
    /* Reading a retained texture must not inherit the last window's clipping uniforms. */
    uint32_t readback; assert(!gpu_tex_read(texture, &readback, 1, 1));
    assert(readback == color);
    round = (GpuRoundClip){24, 16, W / 2, H / 2, 6, 6};
    draw(texture, &round);
    assert(red(24, 47) == 0 && red(71, 47) == 0 && red(48, 47) == 255);
    draw(texture, NULL); /* fullscreen/restored windows do not inherit the mask */
    assert(red(0, H - 1) == 255 && red(W - 1, H - 1) == 255);
    color = 0x80800000; /* premultiplied translucent client */
    texture = gpu_tex_pixels(texture, &color, 1, 1, 0);
    round = (GpuRoundClip){0, 0, W, H, 12, 12};
    draw(texture, &round);
    assert(red(W / 2, H / 2) >= 127 && red(W / 2, H / 2) <= 129);
    assert(red(0, H - 1) == 0 && red(W - 1, H - 1) == 0);
    gpu_tex_free(texture);
    printf("gpu-round: real shaders, square top, rounded bottom, scaling, antialiasing and fullscreen reset passed (%s)\n", glGetString(GL_RENDERER));
    eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, EGL_NO_CONTEXT);
    eglDestroyContext(display, context); eglDestroySurface(display, surface); eglTerminate(display);
    return 0;
}
