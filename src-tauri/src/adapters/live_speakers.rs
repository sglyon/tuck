//! Delayed identity worker. Capture and STT never wait for this background pass.
use super::{
    models::ModelManager,
    refinement::{analyze, live_projection},
    voice_memory::VoiceMemory,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tuck_core::ports::OnSegment;
pub const SPEAKER_DELAY_SECONDS: f64 = 90.;
pub struct Worker {
    cancel: Arc<AtomicBool>,
    wake: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        let _ = self.wake.send(());
    }
    pub fn stop(&mut self) {
        self.cancel();
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}
pub fn start(
    models: Arc<ModelManager>,
    memory: Arc<VoiceMemory>,
    directory: PathBuf,
    meeting: String,
    cache: Arc<Mutex<Vec<Value>>>,
    callback: OnSegment,
) -> Option<Worker> {
    if !models.offline_speakers_installed() || memory.names().ok()?.is_empty() {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let (tx, rx) = mpsc::channel();
    let worker = thread::Builder::new()
        .name("tuck-delayed-speakers".into())
        .spawn(move || {
            if models.verify_offline_speakers().is_err() {
                return;
            }
            let mut checkpoint = 0.;
            let mut previous: Option<Value> = None;
            let mut next_due = std::time::Instant::now();
            loop {
                if rx.recv_timeout(Duration::from_millis(500)).is_ok()
                    || cancelled.load(Ordering::Acquire)
                {
                    break;
                }
                if std::time::Instant::now() < next_due {
                    continue;
                }
                let raw = match cache.lock() {
                    Ok(c) => c.clone(),
                    Err(_) => break,
                };
                let through = ["system", "microphone"]
                    .into_iter()
                    .filter_map(|source| {
                        let mut file =
                            super::audio::open_regular(&directory.join(format!("{source}.wav")))
                                .ok()?;
                        super::audio::inspect(&mut file)
                            .ok()
                            .map(|info| info.duration())
                    })
                    .fold(0., f64::max);
                if through > 7200. || through < SPEAKER_DELAY_SECONDS || through - checkpoint < 30.
                {
                    continue;
                }
                checkpoint = through;
                let window_start = (through - 300.).max(0.);
                let result = (|| -> tuck_core::ports::Result<Value> {
                    let stage = tempfile::Builder::new()
                        .prefix(".speaker-context-")
                        .tempdir_in(directory.parent().unwrap_or(&directory))
                        .map_err(|_| {
                            tuck_core::AppError::unavailable(
                                "Speaker context could not be prepared.",
                            )
                        })?;
                    let mut analyses = HashMap::new();
                    for source in ["system", "microphone"] {
                        if cancelled.load(Ordering::Acquire) {
                            return Err(tuck_core::AppError::unavailable(
                                "Speaker identification cancelled.",
                            ));
                        }
                        let path = directory.join(format!("{source}.wav"));
                        if !path.is_file() {
                            continue;
                        }
                        let snapshot = stage.path().join(format!("{source}.wav"));
                        if let Some(offset) =
                            super::audio::copy_window(&path, &snapshot, window_start, through)?
                        {
                            let mut analysis =
                                analyze(&models, &snapshot, source, true, Some(&cancelled))?;
                            for turn in &mut analysis.segments {
                                turn.start += offset;
                                turn.end += offset;
                            }
                            for chunk in &mut analysis.chunks {
                                chunk.start += offset;
                                chunk.end += offset;
                            }
                            analyses.insert(source.into(), analysis);
                        }
                    }
                    live_projection(
                        &raw,
                        &analyses,
                        &meeting,
                        &memory,
                        through - SPEAKER_DELAY_SECONDS,
                        window_start,
                        previous.as_ref(),
                    )
                })();
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                // Identification failure cannot interrupt audio capture or discard STT.
                if let Ok(projection) = result {
                    if callback(projection.clone()).is_ok() {
                        previous = Some(projection);
                    }
                }
                next_due = std::time::Instant::now() + Duration::from_secs(30);
            }
        })
        .ok()?;
    Some(Worker {
        cancel,
        wake: tx,
        thread: Some(worker),
    })
}
