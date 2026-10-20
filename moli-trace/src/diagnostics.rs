//! Opt-in, subscriber-independent diagnostics for parallel CI failures.
//!
//! Set MOLI_DIAGNOSTICS_DIR to an absolute directory. Each process writes its
//! own file, including CLI children, so Nextest output capture cannot lose the
//! timeline. This is an investigation tool and deliberately changes no budget.

use parking_lot::Mutex;
use std::{
    fmt,
    fs::{File, OpenOptions},
    future::{Future, poll_fn},
    io::Write,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

struct DiagnosticLog {
    file: Mutex<File>,
    started: Instant,
    sequence: AtomicU64,
}

fn log() -> Option<&'static DiagnosticLog> {
    static LOG: OnceLock<Option<DiagnosticLog>> = OnceLock::new();
    LOG.get_or_init(|| {
        let directory = std::env::var_os("MOLI_DIAGNOSTICS_DIR")?;
        let directory = std::path::PathBuf::from(directory);
        let open = || -> std::io::Result<File> {
            std::fs::create_dir_all(&directory)?;
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(directory.join(format!("process-{}.log", std::process::id())))
        };
        match open() {
            Ok(mut file) => {
                let _ = writeln!(
                    file,
                    "process_start unix_us={} pid={} argv={:?} nextest={:?}",
                    unix_us(),
                    std::process::id(),
                    std::env::args_os().collect::<Vec<_>>(),
                    std::env::var_os("NEXTEST_EXECUTION_MODE")
                );
                Some(DiagnosticLog {
                    file: Mutex::new(file),
                    started: Instant::now(),
                    sequence: AtomicU64::new(1),
                })
            }
            Err(error) => {
                eprintln!(
                    "MOLI_DIAGNOSTICS_DIR: cannot open {}: {error}",
                    directory.display()
                );
                None
            }
        }
    })
    .as_ref()
}

fn unix_us() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros()
}

thread_local! {
    // read_link resolves the calling thread, unlike /proc/self which resolves
    // the thread-group leader. Cache it once per thread to avoid hot-path I/O.
    static NATIVE_THREAD_ID: String = std::fs::read_link("/proc/thread-self")
        .ok().and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("{:?}", std::thread::current().id()));
}

pub fn diagnostics_enabled() -> bool {
    log().is_some()
}

/// Writes one complete line without requiring a tracing subscriber.
pub fn diagnostic_event(event: &str, detail: fmt::Arguments<'_>) {
    let Some(log) = log() else { return };
    let thread = std::thread::current();
    let _ = NATIVE_THREAD_ID.try_with(|tid| {
        let mut file = log.file.lock();
        let _ = writeln!(
            file,
            "unix_us={} elapsed_us={} pid={} tid={} thread={:?} seq={} event={} {}",
            unix_us(),
            log.started.elapsed().as_micros(),
            std::process::id(),
            tid,
            thread.name(),
            log.sequence.fetch_add(1, Ordering::Relaxed),
            event,
            detail
        );
    });
}

/// An exit record also appears on cancellation and unwinding. `span_end` only
/// means scope exit; success/error is reported separately by the caller.
pub struct DiagnosticSpan {
    state: Option<(u64, Instant)>,
    label: &'static str,
}

impl DiagnosticSpan {
    pub fn new(label: &'static str, detail: fmt::Arguments<'_>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let state = diagnostics_enabled().then(|| {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            diagnostic_event(
                "span_begin",
                format_args!("span={id} label={label} {detail}"),
            );
            (id, Instant::now())
        });
        Self { state, label }
    }
}

impl Drop for DiagnosticSpan {
    fn drop(&mut self) {
        if let Some((id, started)) = self.state {
            diagnostic_event(
                "span_end",
                format_args!(
                    "span={id} label={} duration_us={}",
                    self.label,
                    started.elapsed().as_micros()
                ),
            );
        }
    }
}

/// Records exactly the polls requested by the original executor. A long poll
/// identifies synchronous work/blocking; time between polls includes async
/// waiting and scheduling and must not be interpreted as CPU starvation alone.
pub fn diagnostic_future<T>(
    label: &'static str,
    future: impl Future<Output = T>,
) -> impl Future<Output = T> {
    // Keep the observer a fixed-size future. Adding another inline async state
    // around the already large navigation future can overflow a 2 MiB test
    // thread stack even when diagnostics are disabled.
    let mut future = Some(Box::pin(future));
    let mut progress: Option<DiagnosticFutureProgress> = None;
    poll_fn(move |context| {
        if diagnostics_enabled() && progress.is_none() {
            progress = Some(DiagnosticFutureProgress {
                span: DiagnosticSpan::new(label, format_args!("")),
                last_return: Instant::now(),
                polls: 0,
            });
        }
        if let Some(progress) = &mut progress {
            progress.polls += 1;
            diagnostic_event(
                "poll_begin",
                format_args!(
                    "span={} label={label} poll={} between_polls_us={}",
                    progress.span.state.map(|(id, _)| id).unwrap_or_default(),
                    progress.polls,
                    progress.last_return.elapsed().as_micros()
                ),
            );
        }
        let started = progress.as_ref().map(|_| Instant::now());
        let result = future
            .as_mut()
            .expect("diagnostic future polled after completion")
            .as_mut()
            .poll(context);
        if let Some(progress) = &mut progress {
            diagnostic_event(
                "poll_end",
                format_args!(
                    "span={} label={label} poll={} poll_us={} ready={}",
                    progress.span.state.map(|(id, _)| id).unwrap_or_default(),
                    progress.polls,
                    started
                        .map(|at| at.elapsed().as_micros())
                        .unwrap_or_default(),
                    result.is_ready()
                ),
            );
            progress.last_return = Instant::now();
        }
        if result.is_ready() {
            drop(future.take());
            drop(progress.take());
        }
        result
    })
}

struct DiagnosticFutureProgress {
    span: DiagnosticSpan,
    last_return: Instant,
    polls: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };

    #[derive(Default)]
    struct Probe {
        polls: AtomicU64,
        wakes: AtomicU64,
        drops: AtomicU64,
    }

    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.wakes.fetch_add(1, Ordering::Relaxed);
        }
    }

    struct Inner(Arc<Probe>);

    impl Future for Inner {
        type Output = u64;
        fn poll(self: std::pin::Pin<&mut Self>, context: &mut Context<'_>) -> Poll<u64> {
            if self.0.polls.fetch_add(1, Ordering::Relaxed) == 0 {
                context.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(42)
            }
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            self.0.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn observer_keeps_large_navigation_state_off_the_callers_stack() {
        let observed = diagnostic_future("large", std::future::ready([0_u8; 16 * 1024]));
        assert!(
            std::mem::size_of_val(&observed) < 1024,
            "observing a large navigation future must not inline its state in the caller"
        );
    }

    #[test]
    fn observation_preserves_pending_wakes_and_result() {
        let probe = Arc::new(Probe::default());
        let waker = Waker::from(probe.clone());
        let mut context = Context::from_waker(&waker);
        let future = diagnostic_future("probe", Inner(probe.clone()));
        let mut future = std::pin::pin!(future);
        assert_eq!(future.as_mut().poll(&mut context), Poll::Pending);
        assert_eq!(probe.polls.load(Ordering::Relaxed), 1);
        assert_eq!(probe.wakes.load(Ordering::Relaxed), 1);
        assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(42));
        assert_eq!(probe.polls.load(Ordering::Relaxed), 2);
        assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn cancellation_drops_pending_operation_without_polling_it_again() {
        let probe = Arc::new(Probe::default());
        let waker = Waker::from(probe.clone());
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(diagnostic_future("cancel", Inner(probe.clone())));
        assert_eq!(future.as_mut().poll(&mut context), Poll::Pending);
        drop(future);
        assert_eq!(probe.polls.load(Ordering::Relaxed), 1);
        assert_eq!(probe.drops.load(Ordering::Relaxed), 1);
    }
}
