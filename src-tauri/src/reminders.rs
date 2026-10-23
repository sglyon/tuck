//! A single host-owned, low-frequency native worker. Composition alone starts nothing.
use crate::adapters::{
    calendar,
    notifications::{self, Notifications},
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex, Weak,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tuck_core::{ports::Result, AppError, MeetingService};

const TICK: Duration = Duration::from_secs(15);
const REFRESH: Duration = Duration::from_secs(4 * 60);

trait Application: Send + Sync {
    fn enabled(&self) -> Result<bool>;
    fn claim(&self, now: &str) -> Result<Value>;
    fn suppressed(&self, now: &str) -> Result<bool>;
    fn snapshot(&self, events: &[Value], now: &str) -> Result<()>;
    fn invalidate_device(&self) -> Result<()>;
}
impl Application for MeetingService {
    fn enabled(&self) -> Result<bool> {
        self.meeting_reminders_enabled()
    }
    fn claim(&self, now: &str) -> Result<Value> {
        self.claim_meeting_reminders_at(now)
    }
    fn suppressed(&self, _: &str) -> Result<bool> {
        self.meeting_day_suppressed()
    }
    fn snapshot(&self, events: &[Value], now: &str) -> Result<()> {
        self.replace_calendar_snapshot("device", events, now)
            .map(|_| ())
    }
    fn invalidate_device(&self) -> Result<()> {
        self.invalidate_calendar_snapshot("device")
    }
}
trait Calendar: Send + Sync {
    fn authorized_events(&self, now: DateTime<Utc>) -> Result<Option<Vec<Value>>>;
}
struct DeviceCalendar;
impl Calendar for DeviceCalendar {
    fn authorized_events(&self, now: DateTime<Utc>) -> Result<Option<Vec<Value>>> {
        // status is read-only; this worker never invokes connect/request access.
        if !cfg!(target_os = "macos") || calendar::status()?["connected"] != true {
            return Ok(None);
        }
        let end = now + chrono::Duration::days(7);
        let events = calendar::events(&now.to_rfc3339(), &end.to_rfc3339())?;
        events.as_array().cloned().map(Some).ok_or_else(|| {
            AppError::new(
                "calendar_error",
                "Calendar returned an invalid event snapshot.",
            )
        })
    }
}
#[derive(Default)]
struct Progress {
    error: Option<String>,
}
struct Tick {
    last_refresh: Option<Instant>,
}
impl Tick {
    fn run(
        &mut self,
        app: &dyn Application,
        notifications: &dyn Notifications,
        calendar: &dyn Calendar,
        clock: &dyn Fn() -> DateTime<Utc>,
        monotonic: Instant,
        stopping: &AtomicBool,
    ) -> Result<&'static str> {
        if stopping.load(Ordering::Acquire) || !app.enabled()? {
            return Ok("disabled");
        }
        let now = clock();
        if app.suppressed(&now.to_rfc3339())? {
            return Ok("disabled");
        }
        let mut refresh_error = None;
        if self
            .last_refresh
            .is_none_or(|last| monotonic.saturating_duration_since(last) >= REFRESH)
        {
            // Attempt time, including failures, prevents hot retry loops.
            self.last_refresh = Some(monotonic);
            match calendar.authorized_events(now) {
                Ok(Some(events)) => {
                    if let Err(error) = app.snapshot(&events, &now.to_rfc3339()) {
                        app.invalidate_device()?;
                        refresh_error = Some(error);
                    }
                }
                Ok(None) => app.invalidate_device()?,
                Err(error) => {
                    app.invalidate_device()?;
                    refresh_error = Some(error);
                }
            }
        }
        // Native authorization failures must not stop the already-authorized
        // calendar cache from serving in-app reminders.
        let permission = notifications.permission()?;
        if stopping.load(Ordering::Acquire) || !matches!(permission, "granted" | "unknown") {
            return Ok(permission);
        }
        // Refresh the wall clock after helper work/sleep; never replay the due time
        // captured before a suspended machine woke up.
        let claims = app.claim(&clock().to_rfc3339())?;
        let delivery_time = clock();
        let due = claims.as_array().into_iter().flatten().any(|claim| {
            tuck_core::domain::CalendarEvent::from_value(&claim["event"])
                .and_then(|event| tuck_core::meeting_day::reminder_due(&event, delivery_time))
                .unwrap_or(false)
        });
        // Coalesce overlapping meetings into one generic notification. Claims remain
        // durable individually; failures, shutdown and sleep never enqueue a retry.
        if due
            && !stopping.load(Ordering::Acquire)
            && app.enabled()?
            && !app.suppressed(&delivery_time.to_rfc3339())?
        {
            notifications.show()?;
        }
        refresh_error.map_or(Ok(permission), Err)
    }
}
struct Worker {
    stop: mpsc::Sender<()>,
    stopping: Arc<AtomicBool>,
    join: JoinHandle<()>,
}
#[derive(Default)]
pub struct ReminderWorker {
    worker: Mutex<Option<Worker>>,
    notifications: Mutex<Option<Arc<dyn Notifications>>>,
    progress: Arc<Mutex<Progress>>,
}
impl ReminderWorker {
    pub fn start(
        &self,
        service: Arc<MeetingService>,
        notifications: Arc<dyn Notifications>,
    ) -> Result<()> {
        self.start_application(service, notifications, Arc::new(DeviceCalendar), TICK)
    }
    fn start_application(
        &self,
        service: Arc<dyn Application>,
        notifications: Arc<dyn Notifications>,
        calendar: Arc<dyn Calendar>,
        interval: Duration,
    ) -> Result<()> {
        let mut worker = self
            .worker
            .lock()
            .map_err(|_| AppError::unavailable("The reminder worker is unavailable."))?;
        if worker.is_some() {
            return Ok(());
        }
        let mut backend = self
            .notifications
            .lock()
            .map_err(|_| notifications::unavailable())?;
        let (stop, receiver) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let thread_stopping = stopping.clone();
        let progress = self.progress.clone();
        let thread_notifications = notifications.clone();
        let service: Weak<dyn Application> = Arc::downgrade(&service);
        let join = thread::Builder::new()
            .name("tuck-meeting-reminders".into())
            .spawn(move || {
                let mut tick = Tick { last_refresh: None };
                loop {
                    let Some(service) = service.upgrade() else {
                        break;
                    };
                    let result = tick.run(
                        service.as_ref(),
                        thread_notifications.as_ref(),
                        calendar.as_ref(),
                        &Utc::now,
                        Instant::now(),
                        &thread_stopping,
                    );
                    if let Ok(mut state) = progress.lock() {
                        match result {
                            Ok("disabled") => {}
                            Ok(_) => {
                                state.error = None;
                            }
                            Err(error) => {
                                state.error = Some(error.message);
                            }
                        }
                    }
                    drop(service);
                    match receiver.recv_timeout(interval) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        _ => break,
                    }
                }
            })
            .map_err(|_| AppError::unavailable("The reminder worker could not start."))?;
        *backend = Some(notifications);
        *worker = Some(Worker {
            stop,
            stopping,
            join,
        });
        Ok(())
    }
    pub fn status(&self, enabled: bool) -> Result<Value> {
        let backend = self
            .notifications
            .lock()
            .map_err(|_| notifications::unavailable())?
            .clone();
        let running = self
            .worker
            .lock()
            .map_err(|_| notifications::unavailable())?
            .is_some();
        let Some(backend) = backend else {
            return Ok(notifications::status("unavailable", enabled, false, None));
        };
        let error = self
            .progress
            .lock()
            .map_err(|_| notifications::unavailable())?
            .error
            .clone();
        match backend.permission() {
            Ok(permission) => Ok(notifications::status(
                permission,
                enabled,
                running,
                error.as_deref(),
            )),
            Err(error) => Ok(notifications::status(
                "unavailable",
                enabled,
                running,
                Some(&error.message),
            )),
        }
    }
    pub fn request_permission(&self) -> Result<()> {
        let backend = self
            .notifications
            .lock()
            .map_err(|_| notifications::unavailable())?
            .clone()
            .ok_or_else(notifications::unavailable)?;
        match backend.request_permission() {
            // A native host can still offer in-app reminders when the OS API is
            // unavailable (for example an unbundled macOS development process).
            Err(error) if error.code == "notification_unavailable" => Ok(()),
            result => result.map(|_| ()),
        }
    }
    pub fn shutdown(&self) {
        let mut owned = self.worker.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(worker) = owned.take() {
            worker.stopping.store(true, Ordering::Release);
            let _ = worker.stop.send(());
            let _ = worker.join.join();
        }
        *self.notifications.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}
impl Drop for ReminderWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;

    struct FakeApp {
        enabled: AtomicBool,
        suppressed: AtomicBool,
        claimed: AtomicBool,
        claims: AtomicUsize,
        invalidated: AtomicUsize,
        last_claim: Mutex<String>,
        batch_size: usize,
        snapshot_fail: bool,
    }
    impl Default for FakeApp {
        fn default() -> Self {
            Self {
                enabled: AtomicBool::new(true),
                suppressed: AtomicBool::new(false),
                claimed: AtomicBool::new(false),
                claims: AtomicUsize::new(0),
                invalidated: AtomicUsize::new(0),
                last_claim: Mutex::new(String::new()),
                batch_size: 1,
                snapshot_fail: false,
            }
        }
    }
    impl Application for FakeApp {
        fn enabled(&self) -> Result<bool> {
            Ok(self.enabled.load(Ordering::Relaxed))
        }
        fn claim(&self, now: &str) -> Result<Value> {
            self.claims.fetch_add(1, Ordering::Relaxed);
            *self.last_claim.lock().unwrap() = now.into();
            Ok(if self.claimed.swap(true, Ordering::Relaxed) {
                json!([])
            } else {
                Value::Array((0..self.batch_size).map(|i| json!({"occurrenceKey":format!("fixture-{i}"),"event":{"id":format!("ics-import:event-{i}"),"source":"ics-import","title":"Fixture", "start":"2026-10-23T12:04:00Z","end":"2026-10-23T12:34:00Z","attendees":[],"joinUrl":"https://example.test/meeting"}})).collect())
            })
        }
        fn suppressed(&self, _: &str) -> Result<bool> {
            Ok(self.suppressed.load(Ordering::Relaxed))
        }
        fn snapshot(&self, _: &[Value], _: &str) -> Result<()> {
            if self.snapshot_fail {
                Err(AppError::invalid("Snapshot fixture failure"))
            } else {
                Ok(())
            }
        }
        fn invalidate_device(&self) -> Result<()> {
            self.invalidated.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    struct FakeNotifications {
        permission: &'static str,
        checks: AtomicUsize,
        requests: AtomicUsize,
        shows: AtomicUsize,
        fail: bool,
        checked: Option<mpsc::Sender<()>>,
    }
    impl FakeNotifications {
        fn new(permission: &'static str, fail: bool) -> Self {
            Self {
                permission,
                checks: AtomicUsize::new(0),
                requests: AtomicUsize::new(0),
                shows: AtomicUsize::new(0),
                fail,
                checked: None,
            }
        }
    }
    impl Notifications for FakeNotifications {
        fn permission(&self) -> Result<&'static str> {
            self.checks.fetch_add(1, Ordering::Relaxed);
            if let Some(sender) = &self.checked {
                let _ = sender.send(());
            }
            if self.permission == "unavailable" {
                Err(notifications::unavailable())
            } else {
                Ok(self.permission)
            }
        }
        fn request_permission(&self) -> Result<&'static str> {
            self.requests.fetch_add(1, Ordering::Relaxed);
            if self.permission == "unavailable" {
                Err(notifications::unavailable())
            } else {
                Ok(self.permission)
            }
        }
        fn show(&self) -> Result<()> {
            self.shows.fetch_add(1, Ordering::Relaxed);
            if self.fail {
                Err(AppError::unavailable("Notification fixture failure"))
            } else {
                Ok(())
            }
        }
    }
    #[derive(Default)]
    struct FakeCalendar {
        calls: AtomicUsize,
        fail: bool,
        stop: Option<Arc<AtomicBool>>,
        events: Option<Vec<Value>>,
    }
    impl Calendar for FakeCalendar {
        fn authorized_events(&self, _: DateTime<Utc>) -> Result<Option<Vec<Value>>> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let Some(stop) = &self.stop {
                stop.store(true, Ordering::Release);
            }
            if self.fail {
                Err(AppError::unavailable("Calendar fixture failure"))
            } else {
                Ok(self.events.clone())
            }
        }
    }
    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-23T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn disabled_and_active_meetings_do_no_native_work() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("granted", false);
        let calendar = FakeCalendar::default();
        let stop = AtomicBool::new(false);
        let mut tick = Tick { last_refresh: None };
        app.enabled.store(false, Ordering::Relaxed);
        assert_eq!(
            tick.run(&app, &notifications, &calendar, &now, Instant::now(), &stop)
                .unwrap(),
            "disabled"
        );
        app.enabled.store(true, Ordering::Relaxed);
        app.suppressed.store(true, Ordering::Relaxed);
        tick.run(&app, &notifications, &calendar, &now, Instant::now(), &stop)
            .unwrap();
        assert_eq!(notifications.checks.load(Ordering::Relaxed), 0);
        assert_eq!(calendar.calls.load(Ordering::Relaxed), 0);
        assert_eq!(app.claims.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn denial_never_prompts_or_claims() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("denied", false);
        let mut tick = Tick { last_refresh: None };
        assert_eq!(
            tick.run(
                &app,
                &notifications,
                &FakeCalendar::default(),
                &now,
                Instant::now(),
                &AtomicBool::new(false)
            )
            .unwrap(),
            "denied"
        );
        assert_eq!(app.claims.load(Ordering::Relaxed), 0);
        assert_eq!(notifications.requests.load(Ordering::Relaxed), 0);
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn durable_claim_precedes_failed_delivery_and_is_not_retried() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("granted", true);
        let mut tick = Tick { last_refresh: None };
        let time = Instant::now();
        let calendar = FakeCalendar::default();
        assert!(tick
            .run(
                &app,
                &notifications,
                &calendar,
                &now,
                time,
                &AtomicBool::new(false)
            )
            .is_err());
        assert!(app.claimed.load(Ordering::Relaxed));
        tick.run(
            &app,
            &notifications,
            &calendar,
            &now,
            time + TICK,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 1);
        assert_eq!(calendar.calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn calendar_failure_invalidates_device_but_does_not_block_imports_or_hot_retry() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("unknown", false);
        let calendar = FakeCalendar {
            fail: true,
            ..Default::default()
        };
        let mut tick = Tick { last_refresh: None };
        let time = Instant::now();
        assert!(tick
            .run(
                &app,
                &notifications,
                &calendar,
                &now,
                time,
                &AtomicBool::new(false)
            )
            .is_err());
        tick.run(
            &app,
            &notifications,
            &calendar,
            &now,
            time + TICK,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(app.invalidated.load(Ordering::Relaxed), 1);
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 1);
        assert_eq!(calendar.calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn cancellation_during_calendar_refresh_prevents_claim_or_delivery() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("granted", false);
        let stop = Arc::new(AtomicBool::new(false));
        let calendar = FakeCalendar {
            stop: Some(stop.clone()),
            ..Default::default()
        };
        Tick { last_refresh: None }
            .run(&app, &notifications, &calendar, &now, Instant::now(), &stop)
            .unwrap();
        assert_eq!(app.claims.load(Ordering::Relaxed), 0);
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn claim_clock_is_refreshed_after_work_or_sleep() {
        let app = FakeApp::default();
        let calls = AtomicUsize::new(0);
        let clock = || {
            now()
                + chrono::Duration::hours(if calls.fetch_add(1, Ordering::Relaxed) == 0 {
                    0
                } else {
                    1
                })
        };
        Tick { last_refresh: None }
            .run(
                &app,
                &FakeNotifications::new("granted", false),
                &FakeCalendar::default(),
                &clock,
                Instant::now(),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(
            *app.last_claim.lock().unwrap(),
            (now() + chrono::Duration::hours(1)).to_rfc3339()
        );
    }
    #[test]
    fn simultaneous_meetings_produce_one_generic_notification() {
        let app = FakeApp {
            batch_size: 12,
            ..Default::default()
        };
        let notifications = FakeNotifications::new("granted", false);
        Tick { last_refresh: None }
            .run(
                &app,
                &notifications,
                &FakeCalendar::default(),
                &now,
                Instant::now(),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn sleep_after_claim_omits_the_old_notification_without_retry() {
        let app = FakeApp::default();
        let notifications = FakeNotifications::new("granted", false);
        let calls = AtomicUsize::new(0);
        let clock = || {
            now()
                + chrono::Duration::hours(if calls.fetch_add(1, Ordering::Relaxed) < 2 {
                    0
                } else {
                    1
                })
        };
        Tick { last_refresh: None }
            .run(
                &app,
                &notifications,
                &FakeCalendar::default(),
                &clock,
                Instant::now(),
                &AtomicBool::new(false),
            )
            .unwrap();
        assert!(app.claimed.load(Ordering::Relaxed));
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn host_explicitly_starts_one_worker_and_shutdown_wakes_its_wait() {
        let worker = ReminderWorker::default();
        let app = Arc::new(FakeApp::default());
        let (send, receive) = mpsc::channel();
        let notifications = Arc::new(FakeNotifications {
            checked: Some(send),
            ..FakeNotifications::new("denied", false)
        });
        assert_eq!(worker.status(false).unwrap()["running"], false);
        assert!(worker.request_permission().is_err());
        let calendar = Arc::new(FakeCalendar::default());
        worker
            .start_application(
                app.clone(),
                notifications.clone(),
                calendar.clone(),
                Duration::from_secs(60),
            )
            .unwrap();
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        worker
            .start_application(
                app,
                notifications.clone(),
                calendar,
                Duration::from_secs(60),
            )
            .unwrap();
        let start = Instant::now();
        worker.shutdown();
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(notifications.checks.load(Ordering::Relaxed), 1);
        assert_eq!(notifications.requests.load(Ordering::Relaxed), 0);
        assert_eq!(worker.status(false).unwrap()["running"], false);
    }
    #[test]
    fn unavailable_native_api_keeps_in_app_opt_in_available_but_inert_host_does_not() {
        let worker = ReminderWorker::default();
        assert!(worker.request_permission().is_err());
        let app = Arc::new(FakeApp::default());
        let notifications = Arc::new(FakeNotifications::new("unavailable", false));
        worker
            .start_application(
                app.clone(),
                notifications.clone(),
                Arc::new(FakeCalendar::default()),
                Duration::from_secs(60),
            )
            .unwrap();
        assert!(worker.request_permission().is_ok());
        assert_eq!(notifications.requests.load(Ordering::Relaxed), 1);
        assert_eq!(worker.status(true).unwrap()["available"], false);
        worker.shutdown();
    }
    #[test]
    fn failed_snapshot_revokes_freshness_and_still_allows_imported_reminder_delivery() {
        let app = FakeApp {
            snapshot_fail: true,
            ..Default::default()
        };
        let calendar = FakeCalendar {
            events: Some(vec![]),
            ..Default::default()
        };
        let notifications = FakeNotifications::new("granted", false);
        assert!(Tick { last_refresh: None }
            .run(
                &app,
                &notifications,
                &calendar,
                &now,
                Instant::now(),
                &AtomicBool::new(false)
            )
            .is_err());
        assert_eq!(app.invalidated.load(Ordering::Relaxed), 1);
        assert_eq!(notifications.shows.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn unavailable_notification_permission_does_not_block_calendar_refresh() {
        let app = FakeApp::default();
        let calendar = FakeCalendar::default();
        let notifications = FakeNotifications::new("unavailable", false);
        assert!(Tick { last_refresh: None }
            .run(
                &app,
                &notifications,
                &calendar,
                &now,
                Instant::now(),
                &AtomicBool::new(false)
            )
            .is_err());
        assert_eq!(calendar.calls.load(Ordering::Relaxed), 1);
        assert_eq!(app.claims.load(Ordering::Relaxed), 0);
    }
}
