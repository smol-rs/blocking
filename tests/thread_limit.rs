//! Test that lowering the thread limit with [`set_max_blocking_threads`]
//! promptly retires excess idle threads (the behavior #75 introduced),
//! while the remaining threads still exit on the idle timeout.
//!
//! Linux-only: it counts the pool's threads through `/proc/self/task`
//! by their `blocking-*` names, which has no portable equivalent.

#![cfg(target_os = "linux")]

use std::fs;
use std::num::NonZeroUsize;
use std::thread;
use std::time::{Duration, Instant};

use blocking::{set_max_blocking_threads, unblock};
use futures_lite::future;

/// Counts threads of this process whose name starts with `blocking-`.
fn blocking_thread_count() -> usize {
    let mut count = 0;
    if let Ok(tasks) = fs::read_dir("/proc/self/task") {
        for task in tasks.flatten() {
            if let Ok(comm) = fs::read_to_string(task.path().join("comm")) {
                if comm.trim_start().starts_with("blocking-") {
                    count += 1;
                }
            }
        }
    }
    count
}

/// Polls `blocking_thread_count()` until it is at most `max`, or the
/// timeout elapses. Returns the last observed count.
fn wait_for_count_at_most(max: usize, timeout: Duration) -> usize {
    let start = Instant::now();
    loop {
        let count = blocking_thread_count();
        if count <= max || start.elapsed() >= timeout {
            return count;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn lowering_thread_limit_retires_excess_idle_threads() {
    // No pool threads before any work is submitted.
    assert_eq!(blocking_thread_count(), 0);

    // Grow the pool with many concurrent sleeping tasks.
    const N: usize = 40;
    future::block_on(async {
        let mut tasks = Vec::with_capacity(N);
        for _ in 0..N {
            tasks.push(unblock(|| thread::sleep(Duration::from_millis(300))));
        }
        for task in tasks {
            task.await;
        }
    });

    // Several pool threads must have been spawned.
    let grown = blocking_thread_count();
    assert!(grown >= 2, "pool did not grow: only {grown} thread(s)");

    // Lower the limit to 1. Excess idle threads are woken up and must
    // exit promptly, without waiting out the 500 ms idle timeout: every
    // thread went idle within the last task duration (300 ms), so the
    // earliest idle timeout cannot fire before 200 ms from now, and the
    // most recently idled thread not before 500 ms from now. Reaching at
    // most one thread within 400 ms therefore requires the over-limit
    // exit path, not the timeout path.
    set_max_blocking_threads(NonZeroUsize::new(1).unwrap());
    assert!(
        wait_for_count_at_most(1, Duration::from_millis(400)) <= 1,
        "excess idle threads did not exit promptly after lowering the limit"
    );

    // The last remaining thread is within the new limit, so it exits on
    // the idle timeout instead.
    assert_eq!(
        wait_for_count_at_most(0, Duration::from_secs(10)),
        0,
        "last idle blocking thread did not exit within 10 s"
    );
}
