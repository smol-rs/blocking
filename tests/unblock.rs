#![allow(clippy::needless_range_loop)]

use std::io::{Cursor, SeekFrom};
use std::panic::AssertUnwindSafe;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use blocking::{unblock, Unblock};
use futures_lite::{future, pin, prelude::*};

#[test]
fn sleep() {
    let dur = Duration::from_secs(1);
    let start = Instant::now();

    future::block_on(async {
        let f = unblock(move || thread::sleep(dur));
        pin!(f);
        assert!(future::poll_once(&mut f).await.is_none());
        f.await;
    });

    assert!(start.elapsed() >= dur);
}

#[test]
fn chan() {
    const N: i32 = if cfg!(miri) { 50 } else { 100_000 };

    future::block_on(async {
        let (s, r) = mpsc::sync_channel::<i32>(100);
        let handle = thread::spawn(move || {
            for i in 0..N {
                s.send(i).unwrap();
            }
        });

        let mut r = Unblock::new(r.into_iter());
        for i in 0..N {
            assert_eq!(r.next().await, Some(i));
        }

        handle.join().unwrap();
        assert!(r.next().await.is_none());
    })
}

#[test]
fn read() {
    const N: usize = if cfg!(miri) { 20_000 } else { 20_000_000 };

    future::block_on(async {
        let mut v1 = vec![0u8; N];
        for i in 0..v1.len() {
            v1[i] = i as u8;
        }
        let mut v1 = Unblock::new(Cursor::new(v1));

        let mut v2 = vec![];
        v1.read_to_end(&mut v2).await.unwrap();

        let v1 = v1.into_inner().await.into_inner();
        assert!(v1 == v2);
    })
}

#[test]
fn write() {
    const N: usize = if cfg!(miri) { 20_000 } else { 20_000_000 };

    future::block_on(async {
        let mut v1 = vec![0u8; N];
        for i in 0..v1.len() {
            v1[i] = i as u8;
        }

        let v2 = vec![];
        let mut v2 = Unblock::new(Cursor::new(v2));
        v2.write_all(&v1).await.unwrap();

        let v2 = v2.into_inner().await.into_inner();
        assert!(v1 == v2);
    })
}

#[test]
fn seek() {
    future::block_on(async {
        let len = 1_000;
        let mut v = vec![0u8; len];
        for i in 0..len {
            v[i] = i as u8;
        }
        let mut v = Unblock::new(Cursor::new(v));

        assert_eq!(v.seek(SeekFrom::Current(7i64)).await.unwrap(), 7);
        assert_eq!(v.seek(SeekFrom::Current(8i64)).await.unwrap(), 15);

        let mut byte = [0u8];
        v.read(&mut byte).await.unwrap();
        assert_eq!(byte[0], 15);
    })
}

#[test]
fn panic() {
    future::block_on(async {
        let x = unblock(|| panic!("expected failure"));
        let panic = x.catch_unwind().await.unwrap_err();

        // Make sure it's our panic and not an unrelated one.
        let msg = if let Some(msg) = panic.downcast_ref::<&'static str>() {
            msg.to_string()
        } else {
            *panic.downcast::<String>().unwrap()
        };
        assert_eq!(msg, "expected failure");
    });
}

#[test]
fn panic_with_mut() {
    future::block_on(async {
        let mut io = Unblock::new(());
        let x = io.with_mut(|()| panic!("expected failure"));
        let panic = AssertUnwindSafe(x).catch_unwind().await.unwrap_err();

        // Make sure it's our panic and not an unrelated one.
        let msg = if let Some(msg) = panic.downcast_ref::<&'static str>() {
            msg.to_string()
        } else {
            *panic.downcast::<String>().unwrap()
        };
        assert_eq!(
            msg,
            "`Unblock::with_mut()` operation has panicked: RecvError"
        );
    });
}

#[test]
fn write_reports_background_error() {
    struct BrokenWriter;

    impl std::io::Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "background write failed",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    future::block_on(async {
        let mut writer = Unblock::with_capacity(1, BrokenWriter);
        let error = writer.write_all(b"ab").await.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(error.to_string(), "background write failed");
    });
}

#[test]
fn write_can_resume_after_background_error() {
    struct RecoveringWriter {
        fail: bool,
        bytes: Vec<u8>,
    }

    impl std::io::Write for RecoveringWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if self.fail {
                self.fail = false;
                return Err(std::io::ErrorKind::PermissionDenied.into());
            }
            self.bytes.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    future::block_on(async {
        let mut writer = Unblock::with_capacity(
            1,
            RecoveringWriter {
                fail: true,
                bytes: Vec::new(),
            },
        );
        assert_eq!(writer.write(&[]).await.unwrap(), 0);
        assert_eq!(
            writer.write_all(b"ab").await.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
        assert_eq!(writer.write(&[]).await.unwrap(), 0);
        writer.write_all(b"xy").await.unwrap();
        writer.flush().await.unwrap();
        assert_eq!(writer.into_inner().await.bytes, b"xy");
    });
}
