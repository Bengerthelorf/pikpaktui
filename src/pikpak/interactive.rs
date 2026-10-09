//! Per-worker read deadlines. Transfers retain their normal long timeout.
use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

thread_local! {
    static DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

pub(crate) fn with_deadline<T>(timeout: Duration, work: impl FnOnce() -> T) -> T {
    struct Restore(Option<Instant>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DEADLINE.set(self.0);
        }
    }
    let end = Instant::now() + timeout;
    let previous = DEADLINE.replace(Some(DEADLINE.get().map_or(end, |old| old.min(end))));
    let _restore = Restore(previous);
    work()
}

pub(crate) fn remaining() -> Option<Duration> {
    DEADLINE.get().map(|end| {
        end.saturating_duration_since(Instant::now())
            .max(Duration::from_millis(1))
    })
}

pub(crate) fn bound(
    request: reqwest::blocking::RequestBuilder,
) -> reqwest::blocking::RequestBuilder {
    match remaining() {
        Some(timeout) => request.timeout(timeout),
        None => request,
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> anyhow::Result<MutexGuard<'_, T>> {
    if DEADLINE.get().is_none() {
        return Ok(mutex.lock().unwrap_or_else(|e| e.into_inner()));
    }
    loop {
        match mutex.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => {
                anyhow::ensure!(
                    DEADLINE.get().is_none_or(|end| Instant::now() < end),
                    "preview timed out waiting for authentication"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deadlines_restore_on_nested_scope_and_unwind() {
        assert!(remaining().is_none());
        with_deadline(Duration::from_secs(1), || {
            let outer = DEADLINE.get();
            with_deadline(Duration::from_secs(10), || {
                assert_eq!(DEADLINE.get(), outer)
            });
            assert_eq!(DEADLINE.get(), outer);
        });
        let _ =
            std::panic::catch_unwind(|| with_deadline(Duration::from_secs(1), || panic!("test")));
        assert!(remaining().is_none());
    }
    #[test]
    fn stalled_http_read_and_auth_lock_obey_worker_deadline() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0; 1024];
            let _ = socket.read(&mut buffer);
            std::thread::sleep(Duration::from_millis(200));
        });
        let http = reqwest::blocking::Client::new();
        let started = Instant::now();
        let result = with_deadline(Duration::from_millis(40), || bound(http.get(url)).send());
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
        server.join().unwrap();
        let mutex = Mutex::new(());
        let _held = mutex.lock().unwrap();
        assert!(with_deadline(Duration::from_millis(20), || lock(&mutex)).is_err());
    }
}
