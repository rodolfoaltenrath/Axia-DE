//! Fixed-size LBN2 records. Version 2 identifies native Rust workloads.
use libc::{c_char, c_int};
pub const LB_MAGIC: u32 = 0x4c424e32;
pub const LB_VERSION: i32 = 2;
pub const LB_MODE_QUICK: i32 = 1;
pub const LB_MODE_FULL: i32 = 2;
pub const LB_MODE_LOAD: i32 = 3;
pub const LB_GO_GPU: i32 = 1;
pub const LB_GO_STORAGE: i32 = 2;
pub const LB_GO_LOW: i32 = 4;
pub const LB_CPU_SHA1: i32 = 0;
pub const LB_CPU_SHAN: i32 = 1;
pub const LB_CPU_MAT1: i32 = 2;
pub const LB_CPU_MATN: i32 = 3;
pub const LB_CPU_XZ1: i32 = 4;
pub const LB_CPU_XZN: i32 = 5;
pub const LB_CPU_DATA1: i32 = 6;
pub const LB_CPU_DATAN: i32 = 7;
pub const LB_CPU_UI1: i32 = 8;
pub const LB_CPU_UIN: i32 = 9;
pub const LB_CPU_SHAPE1: i32 = 10;
pub const LB_CPU_SHAPEN: i32 = 11;
pub const LB_MEM_COPY: i32 = 12;
pub const LB_MEM_TRIAD: i32 = 13;
pub const LB_MEM_LAT: i32 = 14;
pub const LB_GPU_SCENE: i32 = 15;
pub const LB_GPU_FILL: i32 = 16;
pub const LB_GPU_UPLOAD: i32 = 17;
pub const LB_GPU_LAYERS: i32 = 18;
pub const LB_DISK_SEQW: i32 = 19;
pub const LB_DISK_SEQR: i32 = 20;
pub const LB_DISK_RAND: i32 = 21;
pub const LB_SUS_START: i32 = 22;
pub const LB_SUS_END: i32 = 23;
pub const LB_VID_H264_1080: i32 = 24;
pub const LB_VID_HEVC_1080: i32 = 25;
pub const LB_VID_VP9_1080: i32 = 26;
pub const LB_VID_AV1_1080: i32 = 27;
pub const LB_VID_H264_2160: i32 = 28;
pub const LB_VID_HEVC_2160: i32 = 29;
pub const LB_VENC_H264: i32 = 30;
pub const LB_VENC_VP8: i32 = 31;
pub const LB_COMP_FPS: i32 = 32;
pub const LB_COMP_P50: i32 = 33;
pub const LB_COMP_P99: i32 = 34;
pub const LB_COMP_MISSED: i32 = 35;
pub const LB_COMP_IN50: i32 = 36;
pub const LB_COMP_IN99: i32 = 37;
pub const LB_DESK_FPS: i32 = 38;
pub const LB_DESK_P50: i32 = 39;
pub const LB_DESK_P99: i32 = 40;
pub const LB_DESK_MISSED: i32 = 41;
pub const LB_DESK_IN50: i32 = 42;
pub const LB_DESK_IN99: i32 = 43;
pub const LB_DESK_WORK: i32 = 44;
pub const LB_COUNT: i32 = 45;
pub const LB_MSG_STEP: i32 = 1;
pub const LB_MSG_RESULT: i32 = 2;
pub const LB_MSG_SKIP: i32 = 3;
pub const LB_MSG_SAMPLE: i32 = 4;
pub const LB_MSG_DONE: i32 = 5;
pub const LB_MSG_GPUINFO: i32 = 6;
pub const LB_MSG_SANDBOX: i32 = 7;
pub const LB_SBX_SOCKET: i32 = 0;
pub const LB_SBX_INET: i32 = 1;
pub const LB_SBX_EXEC: i32 = 2;
pub const LB_SBX_FORK: i32 = 3;
pub const LB_SBX_NEWFILE: i32 = 4;
pub const LB_SBX_READ: i32 = 5;
pub const LB_SBX_COUNT: i32 = 6;
pub const LB_SKIP_NODEV: i32 = 1;
pub const LB_SKIP_FAILED: i32 = 2;
pub const LB_SKIP_MODE: i32 = 3;
pub const LB_SKIP_MEMORY: i32 = 4;
pub const LB_SKIP_NOCODEC: i32 = 5;
pub const LB_SKIP_NOFRAMES: i32 = 6;
pub const LB_HELPER_LAST: i32 = LB_SUS_END;
pub const LB_VID_FIRST: i32 = LB_VID_H264_1080;
pub const LB_VID_LAST: i32 = LB_VENC_VP8;
pub const LB_DESK_FIRST: i32 = LB_COMP_FPS;
pub const LB_DESK_LAST: i32 = LB_DESK_WORK;
const MEASUREMENTS: [(&[u8], &[u8]); LB_COUNT as usize] = [
    (b"cpu.sha256.1\0", b"MB/s\0"),
    (b"cpu.sha256.n\0", b"MB/s\0"),
    (b"cpu.matmul.1\0", b"GFLOP/s\0"),
    (b"cpu.matmul.n\0", b"GFLOP/s\0"),
    (b"cpu.xz.1\0", b"MB/s\0"),
    (b"cpu.xz.n\0", b"MB/s\0"),
    (b"cpu.data.1\0", b"runs/s\0"),
    (b"cpu.data.n\0", b"runs/s\0"),
    (b"cpu.ui.1\0", b"fps\0"),
    (b"cpu.ui.n\0", b"fps\0"),
    (b"cpu.shape.1\0", b"runs/s\0"),
    (b"cpu.shape.n\0", b"runs/s\0"),
    (b"mem.copy\0", b"GB/s\0"),
    (b"mem.triad\0", b"GB/s\0"),
    (b"mem.latency\0", b"ns\0"),
    (b"gpu.scene\0", b"fps\0"),
    (b"gpu.fill\0", b"Mpx/s\0"),
    (b"gpu.upload\0", b"MB/s\0"),
    (b"gpu.layers\0", b"fps\0"),
    (b"disk.seqwrite\0", b"MB/s\0"),
    (b"disk.seqread\0", b"MB/s\0"),
    (b"disk.random4k\0", b"IOPS\0"),
    (b"sustained.start\0", b"GFLOP/s\0"),
    (b"sustained.end\0", b"GFLOP/s\0"),
    (b"video.h264.1080\0", b"fps\0"),
    (b"video.hevc.1080\0", b"fps\0"),
    (b"video.vp9.1080\0", b"fps\0"),
    (b"video.av1.1080\0", b"fps\0"),
    (b"video.h264.2160\0", b"fps\0"),
    (b"video.hevc.2160\0", b"fps\0"),
    (b"video.enc.h264\0", b"fps\0"),
    (b"video.enc.vp8\0", b"fps\0"),
    (b"comp.fps\0", b"fps\0"),
    (b"comp.p50\0", b"ms\0"),
    (b"comp.p99\0", b"ms\0"),
    (b"comp.missed\0", b"%\0"),
    (b"comp.in50\0", b"ms\0"),
    (b"comp.in99\0", b"ms\0"),
    (b"desk.fps\0", b"fps\0"),
    (b"desk.p50\0", b"ms\0"),
    (b"desk.p99\0", b"ms\0"),
    (b"desk.missed\0", b"%\0"),
    (b"desk.in50\0", b"ms\0"),
    (b"desk.in99\0", b"ms\0"),
    (b"desk.work\0", b"GFLOP/s\0"),
];
const SANDBOX: [&[u8]; LB_SBX_COUNT as usize] = [
    b"helperSocket\0",
    b"helperInet\0",
    b"helperExec\0",
    b"helperFork\0",
    b"helperNewFile\0",
    b"helperRead\0",
];
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LbGo {
    pub magic: u32,
    pub mode: u32,
    pub flags: u32,
    pub threads: u32,
    pub storage_mb: u32,
    pub sustained_s: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LbMsg {
    pub magic: u32,
    pub kind: u32,
    pub id: u32,
    pub pad: u32,
    pub value: f64,
    pub aux: f64,
}
#[no_mangle]
pub extern "C" fn lb_name(id: c_int) -> *const c_char {
    MEASUREMENTS
        .get(id as usize)
        .map(|v| v.0.as_ptr().cast())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn lb_unit(id: c_int) -> *const c_char {
    MEASUREMENTS
        .get(id as usize)
        .map(|v| v.1.as_ptr().cast())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn lb_sbx_name(id: c_int) -> *const c_char {
    SANDBOX
        .get(id as usize)
        .map(|v| v.as_ptr().cast())
        .unwrap_or(std::ptr::null())
}
#[no_mangle]
pub extern "C" fn lb_lower_better(id: c_int) -> c_int {
    matches!(
        id,
        LB_MEM_LAT
            | LB_COMP_P50
            | LB_COMP_P99
            | LB_COMP_MISSED
            | LB_COMP_IN50
            | LB_COMP_IN99
            | LB_DESK_P50
            | LB_DESK_P99
            | LB_DESK_MISSED
            | LB_DESK_IN50
            | LB_DESK_IN99
    ) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn lb_msg_valid(m: *const LbMsg) -> c_int {
    let Some(m) = m.as_ref() else {
        return 0;
    };
    if m.magic != LB_MAGIC || !m.value.is_finite() || !m.aux.is_finite() {
        return 0;
    }
    let helper = m.id <= LB_HELPER_LAST as u32;
    (match m.kind as i32 {
        LB_MSG_STEP => helper,
        LB_MSG_RESULT => helper && m.value > 0. && m.value < 1e12 && m.aux >= 0. && m.aux < 1e12,
        LB_MSG_SKIP => helper && m.aux >= LB_SKIP_NODEV as f64 && m.aux <= LB_SKIP_MEMORY as f64,
        LB_MSG_SAMPLE => m.value >= 0. && m.value < 1e9 && m.aux >= 0. && m.aux < 86400.,
        LB_MSG_DONE => m.aux >= 0. && m.aux < 65536.,
        LB_MSG_GPUINFO => matches!(m.value, 0. | 1. | 2.),
        LB_MSG_SANDBOX => {
            m.id < LB_SBX_COUNT as u32
                && matches!(m.value, 0. | 1.)
                && m.aux >= 0.
                && m.aux < 4096.
                && m.aux == m.aux.trunc()
        }
        _ => false,
    }) as c_int
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_keep_their_c_wire_layout() {
        assert_eq!(std::mem::size_of::<LbGo>(), 24);
        assert_eq!(std::mem::size_of::<LbMsg>(), 32);
        assert_eq!(std::mem::offset_of!(LbMsg, value), 16);
        assert_eq!(std::mem::align_of::<LbMsg>(), 8);
    }
    #[test]
    fn helper_cannot_forge_server_or_media_measurements() {
        unsafe {
            for id in 0..LB_COUNT {
                let msg = LbMsg {
                    magic: LB_MAGIC,
                    kind: LB_MSG_RESULT as u32,
                    id: id as u32,
                    value: 1200.,
                    aux: 15.,
                    ..Default::default()
                };
                assert_eq!(lb_msg_valid(&msg), if id <= LB_HELPER_LAST { 1 } else { 0 });
                assert!(!lb_name(id).is_null());
                assert!(!lb_unit(id).is_null());
                for other in 0..id {
                    assert_ne!(
                        std::ffi::CStr::from_ptr(lb_name(id)),
                        std::ffi::CStr::from_ptr(lb_name(other))
                    );
                }
            }
            assert!(lb_name(-1).is_null());
            assert!(lb_name(LB_COUNT).is_null());
        }
    }
    #[test]
    fn nonfinite_and_out_of_range_records_are_rejected() {
        unsafe {
            let good = LbMsg {
                magic: LB_MAGIC,
                kind: LB_MSG_RESULT as u32,
                id: 0,
                value: 1200.,
                aux: 15.,
                ..Default::default()
            };
            assert_eq!(lb_msg_valid(&good), 1);
            for value in [f64::NAN, f64::INFINITY, -5., 0., 1e12] {
                assert_eq!(lb_msg_valid(&LbMsg { value, ..good }), 0);
            }
            for aux in [f64::NAN, f64::INFINITY, -1., 1e12] {
                assert_eq!(lb_msg_valid(&LbMsg { aux, ..good }), 0);
            }
            assert_eq!(lb_msg_valid(&LbMsg { magic: 1, ..good }), 0);
            assert_eq!(lb_msg_valid(&LbMsg { kind: 77, ..good }), 0);
            let skip = LbMsg {
                kind: LB_MSG_SKIP as u32,
                id: LB_GPU_SCENE as u32,
                aux: LB_SKIP_NODEV as f64,
                value: 0.,
                ..good
            };
            assert_eq!(lb_msg_valid(&skip), 1);
            for aux in [99., LB_SKIP_NOCODEC as f64] {
                assert_eq!(lb_msg_valid(&LbMsg { aux, ..skip }), 0);
            }
        }
    }
    #[test]
    fn sandbox_outcomes_are_binary_and_errno_is_integral() {
        unsafe {
            let m = LbMsg {
                magic: LB_MAGIC,
                kind: LB_MSG_SANDBOX as u32,
                id: LB_SBX_FORK as u32,
                value: 1.,
                aux: 13.,
                ..Default::default()
            };
            assert_eq!(lb_msg_valid(&m), 1);
            for aux in [-1., 1.5, 4096., f64::INFINITY] {
                assert_eq!(lb_msg_valid(&LbMsg { aux, ..m }), 0);
            }
            for value in [0.5, 2.] {
                assert_eq!(lb_msg_valid(&LbMsg { value, ..m }), 0);
            }
            assert_eq!(
                lb_msg_valid(&LbMsg {
                    id: LB_SBX_COUNT as u32,
                    ..m
                }),
                0
            );
        }
    }
}
