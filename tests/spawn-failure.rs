#[cfg(all(target_pointer_width = "64", not(target_family = "wasm"), not(miri)))]
fn main() {
    use std::process::Command;
    use std::time::{Duration, Instant};

    const CHILD_ENV: &str = "BLOCKING_SPAWN_FAILURE_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        assert!(std::thread::Builder::new().spawn(|| {}).is_err());
        drop(blocking::unblock(|| ()));
        return;
    }

    // A stack this large cannot fit in the process address space. Isolate the
    // environment override in a child so other tests can still create threads.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .env(CHILD_ENV, "1")
        .env("RUST_MIN_STACK", (1usize << 60).to_string())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "spawn-failure child failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("scheduling a task did not return after thread creation failed");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(not(all(target_pointer_width = "64", not(target_family = "wasm"), not(miri))))]
fn main() {}
