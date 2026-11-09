//! Regression test for https://github.com/smol-rs/blocking/issues/83
//! (same root cause as #81): idle threads never exit when the pool is
//! under the thread limit, waking every 500 ms forever.
//!
//! Linux-only: it counts the pool's threads through `/proc/self/task`
//! by their `blocking-*` names, which has no portable equivalent.

#![cfg(target_os = "linux")]

use std::fs;
use std::thread;
use std::time::{Duration, Instant};

use blocking::unblock;
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

/// Polls `blocking_thread_count()` until it equals `expected`, or the
/// timeout elapses. Returns the last observed count.
fn wait_for_count(expected: usize, timeout: Duration) -> usize {
    let start = Instant::now();
    loop {
        let count = blocking_thread_count();
        if count == expected || start.elapsed() >= timeout {
            return count;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn idle_threads_exit_when_under_thread_limit() {
    // No pool threads before any work is submitted.
    assert_eq!(blocking_thread_count(), 0);

    // Run one trivial task so the pool spawns a thread.
    future::block_on(async {
        assert_eq!(unblock(|| 42).await, 42);
    });

    // The pool must have spawned a thread to run the task.
    assert_eq!(wait_for_count(1, Duration::from_secs(2)), 1);

    // Once idle, the thread must exit after the 500 ms idle timeout even
    // though the pool is far below the default 500-thread limit. Before
    // the fix, the count stays at 1 forever: the exit condition also
    // required the pool to be over the thread limit.
    assert_eq!(
        wait_for_count(0, Duration::from_secs(10)),
        0,
        "idle blocking thread did not exit within 10 s (see #83)"
    );
}
