//! What the scale tests measure with. The time is the CPU time of the calling thread, not the
//! time on the clock: a busy machine makes a thread wait for a core, and that waiting says nothing
//! about the code, however long it lasts.

use std::time::Duration;

/// The CPU time the calling thread has used so far.
#[cfg(unix)]
pub fn thread_cpu_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` only writes the `timespec` it is given.
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut now) };
    assert_eq!(result, 0, "the thread's CPU time is not available");
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// Where the system has no per-thread CPU time, the clock stands in: the tests still run, they
/// are only as steady as the machine is quiet.
#[cfg(not(unix))]
pub fn thread_cpu_time() -> Duration {
    use std::sync::OnceLock;
    use std::time::Instant;

    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed()
}

/// The CPU time `work` takes.
pub fn cpu_time<T>(work: impl FnOnce() -> T) -> (T, Duration) {
    let start = thread_cpu_time();
    let result = work();
    (result, thread_cpu_time() - start)
}
