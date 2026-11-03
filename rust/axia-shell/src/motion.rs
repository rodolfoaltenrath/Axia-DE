//! Conservative playback budgets for animated backgrounds. No dedicated GPU required.
#[repr(C)]
pub struct MotionProfile {
    pub codecs: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub threads: u32,
    pub rate: u32,
    pub hardware: u32,
}
#[no_mangle]
pub unsafe extern "C" fn axia_motion_profile(
    gpu: bool,
    h264: bool,
    hevc: bool,
    out: *mut MotionProfile,
) {
    if out.is_null() {
        return;
    }
    let accelerated = gpu && (h264 || hevc);
    out.write(if accelerated {
        MotionProfile {
            codecs: (if h264 { 1 } else { 0 }) | (if hevc { 2 } else { 0 }),
            width: 1920,
            height: 1080,
            fps: 30,
            threads: 2,
            rate: 100,
            hardware: 1,
        }
    } else {
        MotionProfile {
            codecs: 1,
            width: 1280,
            height: 720,
            fps: 15,
            threads: 2,
            rate: 50,
            hardware: h264 as u32,
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_laptop_and_unknown_hardware_get_a_bounded_profile() {
        for (gpu, a, b) in [
            (false, false, false),
            (false, true, true),
            (true, false, false),
        ] {
            let mut p = std::mem::MaybeUninit::uninit();
            unsafe {
                axia_motion_profile(gpu, a, b, p.as_mut_ptr());
                let p = p.assume_init();
                assert_eq!(
                    (p.codecs, p.width, p.height, p.fps, p.threads, p.rate),
                    (1, 1280, 720, 15, 2, 50)
                );
            }
        }
    }
    #[test]
    fn acceleration_preserves_supported_formats() {
        let mut p = std::mem::MaybeUninit::uninit();
        unsafe {
            axia_motion_profile(true, false, true, p.as_mut_ptr());
            let p = p.assume_init();
            assert_eq!(p.codecs, 2);
            assert_eq!(p.fps, 30);
        }
    }
}
