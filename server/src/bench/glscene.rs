//! GLES2 scene shared by the windowed testbench and the offscreen benchmark.
#![allow(non_snake_case)]
#![allow(dead_code)] // Offscreen declarations are unused by the windowed-only build.
use super::{bench_gl_kind, bench_now_ms};
use libc::{c_char, c_void};
use std::cell::RefCell;
#[cfg_attr(feature = "gpu", link(name = "GLESv2"))]
extern "C" {
    pub(crate) fn glCreateShader(t: u32) -> u32;
    pub(crate) fn glShaderSource(s: u32, n: i32, p: *const *const c_char, len: *const i32);
    pub(crate) fn glCompileShader(s: u32);
    pub(crate) fn glGetShaderiv(s: u32, k: u32, v: *mut i32);
    pub(crate) fn glDeleteShader(s: u32);
    pub(crate) fn glCreateProgram() -> u32;
    pub(crate) fn glAttachShader(p: u32, s: u32);
    pub(crate) fn glLinkProgram(p: u32);
    pub(crate) fn glGetProgramiv(p: u32, k: u32, v: *mut i32);
    pub(crate) fn glDeleteProgram(p: u32);
    pub(crate) fn glGetUniformLocation(p: u32, k: *const c_char) -> i32;
    pub(crate) fn glGetAttribLocation(p: u32, k: *const c_char) -> i32;
    pub(crate) fn glGetString(k: u32) -> *const u8;
    pub(crate) fn glEnable(k: u32);
    pub(crate) fn glDisable(k: u32);
    pub(crate) fn glViewport(x: i32, y: i32, w: i32, h: i32);
    pub(crate) fn glClearColor(r: f32, g: f32, b: f32, a: f32);
    pub(crate) fn glClear(k: u32);
    pub(crate) fn glUseProgram(p: u32);
    pub(crate) fn glUniformMatrix4fv(k: i32, n: i32, transpose: u8, v: *const f32);
    pub(crate) fn glVertexAttribPointer(
        k: u32,
        n: i32,
        t: u32,
        normalized: u8,
        stride: i32,
        p: *const c_void,
    );
    pub(crate) fn glEnableVertexAttribArray(k: u32);
    pub(crate) fn glDrawArrays(t: u32, first: i32, n: i32);
    pub(crate) fn glFinish();
    pub(crate) fn glGenFramebuffers(n: i32, p: *mut u32);
    pub(crate) fn glBindFramebuffer(t: u32, p: u32);
    pub(crate) fn glGenTextures(n: i32, p: *mut u32);
    pub(crate) fn glBindTexture(t: u32, p: u32);
    pub(crate) fn glTexImage2D(
        t: u32,
        level: i32,
        fmt: i32,
        w: i32,
        h: i32,
        border: i32,
        f: u32,
        ty: u32,
        p: *const c_void,
    );
    pub(crate) fn glTexParameteri(t: u32, k: u32, v: i32);
    pub(crate) fn glFramebufferTexture2D(t: u32, a: u32, tt: u32, p: u32, l: i32);
    pub(crate) fn glGenRenderbuffers(n: i32, p: *mut u32);
    pub(crate) fn glBindRenderbuffer(t: u32, p: u32);
    pub(crate) fn glRenderbufferStorage(t: u32, f: u32, w: i32, h: i32);
    pub(crate) fn glFramebufferRenderbuffer(t: u32, a: u32, tt: u32, p: u32);
    pub(crate) fn glCheckFramebufferStatus(t: u32) -> u32;
    pub(crate) fn glActiveTexture(t: u32);
    pub(crate) fn glUniform1i(k: i32, v: i32);
    pub(crate) fn glUniform1f(k: i32, v: f32);
    pub(crate) fn glBlendFunc(s: u32, d: u32);
    pub(crate) fn glGetError() -> u32;
    pub(crate) fn glTexSubImage2D(
        t: u32,
        l: i32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        f: u32,
        ty: u32,
        p: *const c_void,
    );
    pub(crate) fn glDeleteTextures(n: i32, p: *const u32);
    pub(crate) fn glDeleteFramebuffers(n: i32, p: *const u32);
    pub(crate) fn glDeleteRenderbuffers(n: i32, p: *const u32);
}
pub(crate) unsafe fn program(v: &std::ffi::CStr, f: &std::ffi::CStr) -> Option<u32> {
    unsafe fn shader(t: u32, src: &std::ffi::CStr) -> Option<u32> {
        let s = glCreateShader(t);
        let p = src.as_ptr();
        glShaderSource(s, 1, &p, std::ptr::null());
        glCompileShader(s);
        let mut ok = 0;
        glGetShaderiv(s, 0x8b81, &mut ok);
        if ok == 0 {
            glDeleteShader(s);
            None
        } else {
            Some(s)
        }
    }
    let v = shader(0x8b31, v)?;
    let Some(f) = shader(0x8b30, f) else {
        glDeleteShader(v);
        return None;
    };
    let p = glCreateProgram();
    glAttachShader(p, v);
    glAttachShader(p, f);
    glLinkProgram(p);
    glDeleteShader(v);
    glDeleteShader(f);
    let mut ok = 0;
    glGetProgramiv(p, 0x8b82, &mut ok);
    if ok == 0 {
        glDeleteProgram(p);
        None
    } else {
        Some(p)
    }
}
#[repr(C)]
pub struct BenchGlInfo {
    pub renderer: [c_char; 96],
    pub vendor: [c_char; 64],
    pub version: [c_char; 64],
    pub kind: [c_char; 32],
}
impl Default for BenchGlInfo {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}
#[derive(Default)]
struct Scene {
    program: u32,
    mvp: i32,
    pos: i32,
    col: i32,
}
thread_local! {static SCENE:RefCell<Scene>=RefCell::new(Scene::default());}
#[no_mangle]
pub unsafe extern "C" fn bench_scene_init(info: *mut BenchGlInfo) -> i32 {
    let Some(info) = info.as_mut() else { return -1 };
    *info = BenchGlInfo::default();
    unsafe fn copy(out: &mut [c_char], p: *const u8, max: usize) {
        if !p.is_null() {
            for (o, b) in out
                .iter_mut()
                .take(max)
                .zip(std::ffi::CStr::from_ptr(p.cast()).to_bytes())
            {
                *o = *b as c_char
            }
        }
    }
    copy(&mut info.renderer, glGetString(0x1f01), 90);
    copy(&mut info.vendor, glGetString(0x1f00), 60);
    copy(&mut info.version, glGetString(0x1f02), 60);
    copy(
        &mut info.kind,
        bench_gl_kind(info.renderer.as_ptr()).cast(),
        31,
    );
    bench_scene_free();
    let Some(p)=program(c"uniform mat4 mvp; attribute vec3 pos; attribute vec3 col; varying vec3 c;void main(){ c = col; gl_Position = mvp * vec4(pos, 1.0); }",c"precision mediump float; varying vec3 c; void main(){ gl_FragColor = vec4(c, 1.0); }")else{return -1};
    SCENE.with(|s| {
        *s.borrow_mut() = Scene {
            program: p,
            mvp: glGetUniformLocation(p, c"mvp".as_ptr()),
            pos: glGetAttribLocation(p, c"pos".as_ptr()),
            col: glGetAttribLocation(p, c"col".as_ptr()),
        }
    });
    glEnable(0x0b71);
    0
}
fn mul(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut r = [0.0; 16];
    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                r[j * 4 + i] += a[k * 4 + i] * b[j * 4 + k]
            }
        }
    }
    r
}
fn matrix(w: i32, h: i32, t: f64) -> [f32; 16] {
    let a = t / 1000.0;
    let (ca, sa, cb, sb) = (
        a.cos() as f32,
        a.sin() as f32,
        (a * 0.7).cos() as f32,
        (a * 0.7).sin() as f32,
    );
    let ry = [
        ca, 0.0, -sa, 0.0, 0.0, 1.0, 0.0, 0.0, sa, 0.0, ca, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let rx = [
        1.0, 0.0, 0.0, 0.0, 0.0, cb, sb, 0.0, 0.0, -sb, cb, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let asp = if h != 0 { w as f32 / h as f32 } else { 1.0 };
    let f = 1.0 / 0.5f64.tan() as f32;
    let pr = [
        f / asp,
        0.0,
        0.0,
        0.0,
        0.0,
        f,
        0.0,
        0.0,
        0.0,
        0.0,
        21.0 / -19.0,
        -1.0,
        0.0,
        0.0,
        40.0 / -19.0,
        0.0,
    ];
    let tr = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, -5.0, 1.0,
    ];
    mul(pr, mul(tr, mul(rx, ry)))
}
#[no_mangle]
pub unsafe extern "C" fn bench_scene_draw(w: i32, h: i32, t: f64) -> f64 {
    SCENE.with(|s| {
        let s = s.borrow();
        if s.program == 0 {
            return 0.0;
        }
        let p = [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ];
        let faces = [
            [0, 1, 2, 3],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [1, 2, 6, 5],
            [0, 3, 7, 4],
        ];
        let colors = [
            [0.95, 0.25, 0.25],
            [0.25, 0.85, 0.35],
            [0.25, 0.45, 0.95],
            [0.95, 0.8, 0.2],
            [0.75, 0.35, 0.95],
            [0.2, 0.85, 0.9],
        ];
        let mut v = [0f32; 108];
        let mut c = [0f32; 108];
        let mut k = 0;
        for f in 0..6 {
            for i in [0, 1, 2, 0, 2, 3] {
                v[k..k + 3].copy_from_slice(&p[faces[f][i]]);
                c[k..k + 3].copy_from_slice(&colors[f]);
                k += 3
            }
        }
        let m = matrix(w, h, t);
        let start = bench_now_ms();
        glViewport(0, 0, w, h);
        glClearColor(0.07, 0.07, 0.1, 1.0);
        glClear(0x4100);
        glUseProgram(s.program);
        glUniformMatrix4fv(s.mvp, 1, 0, m.as_ptr());
        glVertexAttribPointer(s.pos as u32, 3, 0x1406, 0, 0, v.as_ptr().cast());
        glVertexAttribPointer(s.col as u32, 3, 0x1406, 0, 0, c.as_ptr().cast());
        glEnableVertexAttribArray(s.pos as u32);
        glEnableVertexAttribArray(s.col as u32);
        glDrawArrays(4, 0, 36);
        glFinish();
        bench_now_ms() - start
    })
}
#[no_mangle]
pub unsafe extern "C" fn bench_scene_free() {
    SCENE.with(|s| {
        let mut s = s.borrow_mut();
        if s.program != 0 {
            glDeleteProgram(s.program)
        }
        *s = Scene::default()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projection_and_rotation_are_finite() {
        for t in [0.0, 1000.0, 1e9] {
            let m = matrix(1920, 1080, t);
            assert!(m.iter().all(|v| v.is_finite()));
        }
        let m = matrix(100, 100, 0.0);
        assert_eq!(m[15], 5.0);
        assert_eq!(m[11], -1.0);
    }
}
