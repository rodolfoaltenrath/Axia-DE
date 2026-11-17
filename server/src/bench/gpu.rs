//! Borrowed render FD, owned GBM/EGL context and GL resources; failed setup unwinds.
use super::{bench_now_ms, glscene::*};
use libc::c_void;
use std::{cell::RefCell, ffi::CStr};
#[link(name = "gbm")]
extern "C" {
    fn gbm_create_device(fd: i32) -> *mut c_void;
    fn gbm_device_destroy(p: *mut c_void);
}
#[link(name = "EGL")]
extern "C" {
    fn eglGetProcAddress(p: *const libc::c_char) -> *const c_void;
    fn eglGetDisplay(p: *mut c_void) -> *mut c_void;
    fn eglInitialize(d: *mut c_void, a: *mut i32, b: *mut i32) -> u32;
    fn eglQueryString(d: *mut c_void, k: i32) -> *const libc::c_char;
    fn eglBindAPI(a: u32) -> u32;
    fn eglChooseConfig(
        d: *mut c_void,
        a: *const i32,
        c: *mut *mut c_void,
        n: i32,
        count: *mut i32,
    ) -> u32;
    fn eglCreateContext(
        d: *mut c_void,
        c: *mut c_void,
        share: *mut c_void,
        a: *const i32,
    ) -> *mut c_void;
    fn eglMakeCurrent(d: *mut c_void, draw: *mut c_void, read: *mut c_void, c: *mut c_void) -> u32;
    fn eglDestroyContext(d: *mut c_void, c: *mut c_void) -> u32;
    fn eglTerminate(d: *mut c_void) -> u32;
}
const W: i32 = 1920;
const H: i32 = 1080;
const TEX: u32 = 0x0de1;
const FB: u32 = 0x8d40;
#[derive(Default)]
struct Gpu {
    gbm: *mut c_void,
    display: *mut c_void,
    context: *mut c_void,
    fbo: u32,
    color: u32,
    depth: u32,
    program: u32,
    layers: [u32; 8],
    pos: i32,
    tex: i32,
    alpha: i32,
    current: bool,
}
impl Drop for Gpu {
    fn drop(&mut self) {
        unsafe {
            if self.current {
                bench_scene_free();
                glDeleteProgram(self.program);
                glDeleteTextures(8, self.layers.as_ptr());
                glDeleteTextures(1, &self.color);
                glDeleteFramebuffers(1, &self.fbo);
                glDeleteRenderbuffers(1, &self.depth);
                eglMakeCurrent(
                    self.display,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                );
            }
            if !self.context.is_null() {
                eglDestroyContext(self.display, self.context);
            }
            if !self.display.is_null() {
                eglTerminate(self.display);
            }
            if !self.gbm.is_null() {
                gbm_device_destroy(self.gbm);
            }
        }
    }
}
thread_local! {static GPU:RefCell<Option<Gpu>>=const{RefCell::new(None)};}
unsafe fn open(fd: i32, info: *mut BenchGlInfo) -> Option<Gpu> {
    let mut g = Gpu::default();
    g.gbm = gbm_create_device(fd);
    if g.gbm.is_null() {
        return None;
    }
    let p = eglGetProcAddress(c"eglGetPlatformDisplayEXT".as_ptr());
    g.display = if p.is_null() {
        eglGetDisplay(g.gbm)
    } else {
        let f: unsafe extern "C" fn(u32, *mut c_void, *const i32) -> *mut c_void =
            std::mem::transmute(p);
        f(0x31d7, g.gbm, std::ptr::null())
    };
    if g.display.is_null()
        || eglInitialize(g.display, std::ptr::null_mut(), std::ptr::null_mut()) == 0
    {
        return None;
    }
    let ext = eglQueryString(g.display, 0x3055);
    if ext.is_null()
        || !CStr::from_ptr(ext)
            .to_bytes()
            .split(|b| *b == b' ')
            .any(|v| v == b"EGL_KHR_surfaceless_context")
    {
        return None;
    }
    if eglBindAPI(0x30a0) == 0 {
        return None;
    }
    let mut cfg = std::ptr::null_mut();
    let mut n = 0;
    if eglChooseConfig(g.display, [0x3040, 4, 0x3038].as_ptr(), &mut cfg, 1, &mut n) == 0 || n < 1 {
        return None;
    }
    g.context = eglCreateContext(
        g.display,
        cfg,
        std::ptr::null_mut(),
        [0x3098, 2, 0x3038].as_ptr(),
    );
    if g.context.is_null()
        || eglMakeCurrent(
            g.display,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            g.context,
        ) == 0
    {
        return None;
    }
    g.current = true;
    if bench_scene_init(info) != 0 {
        return None;
    }
    glGenFramebuffers(1, &mut g.fbo);
    glBindFramebuffer(FB, g.fbo);
    glGenTextures(1, &mut g.color);
    glBindTexture(TEX, g.color);
    glTexImage2D(TEX, 0, 0x1908, W, H, 0, 0x1908, 0x1401, std::ptr::null());
    glTexParameteri(TEX, 0x2801, 0x2601);
    glFramebufferTexture2D(FB, 0x8ce0, TEX, g.color, 0);
    glGenRenderbuffers(1, &mut g.depth);
    glBindRenderbuffer(0x8d41, g.depth);
    glRenderbufferStorage(0x8d41, 0x81a5, W, H);
    glFramebufferRenderbuffer(FB, 0x8d00, 0x8d41, g.depth);
    if glCheckFramebufferStatus(FB) != 0x8cd5 {
        return None;
    }
    g.program=program(c"attribute vec2 pos; varying vec2 uv; void main(){ uv = pos * 0.5 + 0.5; gl_Position = vec4(pos, 0.0, 1.0); }",c"precision mediump float; varying vec2 uv; uniform sampler2D tex; uniform float alpha;void main(){ vec4 c = texture2D(tex, uv); gl_FragColor = vec4(c.rgb, c.a * alpha); }")?;
    g.pos = glGetAttribLocation(g.program, c"pos".as_ptr());
    g.tex = glGetUniformLocation(g.program, c"tex".as_ptr());
    g.alpha = glGetUniformLocation(g.program, c"alpha".as_ptr());
    let mut px = Vec::<u32>::new();
    px.try_reserve_exact((W * H) as usize).ok()?;
    px.resize((W * H) as usize, 0);
    glGenTextures(8, g.layers.as_mut_ptr());
    for l in 0..8 {
        for y in 0..H {
            for x in 0..W {
                px[(y * W + x) as usize] = 0xc0000000
                    | (((x * (l + 1)) & 255) as u32) << 16
                    | (((y + l * 31) & 255) as u32) << 8
                    | (l * 32) as u32
            }
        }
        glBindTexture(TEX, g.layers[l as usize]);
        glTexImage2D(TEX, 0, 0x1908, W, H, 0, 0x1908, 0x1401, px.as_ptr().cast());
        glTexParameteri(TEX, 0x2801, 0x2601);
        glTexParameteri(TEX, 0x2800, 0x2601)
    }
    glFinish();
    if glGetError() != 0 {
        return None;
    }
    Some(g)
}
#[no_mangle]
pub unsafe extern "C" fn lb_gpu_open(fd: i32, info: *mut BenchGlInfo) -> i32 {
    lb_gpu_close();
    let g = open(fd, info);
    let ok = g.is_some();
    GPU.with(|v| *v.borrow_mut() = g);
    if ok {
        0
    } else {
        -1
    }
}
#[no_mangle]
pub extern "C" fn lb_gpu_close() {
    GPU.with(|g| *g.borrow_mut() = None)
}
fn valid(secs: f64) -> bool {
    secs.is_finite() && (0.0..=1800.0).contains(&secs)
}
#[no_mangle]
pub unsafe extern "C" fn lb_gpu_scene(secs: f64) -> f64 {
    if !valid(secs) {
        return -1.0;
    }
    GPU.with(|g| {
        let g = g.borrow();
        let Some(g) = g.as_ref() else { return -1.0 };
        glBindFramebuffer(FB, g.fbo);
        glEnable(0xb71);
        let start = bench_now_ms();
        let mut n = 0;
        loop {
            bench_scene_draw(W, H, n as f64 * 16.667);
            n += 1;
            if bench_now_ms() >= start + secs * 1000.0 {
                break;
            }
        }
        glDisable(0xb71);
        n as f64 * 1000.0 / (bench_now_ms() - start)
    })
}
unsafe fn blend(layers: i32, secs: f64) -> f64 {
    if !valid(secs) {
        return -1.0;
    }
    GPU.with(|g| {
        let g = g.borrow();
        let Some(g) = g.as_ref() else { return -1.0 };
        glBindFramebuffer(FB, g.fbo);
        glViewport(0, 0, W, H);
        glDisable(0xb71);
        glEnable(0xbe2);
        glBlendFunc(0x302, 0x303);
        let start = bench_now_ms();
        let mut n = 0;
        let p: [f32; 8] = [-1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0];
        loop {
            glClear(0x4000);
            for l in 0..layers {
                glUseProgram(g.program);
                glActiveTexture(0x84c0);
                glBindTexture(TEX, g.layers[l as usize % 8]);
                glUniform1i(g.tex, 0);
                glUniform1f(g.alpha, 0.9);
                glVertexAttribPointer(g.pos as u32, 2, 0x1406, 0, 0, p.as_ptr().cast());
                glEnableVertexAttribArray(g.pos as u32);
                glDrawArrays(5, 0, 4)
            }
            glFinish();
            n += 1;
            if bench_now_ms() >= start + secs * 1000.0 {
                break;
            }
        }
        glDisable(0xbe2);
        n as f64 * 1000.0 / (bench_now_ms() - start)
    })
}
#[no_mangle]
pub unsafe extern "C" fn lb_gpu_fill(s: f64) -> f64 {
    let r = blend(16, s);
    if r < 0.0 {
        r
    } else {
        r * 16.0 * W as f64 * H as f64 / 1e6
    }
}
#[no_mangle]
pub unsafe extern "C" fn lb_gpu_layers(s: f64) -> f64 {
    blend(8, s)
}
#[no_mangle]
pub unsafe extern "C" fn lb_gpu_upload(s: f64) -> f64 {
    if !valid(s) {
        return -1.0;
    }
    GPU.with(|g| {
        if g.borrow().is_none() {
            return -1.0;
        }
        let mut px: Vec<u32> = (0..(W * H) as u32)
            .map(|i| i.wrapping_mul(2654435761))
            .collect();
        let mut t = 0;
        glGenTextures(1, &mut t);
        glBindTexture(TEX, t);
        glTexImage2D(TEX, 0, 0x1908, W, H, 0, 0x1908, 0x1401, std::ptr::null());
        let start = bench_now_ms();
        let mut n = 0usize;
        loop {
            let i = n % px.len();
            px[i] ^= 1;
            glTexSubImage2D(TEX, 0, 0, 0, W, H, 0x1908, 0x1401, px.as_ptr().cast());
            glFinish();
            n += 1;
            if bench_now_ms() >= start + s * 1000.0 {
                break;
            }
        }
        glDeleteTextures(1, &t);
        n as f64 * W as f64 * H as f64 * 4.0 / 1e6 / ((bench_now_ms() - start) / 1000.0)
    })
}
