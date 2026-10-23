//! One host-owned wakeable worker; composing a runtime never starts AI work.
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tuck_core::{ports::Result, AppError, MeetingService};

const TICK: Duration = Duration::from_secs(5);

trait Application: Send + Sync {
    fn tick(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
}
impl Application for MeetingService {
    fn tick(&self) -> Result<()> {
        self.coach_tick()
    }
    fn stop(&self) -> Result<()> {
        self.stop_live_coaching()
    }
}

struct Worker {
    application: Weak<dyn Application>,
    stop: mpsc::Sender<()>,
    stopping: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

#[derive(Default)]
pub struct CoachWorker {
    worker: Mutex<Option<Worker>>,
}

impl CoachWorker {
    /// Only the desktop host starts this worker. Core pacing/consent remain authoritative.
    pub fn start(&self, service: Arc<MeetingService>) -> Result<()> {
        self.start_application(service, TICK)
    }

    fn start_application(&self, service: Arc<dyn Application>, interval: Duration) -> Result<()> {
        let mut owned = self
            .worker
            .lock()
            .map_err(|_| AppError::unavailable("The live coach worker is unavailable."))?;
        if owned.is_some() {
            return Ok(());
        }
        let application = Arc::downgrade(&service);
        let thread_application = application.clone();
        let (stop, receiver) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = stopping.clone();
        let join = thread::Builder::new()
            .name("tuck-live-coach".into())
            .spawn(move || loop {
                if thread_stopping.load(Ordering::Acquire) {
                    break;
                }
                let Some(service) = thread_application.upgrade() else {
                    break;
                };
                // Core owns due times, one-request admission, per-call consent and
                // actionable provider errors. No provider or transcript is cached here.
                if !thread_stopping.load(Ordering::Acquire) {
                    let _ = service.tick();
                }
                drop(service);
                match receiver.recv_timeout(interval) {
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    _ => break,
                }
            })
            .map_err(|_| AppError::unavailable("The live coach worker could not start."))?;
        *owned = Some(Worker {
            application,
            stop,
            stopping,
            join,
        });
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        let mut owned = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        let Some(worker) = owned.take() else {
            return Ok(());
        };
        worker.stopping.store(true, Ordering::Release);
        // Invalidate an already-running completion BEFORE joining its thread.
        // The provider's bounded request may finish, but its result cannot be saved.
        let invalidated = worker.application.upgrade().map(|app| app.stop());
        let _ = worker.stop.send(());
        let joined = worker.join.join();
        invalidated.transpose()?;
        joined.map_err(|_| AppError::unavailable("The live coach worker stopped unexpectedly."))
    }
}

impl Drop for CoachWorker {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{atomic::AtomicUsize, Condvar};
    use std::time::Instant;

    struct FakeApp {
        ticks: AtomicUsize,
        stops: AtomicUsize,
        entered: mpsc::Sender<()>,
        blocked: bool,
        valid: Mutex<bool>,
        wake: Condvar,
        published: AtomicBool,
    }
    impl FakeApp {
        fn new(blocked: bool) -> (Arc<Self>, mpsc::Receiver<()>) {
            let (entered, receiver) = mpsc::channel();
            (
                Arc::new(Self {
                    ticks: AtomicUsize::new(0),
                    stops: AtomicUsize::new(0),
                    entered,
                    blocked,
                    valid: Mutex::new(true),
                    wake: Condvar::new(),
                    published: AtomicBool::new(false),
                }),
                receiver,
            )
        }
    }
    impl Application for FakeApp {
        fn tick(&self) -> Result<()> {
            self.ticks.fetch_add(1, Ordering::Relaxed);
            let mut valid = self.valid.lock().unwrap();
            let _ = self.entered.send(());
            if self.blocked {
                while *valid {
                    valid = self.wake.wait(valid).unwrap();
                }
            }
            self.published.store(*valid, Ordering::Release);
            Ok(())
        }
        fn stop(&self) -> Result<()> {
            self.stops.fetch_add(1, Ordering::Relaxed);
            *self.valid.lock().unwrap() = false;
            self.wake.notify_all();
            Ok(())
        }
    }

    #[test]
    fn composition_is_inert_and_host_start_is_idempotent_and_wakeable() {
        let worker = CoachWorker::default();
        let (app, entered) = FakeApp::new(false);
        assert!(worker.worker.lock().unwrap().is_none());
        assert_eq!(app.ticks.load(Ordering::Relaxed), 0);
        worker
            .start_application(app.clone(), Duration::from_secs(60))
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        worker
            .start_application(app.clone(), Duration::from_secs(60))
            .unwrap();
        let before = Instant::now();
        worker.shutdown().unwrap();
        assert!(before.elapsed() < Duration::from_secs(1));
        assert_eq!(app.ticks.load(Ordering::Relaxed), 1);
        assert_eq!(app.stops.load(Ordering::Relaxed), 1);
        worker.shutdown().unwrap();
        assert_eq!(app.stops.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn shutdown_invalidates_an_inflight_completion_before_joining_it() {
        let worker = CoachWorker::default();
        let (app, entered) = FakeApp::new(true);
        worker
            .start_application(app.clone(), Duration::from_secs(60))
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.shutdown().unwrap();
        assert_eq!(app.stops.load(Ordering::Relaxed), 1);
        assert!(!app.published.load(Ordering::Acquire));
    }

    #[test]
    fn idle_worker_does_not_keep_the_application_alive() {
        let worker = CoachWorker::default();
        let (app, entered) = FakeApp::new(false);
        let weak = Arc::downgrade(&app);
        worker
            .start_application(app.clone(), Duration::from_secs(60))
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(app);
        // Tick may still be returning when its entered signal is observed.
        let deadline = Instant::now() + Duration::from_secs(2);
        while weak.upgrade().is_some() && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(weak.upgrade().is_none());
        worker.shutdown().unwrap();
    }

    #[test]
    fn dropping_the_worker_stops_and_joins_its_host_work() {
        let worker = CoachWorker::default();
        let (app, entered) = FakeApp::new(true);
        worker
            .start_application(app.clone(), Duration::from_secs(60))
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(worker);
        assert_eq!(app.stops.load(Ordering::Relaxed), 1);
        assert!(!app.published.load(Ordering::Acquire));
    }
}
