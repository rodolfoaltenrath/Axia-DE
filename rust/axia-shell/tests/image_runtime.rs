//! Native ABI/pixel regressions. The reference hashes were recorded from the
//! original C image renderer before deleting it; new expected pixels are never
//! inferred from the implementation under test.
use std::{
    ffi::{c_char, c_void, CString},
    path::{Path, PathBuf},
    process::Command,
};

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
}
#[repr(C)]
struct Canvas {
    px: *mut u32,
    w: i32,
    h: i32,
    stride: i32,
    clip: Rect,
}
type Image = *mut c_void;
struct Api {
    library: *mut c_void,
    get: unsafe extern "C" fn(*const c_char) -> Image,
    rgba: unsafe extern "C" fn(Image, i32, i32, *const u8) -> Image,
    argb: unsafe extern "C" fn(Image, i32, i32, *const u32) -> Image,
    release: unsafe extern "C" fn(Image),
    size: unsafe extern "C" fn(Image, *mut i32, *mut i32),
    bytes: unsafe extern "C" fn() -> u64,
    init: unsafe extern "C" fn(*mut Canvas, *mut u32, i32, i32, i32),
    draw: unsafe extern "C" fn(*mut Canvas, Image, f32, f32, f32, f32, f32, i32, f32),
}
impl Drop for Api {
    fn drop(&mut self) {
        unsafe {
            libc::dlclose(self.library);
        }
    }
}
impl Api {
    unsafe fn load(path: &Path) -> Self {
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let library = libc::dlopen(path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if library.is_null() {
            panic!("dlopen: {:?}", std::ffi::CStr::from_ptr(libc::dlerror()));
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                let p = libc::dlsym(library, concat!($name, "\0").as_ptr().cast());
                assert!(!p.is_null(), "missing {}", $name);
                std::mem::transmute::<*mut c_void, $ty>(p)
            }};
        }
        Self {
            library,
            get: symbol!("image_get", unsafe extern "C" fn(*const c_char) -> Image),
            rgba: symbol!(
                "image_pixels",
                unsafe extern "C" fn(Image, i32, i32, *const u8) -> Image
            ),
            argb: symbol!(
                "image_argb",
                unsafe extern "C" fn(Image, i32, i32, *const u32) -> Image
            ),
            release: symbol!("image_release", unsafe extern "C" fn(Image)),
            size: symbol!(
                "image_size",
                unsafe extern "C" fn(Image, *mut i32, *mut i32)
            ),
            bytes: symbol!("image_managed_bytes", unsafe extern "C" fn() -> u64),
            init: symbol!(
                "canvas_init",
                unsafe extern "C" fn(*mut Canvas, *mut u32, i32, i32, i32)
            ),
            draw: symbol!(
                "image_draw",
                unsafe extern "C" fn(*mut Canvas, Image, f32, f32, f32, f32, f32, i32, f32)
            ),
        }
    }
}
fn command(mut command: Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn build(root: &Path, work: &Path) -> PathBuf {
    let map = work.join("exports.map");
    std::fs::write(&map, "{ global: image_*; canvas_init; local: *; };\n").unwrap();
    let library = work.join("images.so");
    let mut cc = Command::new("cc");
    cc.current_dir(root.join("server"))
        .args([
            "-shared",
            "-fPIC",
            "-O2",
            "-D_GNU_SOURCE",
            "-Isrc",
            "-I../third_party/stb",
            "src/canvas.c",
            "src/ui/stb.c",
            "build/rust/release/libaxia_shell.a",
            "-Wl,--gc-sections",
            "-Wl,-u,image_get",
            "-ldl",
            "-lm",
            "-lpthread",
        ])
        .arg(format!("-Wl,--version-script={}", map.display()))
        .arg("-o")
        .arg(&library);
    command(cc);
    library
}
fn texture(w: usize, h: usize) -> Vec<u8> {
    if w == 1 && h == 1 {
        return vec![25, 117, 211, 255];
    }
    (0..w * h)
        .flat_map(|i| {
            let (x, y) = (i % w, i / w);
            [
                (x * 29 + y * 7) as u8,
                (x * 11 + y * 31) as u8,
                (x * 3 + y * 19) as u8,
                if w == 2 && h == 2 {
                    255
                } else if i % 7 == 0 {
                    0
                } else if i % 3 == 0 {
                    113
                } else {
                    255
                },
            ]
        })
        .collect()
}
fn hash(pixels: &[u32]) -> u64 {
    pixels
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(1099511628211)
        })
}
// Source size, target size, fit, clip, position, radius and opacity all vary.
const CASES: &[(i32, i32, i32, i32, i32, f32, f32)] = &[
    (7, 5, 7, 5, 0, 0., 1.),
    (7, 5, 23, 17, 0, 0., 1.),
    (7, 5, 23, 17, 1, 0., 1.),
    (7, 5, 23, 17, 2, 0., 1.),
    (31, 19, 4, 3, 0, 0., 1.),
    (31, 19, 4, 3, 1, 0., 1.),
    (31, 19, 4, 3, 2, 0., 1.),
    (7, 5, 23, 17, 0, 4.25, 0.53),
    (13, 29, 19, 11, 2, 3., 0.81),
    (13, 29, 19, 11, 1, 3., 0.81),
    (31, 19, 4, 27, 0, 0., 1.),
    (1, 1, 23, 17, 2, 5., 1.),
    (2, 2, 11, 9, 0, 2., 0.67),
];
const GOLDENS: &[u64] = &[
    0x7c696d45ec0c5549,
    0xb8a672f388699a79,
    0xdc67503d18ee906e,
    0x844dbc7fa97eff77,
    0x8497d45691f3dbd5,
    0xdcea06c9500eb32b,
    0x839559330ab0d4fd,
    0x1a8e08f77e9d3dc2,
    0x2ecbb79ccbad8078,
    0x8125c2d6c084129d,
    0x12c28787dfcbdfcd,
    0xc1137432d68022c1,
    0xab0ff88ec41717e8,
];

#[test]
#[ignore = "requires make -C server and a native C toolchain; see docs/RUST.md"]
fn native_image_pixels_decoding_cache_and_abi_preserve_the_renderer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let work = root.join("server/build/image-runtime");
    std::fs::create_dir_all(&work).unwrap();
    let library = build(&root, &work);
    let api = unsafe { Api::load(&library) };
    let mut hashes = Vec::new();
    unsafe {
        for (ix, &(sw, sh, w, h, fit, r, opacity)) in CASES.iter().enumerate() {
            let rgba = texture(sw as usize, sh as usize);
            let image = (api.rgba)(std::ptr::null_mut(), sw, sh, rgba.as_ptr());
            assert!(!image.is_null());
            let (mut width, mut height) = (0, 0);
            (api.size)(image, &mut width, &mut height);
            assert_eq!((width, height), (sw, sh));
            // Guard pixels and a stride larger than the clip catch ABI/layout mistakes.
            let mut pixels = vec![0xff29374bu32; 43 * 37 + 16];
            let mut canvas = std::mem::MaybeUninit::<Canvas>::uninit();
            (api.init)(canvas.as_mut_ptr(), pixels.as_mut_ptr().add(8), 39, 37, 43);
            let mut canvas = canvas.assume_init();
            assert_eq!((canvas.w, canvas.h, canvas.stride), (39, 37, 43));
            canvas.clip = Rect {
                x0: 3,
                y0: 2,
                x1: 35,
                y1: 33,
            };
            let (x, y) = if ix % 3 == 0 && w > 7 {
                (-2.25, 5.5)
            } else if ix % 3 == 1 && h > 7 {
                (6.5, -1.25)
            } else {
                (2.5, 5.5)
            };
            (api.draw)(
                &mut canvas,
                image,
                x,
                y,
                w as f32,
                h as f32,
                r,
                fit,
                opacity,
            );
            hashes.push(hash(&pixels));
            assert_eq!(&pixels[..8], &[0xff29374b; 8]);
            assert_eq!(&pixels[pixels.len() - 8..], &[0xff29374b; 8]);
            // Premultiplied input must have exactly the same pixels.
            let argb: Vec<_> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| {
                    let [r, g, b, a] = [c[0], c[1], c[2], c[3]].map(u32::from);
                    a << 24
                        | ((r * a + 127) / 255) << 16
                        | ((g * a + 127) / 255) << 8
                        | ((b * a + 127) / 255)
                })
                .collect();
            let same = (api.argb)(image, sw, sh, argb.as_ptr());
            assert_eq!(same, image);
            let mut other = vec![0xff29374bu32; 43 * 37 + 16];
            canvas.px = other.as_mut_ptr().add(8);
            (api.draw)(
                &mut canvas,
                image,
                x,
                y,
                w as f32,
                h as f32,
                r,
                fit,
                opacity,
            );
            assert_eq!(pixels, other);
            (api.release)(image);
        }
        assert_eq!(hashes, GOLDENS, "pixels must match the C reference");
        // The original wallpaper lifetime test, now through the opaque ABI.
        let folder = work.join("axia/wallpapers");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("test.png");
        let rgba = texture(16, 16);
        image::save_buffer(&path, &rgba, 16, 16, image::ColorType::Rgba8).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let before = (api.bytes)();
        let a = (api.get)(path.as_ptr());
        let b = (api.get)(path.as_ptr());
        assert!(!a.is_null());
        assert_eq!(a, b);
        let mut pixels = vec![0u32; 32 * 32];
        let mut canvas = std::mem::MaybeUninit::<Canvas>::uninit();
        (api.init)(canvas.as_mut_ptr(), pixels.as_mut_ptr(), 32, 32, 32);
        let mut canvas = canvas.assume_init();
        (api.draw)(&mut canvas, a, 0., 0., 24., 24., 0., 1, 1.);
        (api.draw)(&mut canvas, a, 0., 0., 32., 32., 0., 1, 1.);
        let source = (api.rgba)(std::ptr::null_mut(), 16, 16, rgba.as_ptr());
        assert!(!source.is_null());
        let mut expected = vec![0u32; 32 * 32];
        let mut reference = std::mem::MaybeUninit::<Canvas>::uninit();
        (api.init)(reference.as_mut_ptr(), expected.as_mut_ptr(), 32, 32, 32);
        let mut reference = reference.assume_init();
        (api.draw)(&mut reference, source, 0., 0., 24., 24., 0., 1, 1.);
        (api.draw)(&mut reference, source, 0., 0., 32., 32., 0., 1, 1.);
        assert_eq!(
            pixels, expected,
            "PNG decoding must preserve premultiplied colors"
        );
        (api.release)(source);
        assert!((api.bytes)() >= before + (16 * 16 + 24 * 24 + 32 * 32) * 4);
        (api.release)(a);
        assert!((api.bytes)() > before);
        (api.release)(b);
        assert_eq!((api.bytes)(), before);
        // The retained decoder also accepts JPEG and non-UTF-8 file names.
        use std::os::unix::ffi::OsStringExt;
        let jpeg = folder.join(std::ffi::OsString::from_vec(b"test-\xff.jpg".to_vec()));
        let rgb: Vec<_> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| p[..3].iter().copied())
            .collect();
        image::save_buffer(&jpeg, &rgb, 16, 16, image::ColorType::Rgb8).unwrap();
        let jpeg = CString::new(jpeg.as_os_str().as_bytes()).unwrap();
        let j = (api.get)(jpeg.as_ptr());
        assert!(!j.is_null());
        let (mut jw, mut jh) = (0, 0);
        (api.size)(j, &mut jw, &mut jh);
        assert_eq!((jw, jh), (16, 16));
        (api.release)(j);
        assert_eq!((api.bytes)(), before);
        // Missing and corrupt files do not enter the cache.
        assert!((api.get)(c"/nonexistent/axia/image.png".as_ptr()).is_null());
        std::fs::write(folder.join("broken.png"), b"not a PNG").unwrap();
        let bad = CString::new(folder.join("broken.png").as_os_str().as_bytes()).unwrap();
        assert!((api.get)(bad.as_ptr()).is_null());
        assert_eq!((api.bytes)(), before);
        // Invalid requests cannot consume the prior handle.
        let old = (api.argb)(std::ptr::null_mut(), 1, 1, [0xff123456].as_ptr());
        assert!(!old.is_null());
        assert!((api.rgba)(old, i32::MAX, 1, std::ptr::dangling()).is_null());
        (api.size)(old, &mut jw, &mut jh);
        assert_eq!((jw, jh), (1, 1));
        (api.release)(old);
    }
}
