pub fn now() -> i64 {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &mut t) };
    t.tv_sec * 1000 + t.tv_nsec / 1_000_000
}
pub fn local(ms: i64) -> libc::tm {
    let sec = ms.div_euclid(1000) as libc::time_t;
    let mut tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&sec, &mut tm) };
    tm
}
