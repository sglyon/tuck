//! Hardware and local speech adapter. WAV evidence is flushed before inference is queued.
//! The application alone applies retention after its durable completion transaction.
use super::{
    models::ModelManager,
    speech::{helper_path, PcmResampler, Segmenter, SpeechRuntime, SAMPLE_RATE},
};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use tuck_core::{
    ports::{Emit, OnSegment, RecordingPort, Result},
    AppError,
};
const QUEUE_CAPACITY: usize = 200;
const BLOCK: usize = 1600;
fn error(message: impl Into<String>) -> AppError {
    AppError::new("capture_error", message)
}
fn io_error(_: impl std::fmt::Display) -> AppError {
    error("The recording could not be saved. Check available storage. Existing audio has been preserved.")
}
fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value.lock().unwrap_or_else(|p| p.into_inner())
}

pub fn meeting_audio_dir(data_dir: &Path, meeting: &str) -> Result<PathBuf> {
    if meeting.is_empty()
        || meeting.len() > 100
        || !meeting
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err(error("Invalid meeting identifier."));
    }
    let root = data_dir.join("audio");
    let directory = root.join(meeting);
    if fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink())
        || fs::symlink_metadata(&directory).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(error("The audio destination is not a meeting folder."));
    }
    Ok(directory)
}
fn new_audio_dir(path: &Path) -> Result<()> {
    if path.exists() && fs::read_dir(path).map_err(io_error)?.next().is_some() {
        return Err(error("This meeting already contains saved audio. Start a new meeting to preserve the original."));
    }
    fs::create_dir_all(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}
#[derive(Default)]
struct State {
    meeting: Option<String>,
    state: &'static str,
    accepting: bool,
    accumulated: f64,
    epoch: Option<Instant>,
    error: Option<AppError>,
    last_signal: HashMap<&'static str, f64>,
    silence_warned: HashSet<&'static str>,
}
impl State {
    fn elapsed(&self) -> f64 {
        self.accumulated
            + if self.accepting {
                self.epoch.map_or(0.0, |t| t.elapsed().as_secs_f64())
            } else {
                0.0
            }
    }
    fn deactivate(&mut self) {
        self.accumulated = self.elapsed();
        self.accepting = false;
        self.epoch = None;
    }
}
#[derive(Clone)]
struct Context {
    state: Arc<Mutex<State>>,
    emit: Emit,
}
impl Context {
    fn fail(&self, problem: AppError) {
        let meeting = {
            let mut state = lock(&self.state);
            if state.error.is_some() || state.meeting.is_none() {
                return;
            }
            state.deactivate();
            state.state = "interrupted";
            state.error = Some(problem.clone());
            state.meeting.clone()
        };
        (self.emit)(
            "engine_error",
            json!({"code":"capture_interrupted","message":problem.message,"meetingId":meeting}),
        );
        (self.emit)(
            "recording_state",
            json!({"meetingId":meeting,"state":"interrupted"}),
        );
    }
    fn activation(&self) -> Result<()> {
        let mut state = lock(&self.state);
        if let Some(e) = &state.error {
            return Err(e.clone());
        }
        state.epoch = Some(Instant::now());
        state.accepting = true;
        state.state = "recording";
        Ok(())
    }
}
struct Frame {
    source: &'static str,
    samples: Vec<f32>,
    rate: u32,
    start: f64,
}
fn raw_frame(
    context: &Context,
    tx: &SyncSender<Frame>,
    source: &'static str,
    samples: Vec<f32>,
    rate: u32,
) {
    let start = {
        let state = lock(&context.state);
        if !state.accepting && !matches!(state.state, "pausing" | "processing") {
            return;
        }
        (state.elapsed() - samples.len() as f64 / rate as f64).max(0.0)
    };
    if tx
        .try_send(Frame {
            source,
            samples,
            rate,
            start,
        })
        .is_err()
    {
        context.fail(error("The audio input buffer overflowed. Recording stopped; saved audio is available for recovery."));
    }
}
enum Work {
    Audio {
        source: &'static str,
        samples: Vec<f32>,
        start: f64,
    },
    Flush(Sender<Result<()>>),
    Finish,
}
trait Pipeline {
    fn feed(&mut self, source: &'static str, samples: &[f32], start: f64) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
}
struct SpeechPipeline {
    runtime: SpeechRuntime,
    segmenters: HashMap<&'static str, Segmenter>,
    meeting: String,
    on_segment: OnSegment,
}
impl Pipeline for SpeechPipeline {
    fn feed(&mut self, source: &'static str, samples: &[f32], start: f64) -> Result<()> {
        let chunks = self
            .segmenters
            .get_mut(source)
            .ok_or_else(|| error("Unknown audio source."))?
            .feed(samples, start);
        for chunk in chunks {
            if let Some(segment) =
                self.runtime
                    .transcribe(&chunk.samples, chunk.start, source, &self.meeting)?
            {
                (self.on_segment)(segment)?;
            }
        }
        Ok(())
    }
    fn flush(&mut self) -> Result<()> {
        for (source, segmenter) in &mut self.segmenters {
            for chunk in segmenter.flush() {
                if let Some(segment) =
                    self.runtime
                        .transcribe(&chunk.samples, chunk.start, source, &self.meeting)?
                {
                    (self.on_segment)(segment)?;
                }
            }
        }
        Ok(())
    }
}
fn inference_loop(receiver: Receiver<Work>, pipeline: &mut dyn Pipeline) -> Result<()> {
    while let Ok(work) = receiver.recv() {
        match work {
            Work::Audio {
                source,
                samples,
                start,
            } => pipeline.feed(source, &samples, start)?,
            Work::Flush(reply) => {
                let result = pipeline.flush();
                let _ = reply.send(result.clone());
                result?;
            }
            Work::Finish => return pipeline.flush(),
        }
    }
    pipeline.flush()
}
fn enqueue_control(tx: &SyncSender<Work>, mut work: Work) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match tx.try_send(work) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => {
                return Err(error(
                    "Local transcription stopped. Audio has been preserved.",
                ))
            }
            Err(TrySendError::Full(pending)) => {
                work = pending;
                if Instant::now() >= deadline {
                    return Err(error("Local transcription is still busy. Audio has been preserved; restart Tuck to recover it."));
                }
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
struct Writer {
    inner: hound::WavWriter<BufWriter<File>>,
    frames: u64,
    resampler: PcmResampler,
    rate: u32,
    last_end: f64,
}
impl Writer {
    fn create(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(io_error)?;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        Ok(Self {
            inner: hound::WavWriter::new(BufWriter::with_capacity(64 * 1024, file), spec)
                .map_err(io_error)?,
            frames: 0,
            resampler: PcmResampler::new(SAMPLE_RATE)?,
            rate: SAMPLE_RATE,
            last_end: 0.0,
        })
    }
    fn write(&mut self, samples: &[f32]) -> Result<()> {
        for sample in samples {
            self.inner
                .write_sample((sample.clamp(-1.0, 1.0) * 32767.0) as i16)
                .map_err(io_error)?;
        }
        self.inner.flush().map_err(io_error)?;
        self.frames += samples.len() as u64;
        Ok(())
    }
}
fn save_and_enqueue(
    writer: &mut Writer,
    tx: &SyncSender<Work>,
    source: &'static str,
    samples: Vec<f32>,
    start: f64,
) -> Result<()> {
    writer.write(&samples)?;
    writer.last_end = start + samples.len() as f64 / SAMPLE_RATE as f64;
    tx.try_send(Work::Audio{source,samples,start}).map_err(|e|match e{TrySendError::Full(_)=>error("Speech processing could not keep up. Recording stopped to prevent transcript gaps; saved audio can be imported for recovery."),TrySendError::Disconnected(_)=>error("Local transcription was interrupted. Saved audio has been preserved.")})
}
fn flush_inputs(writers: &mut HashMap<&'static str, Writer>, tx: &SyncSender<Work>) -> Result<()> {
    for (source, writer) in writers {
        let samples = writer.resampler.feed(&[], true);
        if !samples.is_empty() {
            let start = writer.last_end;
            save_and_enqueue(writer, tx, source, samples, start)?;
        }
        writer.resampler = PcmResampler::new(writer.rate)?;
    }
    Ok(())
}
fn process_frame(
    frame: Frame,
    writers: &mut HashMap<&'static str, Writer>,
    tx: &SyncSender<Work>,
    context: &Context,
    levels: &mut HashMap<&'static str, f32>,
    last_level: &mut Instant,
) -> Result<()> {
    let writer = writers
        .get_mut(frame.source)
        .ok_or_else(|| error("Unknown capture source."))?;
    if writer.rate != frame.rate {
        writer.resampler = PcmResampler::new(frame.rate)?;
        writer.rate = frame.rate;
    }
    let clean: Vec<_> = frame
        .samples
        .into_iter()
        .map(|x| {
            if x.is_finite() {
                x.clamp(-1.0, 1.0)
            } else {
                0.0
            }
        })
        .collect();
    let samples = writer.resampler.feed(&clean, false);
    if samples.is_empty() {
        return Ok(());
    }
    let rms = (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt();
    let warning = {
        let mut state = lock(&context.state);
        let elapsed = state.elapsed();
        if rms > 0.005 {
            state.last_signal.insert(frame.source, elapsed);
            state.silence_warned.clear();
            None
        } else {
            let since = state.last_signal.values().copied().fold(0.0_f64, f64::max);
            if elapsed - since >= 30.0 && state.silence_warned.insert("all") {
                state.meeting.clone()
            } else {
                None
            }
        }
    };
    if let Some(meeting) = warning {
        (context.emit)(
            "recording_warning",
            json!({"code":"silence","source":"all","meetingId":meeting,"message":"No sound has been detected for 30 seconds. Check mute and audio sources; recording continues."}),
        );
    }
    levels.insert(
        frame.source,
        (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32)
            .sqrt()
            .mul_add(4.0, 0.0)
            .min(1.0),
    );
    save_and_enqueue(writer, tx, frame.source, samples, frame.start)?;
    if last_level.elapsed() >= Duration::from_millis(100) {
        *last_level = Instant::now();
        (context.emit)(
            "audio_level",
            json!({"microphone":levels.get("microphone").copied().unwrap_or(0.0),"system":levels.get("system").copied().unwrap_or(0.0)}),
        );
    }
    Ok(())
}
fn drain_inputs(
    raw: &Receiver<Frame>,
    writers: &mut HashMap<&'static str, Writer>,
    work: &SyncSender<Work>,
    context: &Context,
    levels: &mut HashMap<&'static str, f32>,
    last_level: &mut Instant,
) -> Result<()> {
    let mut failure = None;
    // Even when inference is full or disconnected, preserve every already-captured frame.
    for frame in raw.try_iter() {
        if let Err(e) = process_frame(frame, writers, work, context, levels, last_level) {
            failure.get_or_insert(e);
        }
    }
    if let Err(e) = flush_inputs(writers, work) {
        failure.get_or_insert(e);
    }
    failure.map_or(Ok(()), Err)
}

enum Source {
    Cpal {
        stream: cpal::Stream,
        stopping: Arc<AtomicBool>,
    },
    Process {
        child: Child,
        reader: Option<JoinHandle<()>>,
        diagnostics: Option<JoinHandle<()>>,
        stopping: Arc<AtomicBool>,
    },
}
impl Source {
    fn stop(self) {
        match self {
            Source::Cpal { stream, stopping } => {
                stopping.store(true, Ordering::Release);
                let _ = stream.pause();
                drop(stream);
            }
            Source::Process {
                mut child,
                reader,
                diagnostics,
                stopping,
            } => {
                stopping.store(true, Ordering::Release);
                drop(child.stdin.take());
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        _ => {
                            if Instant::now() >= deadline {
                                let _ = child.kill();
                                break;
                            }
                            thread::sleep(Duration::from_millis(10));
                        }
                    }
                }
                let _ = child.wait();
                if let Some(thread) = reader {
                    let _ = thread.join();
                }
                if let Some(thread) = diagnostics {
                    let _ = thread.join();
                }
            }
        }
    }
}
fn cpal_stream<T: cpal::SizedSample + Copy + 'static>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    convert: fn(T) -> f32,
    source: &'static str,
    context: Context,
    tx: SyncSender<Frame>,
    stopping: Arc<AtomicBool>,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let rate = config.sample_rate;
    let failed = context.clone();
    device.build_input_stream(config,move|data:&[T],_:&cpal::InputCallbackInfo|{let samples=data.chunks_exact(channels).map(|frame|frame.iter().map(|&x|convert(x)).sum::<f32>()/channels as f32).collect();raw_frame(&context,&tx,source,samples,rate);},move|_|{if !stopping.load(Ordering::Acquire){failed.fail(error("Audio capture was interrupted. Check your device and permissions. Saved audio has been preserved."));}},None).map_err(|_|error("Audio capture could not start. Check the selected device and microphone permission."))
}
fn start_cpal(
    device_id: &str,
    source: &'static str,
    context: Context,
    tx: SyncSender<Frame>,
) -> Result<Source> {
    let host = cpal::default_host();
    let device = if device_id.is_empty() {
        if source == "system" { host.default_output_device() } else { host.default_input_device() }
    } else {
        host.devices().map_err(|_| error("Audio devices could not be listed."))?
            .find(|device| device.id().is_ok_and(|id| id.to_string() == device_id))
    }.ok_or_else(|| error("The selected audio device is unavailable. Reconnect it or choose System default in Settings."))?;
    let supported = if source == "system" {
        device.default_output_config()
    } else {
        device.default_input_config()
    }
    .map_err(|_| error("The audio device has no supported capture format."))?;
    let config = supported.config();
    if config.channels == 0 || !(8_000..=192_000).contains(&config.sample_rate) {
        return Err(error(
            "The selected audio device uses an unsupported sample rate or channel layout.",
        ));
    }
    let stopping = Arc::new(AtomicBool::new(false));
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => cpal_stream(
            &device,
            &config,
            |x: f32| x,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::F64 => cpal_stream(
            &device,
            &config,
            |x: f64| x as f32,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::I16 => cpal_stream(
            &device,
            &config,
            |x: i16| x as f32 / 32768.0,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::U16 => cpal_stream(
            &device,
            &config,
            |x: u16| (x as f32 - 32768.0) / 32768.0,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::I32 => cpal_stream(
            &device,
            &config,
            |x: i32| x as f32 / 2147483648.0,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::I8 => cpal_stream(
            &device,
            &config,
            |x: i8| x as f32 / 128.0,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        cpal::SampleFormat::U8 => cpal_stream(
            &device,
            &config,
            |x: u8| (x as f32 - 128.0) / 128.0,
            source,
            context,
            tx,
            stopping.clone(),
        )?,
        _ => return Err(error(
            "The audio device's sample format is unsupported. Choose a standard PCM input device.",
        )),
    };
    stream
        .play()
        .map_err(|_| error("The audio stream could not start. Check device permissions."))?;
    Ok(Source::Cpal { stream, stopping })
}
fn start_process(
    mut command: Command,
    ready_required: bool,
    context: Context,
    tx: SyncSender<Frame>,
) -> Result<Source> {
    let mut child=command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|_|error("The system audio capture helper could not start. On Linux install pulseaudio-utils and enable the PulseAudio/PipeWire monitor; on macOS check the native helper and audio permissions."))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let diag_context = context.clone();
    let stopping = Arc::new(AtomicBool::new(false));
    let diagnostic_stop = stopping.clone();
    let reader_stop = stopping.clone();
    let diagnostics = thread::spawn(move || {
        let mut first = true;
        for line in BufReader::new(stderr).lines() {
            let Ok(line) = line else {
                break;
            };
            if first {
                first = false;
                let _ = ready_tx.send(line.trim() == "READY");
            }
            if line.starts_with("ERROR:") && !diagnostic_stop.load(Ordering::Acquire) {
                diag_context.fail(error("System audio capture failed. Check Screen & System Audio Recording permission. Saved audio has been preserved."));
            }
        }
        if first {
            let _ = ready_tx.send(false);
        }
    });
    let (first_pcm_tx, first_pcm_rx) = mpsc::sync_channel(1);
    let reader_context = context.clone();
    let reader = thread::spawn(move || {
        let mut stdout = stdout;
        let mut bytes = vec![0; BLOCK * 4];
        let mut pending = Vec::new();
        let mut first = true;
        loop {
            match stdout.read(&mut bytes) {
                Ok(0) | Err(_) => {
                    if first {
                        let _ = first_pcm_tx.send(false);
                    }
                    if !reader_stop.load(Ordering::Acquire) {
                        reader_context.fail(error("System audio capture stopped unexpectedly. Saved audio has been preserved."));
                    }
                    break;
                }
                Ok(n) => {
                    pending.extend_from_slice(&bytes[..n]);
                    let complete = pending.len() / 4 * 4;
                    if complete == 0 {
                        continue;
                    }
                    if first {
                        first = false;
                        let _ = first_pcm_tx.send(true);
                    }
                    let samples = pending[..complete]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b))
                        .collect();
                    pending.drain(..complete);
                    raw_frame(&reader_context, &tx, "system", samples, SAMPLE_RATE);
                }
            }
        }
    });
    let ready = if ready_required {
        ready_rx.recv_timeout(Duration::from_secs(20)) == Ok(true)
    } else {
        first_pcm_rx.recv_timeout(Duration::from_secs(15)) == Ok(true)
    };
    if !ready {
        stopping.store(true, Ordering::Release);
        let _ = child.kill();
        let _ = child.wait();
        let _ = reader.join();
        let _ = diagnostics.join();
        return Err(error(if ready_required {
            "System audio could not start. Allow Tuck in System Settings → Privacy & Security → Screen & System Audio Recording, then restart Tuck."
        } else {
            "The PulseAudio/PipeWire system monitor could not start. Install pulseaudio-utils and check the default output monitor."
        }));
    }
    Ok(Source::Process {
        child,
        reader: Some(reader),
        diagnostics: Some(diagnostics),
        stopping,
    })
}
fn start_sources(
    system: bool,
    microphone: bool,
    devices: &DeviceSelection,
    context: &Context,
    tx: &SyncSender<Frame>,
) -> Result<Vec<Source>> {
    let mut sources = vec![];
    let result = (|| {
        if system {
            #[cfg(target_os = "macos")]
            {
                if !devices.system.is_empty() && devices.system != "system-mix" {
                    return Err(error(
                        "macOS captures the system audio mix. Choose All system audio in Settings.",
                    ));
                }
                sources.push(start_process(
                    Command::new(helper_path("tuck-audio", "TUCK_AUDIO_HELPER")?),
                    true,
                    context.clone(),
                    tx.clone(),
                )?);
            }
            #[cfg(target_os = "windows")]
            {
                sources.push(start_cpal(
                    &devices.system,
                    "system",
                    context.clone(),
                    tx.clone(),
                )?);
            }
            #[cfg(target_os = "linux")]
            {
                let mut command = Command::new("parec");
                command.args([
                    &format!(
                        "--device={}",
                        if devices.system.is_empty() {
                            "@DEFAULT_MONITOR@"
                        } else {
                            &devices.system
                        }
                    ),
                    "--raw",
                    "--format=float32le",
                    "--rate=16000",
                    "--channels=1",
                    "--latency-msec=100",
                ]);
                sources.push(start_process(command, false, context.clone(), tx.clone())?);
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
            {
                return Err(error(
                    "System audio capture is unavailable on this platform.",
                ));
            }
        }
        if microphone {
            sources.push(start_cpal(
                &devices.microphone,
                "microphone",
                context.clone(),
                tx.clone(),
            )?);
        }
        Ok(())
    })();
    if let Err(e) = result {
        for source in sources {
            source.stop();
        }
        return Err(e);
    }
    Ok(sources)
}
fn stop_sources(sources: &mut Vec<Source>) {
    for source in sources.drain(..) {
        source.stop();
    }
}
enum Control {
    Pause(Sender<Result<()>>),
    Resume(Sender<Result<()>>),
    Stop(Sender<Result<()>>),
}
struct Session {
    control: Sender<Control>,
    acquisition: Option<JoinHandle<()>>,
    inference: Option<JoinHandle<()>>,
    done: Receiver<Result<()>>,
}
#[derive(Clone, Default)]
struct DeviceSelection {
    microphone: String,
    system: String,
}
// Only idle, successfully used recognizers are reusable. A generation prevents
// an in-flight worker from restoring a model after unload/install/shutdown.
struct WarmSpeech<T> {
    generation: u64,
    ready: Option<(u32, T)>,
}
impl<T> Default for WarmSpeech<T> {
    fn default() -> Self { Self { generation: 0, ready: None } }
}
impl<T> WarmSpeech<T> {
    fn take(&mut self, threads: u32) -> (u64, Option<T>) {
        (self.generation, self.ready.take().and_then(|(key, runtime)| (key == threads).then_some(runtime)))
    }
    fn put(&mut self, generation: u64, threads: u32, runtime: T) {
        if self.generation == generation { self.ready = Some((threads, runtime)); }
    }
    fn clear(&mut self) { self.generation = self.generation.wrapping_add(1); self.ready = None; }
}

pub struct CaptureEngine {
    devices: Mutex<DeviceSelection>,
    diarization: Mutex<bool>,
    data_dir: PathBuf,
    _resources_dir: PathBuf,
    models: Arc<ModelManager>,
    context: Context,
    operation: Mutex<()>,
    session: Mutex<Option<Session>>,
    warm_speech: Arc<Mutex<WarmSpeech<SpeechRuntime>>>,
}
impl CaptureEngine {
    pub fn new(data_dir: PathBuf, resources_dir: PathBuf, emit: Emit) -> Self {
        let models = ModelManager::new(&data_dir, emit.clone());
        Self {
            data_dir,
            devices: Mutex::new(DeviceSelection::default()),
            diarization: Mutex::new(true),
            _resources_dir: resources_dir,
            models,
            context: Context {
                state: Arc::new(Mutex::new(State {
                    state: "idle",
                    ..Default::default()
                })),
                emit,
            },
            operation: Mutex::new(()),
            session: Mutex::new(None),
            warm_speech: Arc::new(Mutex::new(WarmSpeech::default())),
        }
    }
    /// Load verified local weights only; never opens an audio stream or requests
    /// permission. Host activation is explicit so diagnostic composition is inert.
    pub fn prewarm_dictation(self: &Arc<Self>, threads: u32) {
        if !self.models.installed("parakeet") { return; }
        let engine = self.clone();
        let generation = lock(&self.warm_speech).generation;
        let _ = thread::Builder::new().name("tuck-dictation-warmup".into()).spawn(move || {
            let _operation = lock(&engine.operation);
            if lock(&engine.session).is_some() { return; }
            let threads = threads.clamp(1, 4);
            {
                let cache = lock(&engine.warm_speech);
                if cache.generation != generation || cache.ready.as_ref().is_some_and(|(key, _)| *key == threads) { return; }
            }
            if let Ok(runtime) = SpeechRuntime::load(&engine.models, threads, false) {
                lock(&engine.warm_speech).put(generation, threads, runtime);
            }
        });
    }
    pub fn clear_dictation_warmth(&self) { lock(&self.warm_speech).clear(); }
    fn stop_inner(&self) -> Result<f64> {
        let duration = {
            let mut state = lock(&self.context.state);
            if state.meeting.is_none() {
                return Err(error("There is no active recording to stop."));
            }
            state.deactivate();
            state.state = "processing";
            state.accumulated
        };
        let mut session_guard = lock(&self.session);
        let session = session_guard.as_mut().ok_or_else(|| {
            error("The recording worker is unavailable. Saved audio has been preserved.")
        })?;
        let (tx, rx) = mpsc::channel();
        let stopped = if session.control.send(Control::Stop(tx)).is_ok() {
            match rx.recv_timeout(Duration::from_secs(130)){Ok(result)=>result,Err(mpsc::RecvTimeoutError::Disconnected)=>Err(error("The capture worker stopped unexpectedly. Saved audio has been preserved.")),Err(mpsc::RecvTimeoutError::Timeout)=>return Err(error("Audio capture did not stop in time. Saved audio has been preserved; restart Tuck to recover it."))}
        } else {
            Err(error(
                "The capture worker stopped unexpectedly. Saved audio has been preserved.",
            ))
        };
        // Completed failures still release the session; timeouts retain handles to prevent races.
        let outcome = match session.done.recv_timeout(Duration::from_secs(130)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(error("The speech worker stopped unexpectedly. Saved audio has been preserved.")),
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(error("Local transcription is still busy. Audio has been preserved; restart Tuck to recover it.")),
        };
        if let Some(thread) = session.acquisition.take() {
            let _ = thread.join();
        }
        if let Some(thread) = session.inference.take() {
            let _ = thread.join();
        }
        *session_guard = None;
        let failure = {
            let mut state = lock(&self.context.state);
            state.meeting = None;
            state.state = "idle";
            state.error.clone()
        };
        (self.context.emit)("audio_level", json!({"microphone":0,"system":0}));
        if let Some(e) = failure {
            return Err(e);
        }
        stopped?;
        outcome?;
        Ok((duration * 1000.0).round() / 1000.0)
    }
}
impl RecordingPort for CaptureEngine {
    fn configure(&self, settings: &Value) -> Result<()> {
        *lock(&self.diarization) = settings["diarization"].as_bool().unwrap_or(true);
        *lock(&self.devices) = DeviceSelection {
            microphone: settings["microphoneDeviceId"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
            system: settings["systemDeviceId"].as_str().unwrap_or("").to_owned(),
        };
        Ok(())
    }
    fn status(&self) -> Value {
        let mut status = self.models.status();
        let state = lock(&self.context.state);
        status["activeMeetingId"] = json!(state.meeting);
        status["recordingState"] = json!(state.state);
        status["captureAvailable"] = json!(cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )));
        if let Some(error) = &state.error {
            status["error"] = json!(error.message);
        }
        status
    }
    fn start(
        &self,
        meeting: &str,
        on_segment: OnSegment,
        system: bool,
        microphone: bool,
        threads: u32,
        _keep_audio: bool,
    ) -> Result<()> {
        let _operation = lock(&self.operation);
        if lock(&self.session).is_some() {
            return Err(error(
                "A recording is already active. Stop it before starting another.",
            ));
        }
        if !system && !microphone {
            return Err(error(
                "Choose microphone, system audio, or both before recording.",
            ));
        }
        let directory = meeting_audio_dir(&self.data_dir, meeting)?;
        if directory.exists() && fs::read_dir(&directory).map_err(io_error)?.next().is_some() {
            return Err(error(
                "This meeting already contains saved audio. Start a new meeting.",
            ));
        }
        {
            let mut state = lock(&self.context.state);
            *state = State {
                meeting: Some(meeting.into()),
                state: "starting",
                ..Default::default()
            };
        }
        let (work_tx, work_rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let models = self.models.clone();
        let diarization = *lock(&self.diarization);
        let context = self.context.clone();
        let id = meeting.to_owned();
        let warm_speech = self.warm_speech.clone();
        let inference = thread::Builder::new().name("tuck-local-speech".into()).spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<()> {
                let key = threads.clamp(1, 4);
                let (generation, warmed) = if diarization { (0, None) } else { lock(&warm_speech).take(key) };
                let warmed = warmed.and_then(|mut runtime| runtime.is_alive().then_some(runtime));
                let runtime = match warmed {
                    Some(runtime) => runtime,
                    None => SpeechRuntime::load(&models, threads, diarization)?,
                };
                // The recognizer is resident, but each VAD is loaded afresh.
                models.verify_vad()?;
                let mut segmenters = HashMap::new();
                for (source, enabled) in [("microphone", microphone), ("system", system)] {
                    if enabled { segmenters.insert(source, Segmenter::new(&models.vad_path())?); }
                }
                let mut pipeline = SpeechPipeline { runtime, segmenters, meeting: id, on_segment };
                let _ = ready_tx.send(Ok(()));
                let result = inference_loop(work_rx, &mut pipeline);
                if result.is_ok() && !diarization {
                    pipeline.runtime.reset_session();
                    lock(&warm_speech).put(generation, key, pipeline.runtime);
                }
                result
            })).unwrap_or_else(|_| Err(error("Local transcription failed. The original audio has been preserved for recovery.")));
            if let Err(e) = &result { let _ = ready_tx.send(Err(e.clone())); context.fail(e.clone()); }
            let _ = done_tx.send(result);
        }).map_err(|_| error("The speech worker could not start."))?;
        let ready = ready_rx
            .recv_timeout(Duration::from_secs(200))
            .map_err(|_| error("The speech model did not load in time."));
        if let Err(e) = ready.and_then(|x| x) {
            drop(work_tx);
            let _ = inference.join();
            let mut state = lock(&self.context.state);
            state.meeting = None;
            state.state = "idle";
            return Err(e);
        }
        let (control, control_rx) = mpsc::channel();
        let (acq_ready_tx, acq_ready_rx) = mpsc::channel();
        let context = self.context.clone();
        let devices = lock(&self.devices).clone();
        let acquisition=thread::Builder::new().name("tuck-audio-owner".into()).spawn(move||{
            let mut writers=HashMap::new();let mut sources=vec![];let (raw_tx,raw_rx)=mpsc::sync_channel(QUEUE_CAPACITY);let mut levels=HashMap::new();let mut last_level=Instant::now();
            let initial=(||->Result<()>{new_audio_dir(&directory)?;for(source,enabled)in[("microphone",microphone),("system",system)]{if enabled{writers.insert(source,Writer::create(&directory.join(format!("{source}.wav")))?);}}sources=start_sources(system,microphone,&devices,&context,&raw_tx)?;context.activation()})();
            let _=acq_ready_tx.send(initial.clone());
            if initial.is_err(){stop_sources(&mut sources);drop(work_tx);let empty:Vec<_>=writers.iter().filter(|(_,w)|w.frames==0).map(|(s,_)|directory.join(format!("{s}.wav"))).collect();drop(writers);for file in empty{let _=fs::remove_file(file);}let _=fs::remove_dir(&directory);return;}
            loop {
                if lock(&context.state).error.is_some(){stop_sources(&mut sources);}
                match control_rx.try_recv(){
                    Ok(Control::Pause(reply))=>{
                        {let mut state=lock(&context.state);state.deactivate();state.state="pausing";}stop_sources(&mut sources);
                        let mut failure=drain_inputs(&raw_rx,&mut writers,&work_tx,&context,&mut levels,&mut last_level).err();let(tx,rx)=mpsc::channel();
                        match enqueue_control(&work_tx,Work::Flush(tx)){Ok(())=>{match rx.recv_timeout(Duration::from_secs(120)){Ok(Ok(()))=>{},Ok(Err(e))=>{failure.get_or_insert(e);},Err(_)=>{failure.get_or_insert_with(||error("Speech processing did not pause in time. Audio has been preserved."));}}},Err(e)=>{failure.get_or_insert(e);}}
                        let result=failure.map_or(Ok(()),Err);
                        if result.is_ok(){let mut state=lock(&context.state);if state.error.is_none(){state.state="paused";}}else if let Err(e)=&result{context.fail(e.clone());}(context.emit)("audio_level",json!({"microphone":0,"system":0}));let _=reply.send(result);
                    }
                    Ok(Control::Resume(reply))=>{let result=start_sources(system,microphone,&devices,&context,&raw_tx).and_then(|created|{sources=created;context.activation()});if result.is_err(){stop_sources(&mut sources);}let _=reply.send(result);}
                    Ok(Control::Stop(reply))=>{lock(&context.state).deactivate();stop_sources(&mut sources);let mut failure=drain_inputs(&raw_rx,&mut writers,&work_tx,&context,&mut levels,&mut last_level).err();if let Err(e)=enqueue_control(&work_tx,Work::Finish){failure.get_or_insert(e);}let result=failure.map_or(Ok(()),Err);drop(writers);let _=reply.send(result);break;}
                    Err(mpsc::TryRecvError::Disconnected)=>{stop_sources(&mut sources);break;},_=>{}
                }
                match raw_rx.recv_timeout(Duration::from_millis(20)){Ok(frame)=>{if let Err(e)=process_frame(frame,&mut writers,&work_tx,&context,&mut levels,&mut last_level){context.fail(e);}},Err(mpsc::RecvTimeoutError::Disconnected)=>{context.fail(error("The audio capture worker stopped unexpectedly."));},Err(mpsc::RecvTimeoutError::Timeout)=>{}}
            }
        }).map_err(|_|error("The audio worker could not start."))?;
        match acq_ready_rx
            .recv_timeout(Duration::from_secs(30))
            .map_err(|_| error("Audio capture did not start in time."))
            .and_then(|x| x)
        {
            Ok(()) => {
                *lock(&self.session) = Some(Session {
                    control,
                    acquisition: Some(acquisition),
                    inference: Some(inference),
                    done,
                });
                Ok(())
            }
            Err(e) => {
                drop(control);
                let _ = acquisition.join();
                let _ = inference.join();
                let mut state = lock(&self.context.state);
                state.meeting = None;
                state.state = "idle";
                Err(e)
            }
        }
    }
    fn pause(&self) -> Result<()> {
        let _operation = lock(&self.operation);
        if lock(&self.context.state).state != "recording" {
            return Err(error("Only an active recording can be paused."));
        }
        let (tx, rx) = mpsc::channel();
        lock(&self.session)
            .as_ref()
            .ok_or_else(|| error("The recording worker is unavailable."))?
            .control
            .send(Control::Pause(tx))
            .map_err(|_| error("The recording worker stopped."))?;
        rx.recv_timeout(Duration::from_secs(130))
            .map_err(|_| error("The recording did not pause in time."))?
    }
    fn resume(&self) -> Result<()> {
        let _operation = lock(&self.operation);
        if lock(&self.context.state).state != "paused" {
            return Err(error("There is no paused recording to resume."));
        }
        let (tx, rx) = mpsc::channel();
        lock(&self.session)
            .as_ref()
            .ok_or_else(|| error("The recording worker is unavailable."))?
            .control
            .send(Control::Resume(tx))
            .map_err(|_| error("The recording worker stopped."))?;
        rx.recv_timeout(Duration::from_secs(30))
            .map_err(|_| error("Audio capture did not resume in time."))?
    }
    fn stop(&self) -> Result<f64> {
        let _operation = lock(&self.operation);
        self.stop_inner()
    }
    fn shutdown(&self) -> Result<()> {
        let _operation = lock(&self.operation);
        self.clear_dictation_warmth();
        if lock(&self.session).is_some() {
            let result = self.stop_inner().map(|_| ());
            self.clear_dictation_warmth();
            result
        } else {
            Ok(())
        }
    }
    fn unload(&self) -> Result<()> {
        let _operation = lock(&self.operation);
        if lock(&self.session).is_some() {
            return Err(error("Stop recording before unloading speech models."));
        }
        self.clear_dictation_warmth();
        Ok(())
    }
    fn install_model(&self, kind: &str) -> Result<Value> {
        let _operation = lock(&self.operation);
        if lock(&self.session).is_some() {
            return Err(error("Stop recording before installing a model."));
        }
        self.clear_dictation_warmth();
        self.models.install(kind)
    }
    fn import_file(
        &self,
        path: &str,
        meeting: &str,
        on_segment: OnSegment,
        threads: u32,
    ) -> Result<f64> {
        let _operation = lock(&self.operation);
        if lock(&self.session).is_some() {
            return Err(error("Stop the active recording before importing audio."));
        }
        let source = Path::new(path);
        let mut original = super::audio::open_regular(source)?;
        if original.metadata().map_err(io_error)?.len() > super::import_audio::MAX_FILE_BYTES {
            return Err(error(
                "Choose audio smaller than 2 GiB and no longer than 12 hours.",
            ));
        }
        let preflight = super::audio::inspect(&mut original).ok();
        drop(original);
        if preflight
            .as_ref()
            .is_some_and(|wav| wav.duration() > super::import_audio::MAX_SECONDS as f64)
        {
            return Err(error("Audio imports are limited to 12 hours."));
        }
        let decoder = if preflight.is_none() {
            Some(super::import_audio::Decoder::open(source)?)
        } else {
            None
        };
        let directory = meeting_audio_dir(&self.data_dir, meeting)?;
        if directory.exists() && fs::read_dir(&directory).map_err(io_error)?.next().is_some() {
            return Err(error(
                "This meeting already contains audio. Import into a new meeting.",
            ));
        }
        let mut runtime = SpeechRuntime::load(&self.models, threads, *lock(&self.diarization))?;
        let mut segmenter = Segmenter::new(&self.models.vad_path())?;
        new_audio_dir(&directory)?;
        // Normalize into the new meeting only. Repairable interrupted headers are
        // rebuilt while streaming the owned samples; the original stays untouched.
        if let Some(decoder) = decoder {
            decoder.normalize_to(&directory.join("import.wav"))?;
        } else if preflight.is_some_and(|wav| wav.repairable) {
            super::audio::copy_normalized(source, &directory.join("import.wav"))?;
        } else {
            super::audio::copy_exact(source, &directory.join("import.wav"))?;
        }
        let mut reader = hound::WavReader::open(directory.join("import.wav")).map_err(io_error)?;
        let spec = reader.spec();
        if spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
            || !matches!(spec.channels, 1 | 2)
            || !(8_000..=192_000).contains(&spec.sample_rate)
            || reader.duration() == 0
        {
            return Err(error("The preserved WAV copy has an unsupported format."));
        }
        let duration = reader.duration() as f64 / spec.sample_rate as f64;
        let resampler = PcmResampler::new(spec.sample_rate)?;
        let block = (spec.sample_rate as usize / 10) * spec.channels as usize;
        let mut samples = reader.samples::<i16>();
        let mut count = 0usize;
        loop {
            let input:Vec<i16>=samples.by_ref().take(block).collect::<std::result::Result<Vec<_>,_>>().map_err(|_|error("The WAV audio is truncated or unreadable. Its preserved copy is available for recovery."))?;
            let last = input.len() < block;
            if !input.len().is_multiple_of(spec.channels as usize) {
                return Err(error(
                    "The WAV contains an incomplete audio frame. Its copy has been preserved.",
                ));
            }
            let mono: Vec<f32> = input
                .chunks_exact(spec.channels as usize)
                .map(|frame| {
                    frame.iter().map(|v| *v as f32 / 32768.0).sum::<f32>() / spec.channels as f32
                })
                .collect();
            let out = resampler.feed(&mono, last);
            let mut chunks = segmenter.feed(&out, count as f64 / SAMPLE_RATE as f64);
            count += out.len();
            if last {
                chunks.extend(segmenter.flush());
            }
            for chunk in chunks {
                if let Some(mut segment) =
                    runtime.transcribe(&chunk.samples, chunk.start, "import", meeting)?
                {
                    if let Some(end) = segment["end"].as_f64() {
                        segment["end"] = json!(end.min((duration * 1000.0).round() / 1000.0));
                    }
                    on_segment(segment)?;
                }
            }
            if last {
                break;
            }
        }
        Ok((duration * 1000.0).round() / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warmed_engine_is_exclusive_and_invalidated_by_settings_or_unload() {
        let mut cache = WarmSpeech::default();
        let (generation, empty) = cache.take(2);
        assert_eq!(empty, None::<String>);
        cache.put(generation, 2, "verified engine".into());
        let (generation, first) = cache.take(2);
        assert_eq!(first.as_deref(), Some("verified engine"));
        assert!(cache.take(2).1.is_none(), "a recognizer must have one owner");
        cache.put(generation, 2, first.unwrap());
        assert!(cache.take(4).1.is_none(), "thread changes must rebuild the engine");
        cache.put(generation, 2, "old engine".into());
        cache.clear();
        cache.put(generation, 2, "late worker".into());
        assert!(cache.take(2).1.is_none(), "unload must reject late returns");
    }
    #[test]
    #[ignore = "requires explicitly provisioned local speech weights; does not open the microphone"]
    fn benchmark_dictation_model_startup() {
        let data = PathBuf::from(std::env::var_os("TUCK_BENCH_DATA").expect("Set TUCK_BENCH_DATA to an existing model installation"));
        let engine = CaptureEngine::new(data, PathBuf::new(), Arc::new(|_, _| {}));
        let cold = Instant::now();
        let runtime = SpeechRuntime::load(&engine.models, 2, false).unwrap();
        let cold_ms = cold.elapsed().as_secs_f64() * 1000.0;
        lock(&engine.warm_speech).put(0, 2, runtime);
        let hot = Instant::now();
        let (_, runtime) = lock(&engine.warm_speech).take(2);
        assert!(runtime.is_some());
        engine.models.verify_vad().unwrap();
        let _vad = Segmenter::new(&engine.models.vad_path()).unwrap();
        let warm_ms = hot.elapsed().as_secs_f64() * 1000.0;
        let ready = Instant::now();
        let _ = engine.readiness_for_dictation().unwrap();
        println!("dictation startup: cold_model_ms={cold_ms:.1} warm_model_and_vad_ms={warm_ms:.1} device_readiness_ms={:.1}", ready.elapsed().as_secs_f64()*1000.0);
    }
    #[test]
    fn folder_paths_reject_traversal() {
        let temp = tempfile::tempdir().unwrap();
        for id in ["../x", "x/y", "", "x\\y"] {
            assert!(meeting_audio_dir(temp.path(), id).is_err());
        }
        assert!(meeting_audio_dir(temp.path(), "meeting-123").is_ok());
    }
    #[test]
    fn full_inference_queue_preserves_audio_first() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("microphone.wav");
        let mut writer = Writer::create(&path).unwrap();
        let (tx, _rx) = mpsc::sync_channel(1);
        save_and_enqueue(&mut writer, &tx, "microphone", vec![0.1; 1600], 0.0).unwrap();
        assert!(save_and_enqueue(&mut writer, &tx, "microphone", vec![0.2; 1600], 0.1).is_err());
        drop(writer);
        assert_eq!(hound::WavReader::open(path).unwrap().duration(), 3200);
    }
    #[test]
    fn stop_drains_before_returning() {
        struct Fake {
            seen: Vec<f64>,
            flushed: bool,
        }
        impl Pipeline for Fake {
            fn feed(&mut self, _: &'static str, _: &[f32], start: f64) -> Result<()> {
                self.seen.push(start);
                Ok(())
            }
            fn flush(&mut self) -> Result<()> {
                self.flushed = true;
                Ok(())
            }
        }
        let (tx, rx) = mpsc::sync_channel(4);
        for n in 0..3 {
            tx.send(Work::Audio {
                source: "system",
                samples: vec![],
                start: n as f64,
            })
            .unwrap();
        }
        tx.send(Work::Finish).unwrap();
        let mut fake = Fake {
            seen: vec![],
            flushed: false,
        };
        inference_loop(rx, &mut fake).unwrap();
        assert_eq!(fake.seen, vec![0.0, 1.0, 2.0]);
        assert!(fake.flushed);
    }
    #[test]
    fn callback_failure_propagates() {
        struct Fake;
        impl Pipeline for Fake {
            fn feed(&mut self, _: &'static str, _: &[f32], _: f64) -> Result<()> {
                Err(error("persistence failed"))
            }
            fn flush(&mut self) -> Result<()> {
                Ok(())
            }
        }
        let (tx, rx) = mpsc::sync_channel(1);
        tx.send(Work::Audio {
            source: "system",
            samples: vec![],
            start: 0.0,
        })
        .unwrap();
        assert_eq!(
            inference_loop(rx, &mut Fake).unwrap_err().message,
            "persistence failed"
        );
    }
    #[test]
    fn pause_excludes_wall_time() {
        let mut state = State {
            accepting: true,
            epoch: Some(Instant::now() - Duration::from_secs(2)),
            ..Default::default()
        };
        state.deactivate();
        let elapsed = state.elapsed();
        state.epoch = Some(Instant::now() - Duration::from_secs(100));
        assert_eq!(state.elapsed(), elapsed);
    }
    #[test]
    fn existing_audio_is_never_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("meeting");
        fs::create_dir(&path).unwrap();
        fs::write(path.join("system.wav"), b"evidence").unwrap();
        assert!(new_audio_dir(&path).is_err());
        assert_eq!(fs::read(path.join("system.wav")).unwrap(), b"evidence");
    }
    #[test]
    fn completed_inference_failure_releases_session() {
        let temp = tempfile::tempdir().unwrap();
        let engine =
            CaptureEngine::new(temp.path().into(), temp.path().into(), Arc::new(|_, _| {}));
        let evidence = temp.path().join("audio/failed/system.wav");
        fs::create_dir_all(evidence.parent().unwrap()).unwrap();
        fs::write(&evidence, b"evidence").unwrap();
        *lock(&engine.context.state) = State {
            meeting: Some("failed".into()),
            state: "interrupted",
            error: Some(error("inference failed")),
            ..Default::default()
        };
        let (control, rx) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let acquisition = thread::spawn(move || {
            if let Ok(Control::Stop(reply)) = rx.recv() {
                let _ = reply.send(Err(error("inference disconnected")));
            }
        });
        let inference = thread::spawn(move || {
            let _ = done_tx.send(Err(error("inference failed")));
        });
        *lock(&engine.session) = Some(Session {
            control,
            acquisition: Some(acquisition),
            inference: Some(inference),
            done,
        });
        assert!(engine.stop().is_err());
        assert!(lock(&engine.session).is_none());
        assert!(lock(&engine.context.state).meeting.is_none());
        assert_eq!(fs::read(evidence).unwrap(), b"evidence");
    }
    #[test]
    fn disconnected_inference_still_preserves_all_buffered_capture() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("system.wav");
        let mut writers = HashMap::from([("system", Writer::create(&path).unwrap())]);
        let (raw_tx, raw_rx) = mpsc::sync_channel(4);
        let (work_tx, work_rx) = mpsc::sync_channel(1);
        drop(work_rx);
        for n in 0..3 {
            raw_tx
                .send(Frame {
                    source: "system",
                    samples: vec![0.1; 100],
                    rate: 16000,
                    start: n as f64 / 160.0,
                })
                .unwrap();
        }
        let context = Context {
            state: Arc::new(Mutex::new(State::default())),
            emit: Arc::new(|_, _| {}),
        };
        assert!(drain_inputs(
            &raw_rx,
            &mut writers,
            &work_tx,
            &context,
            &mut HashMap::new(),
            &mut Instant::now()
        )
        .is_err());
        drop(writers);
        assert_eq!(hound::WavReader::open(path).unwrap().duration(), 300);
    }
    #[cfg(unix)]
    #[test]
    fn process_capture_preserves_partial_pcm_and_closes_hardware() {
        let state = Arc::new(Mutex::new(State {
            meeting: Some("fake".into()),
            state: "recording",
            accepting: true,
            epoch: Some(Instant::now()),
            ..Default::default()
        }));
        let context = Context {
            state: state.clone(),
            emit: Arc::new(|_, _| {}),
        };
        let (tx, rx) = mpsc::sync_channel(4);
        let mut command = Command::new("sh");
        command.args([
            "-c",
            "printf 'READY\\n' >&2; printf '\\000'; printf '\\000\\200\\076'; cat >/dev/null",
        ]);
        let source = start_process(command, true, context, tx).unwrap();
        let frame = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(frame.samples, vec![0.25]);
        lock(&state).deactivate();
        source.stop();
        assert!(lock(&state).error.is_none());
    }
    #[cfg(unix)]
    #[test]
    fn ready_then_exit_during_start_or_resume_is_a_failure() {
        for phase in ["starting", "paused"] {
            let state = Arc::new(Mutex::new(State {
                meeting: Some("fake".into()),
                state: phase,
                ..Default::default()
            }));
            let context = Context {
                state: state.clone(),
                emit: Arc::new(|_, _| {}),
            };
            let (tx, _rx) = mpsc::sync_channel(4);
            let mut command = Command::new("sh");
            command.args(["-c", "printf 'READY\\n' >&2"]);
            let source = start_process(command, true, context.clone(), tx).unwrap();
            let deadline = Instant::now() + Duration::from_secs(2);
            while lock(&state).error.is_none() && Instant::now() < deadline {
                thread::yield_now();
            }
            assert!(
                context.activation().is_err(),
                "dead source activated from {phase}"
            );
            source.stop();
        }
    }
    /// Explicit integration check. Reuses only pinned public fixture/installed models.
    #[test]
    #[ignore = "requires explicitly provisioned local speech weights"]
    fn real_speech_fixture() {
        let directory =
            PathBuf::from(std::env::var_os("TUCK_VERIFY_DATA").expect("TUCK_VERIFY_DATA"));
        let fixture = PathBuf::from(
            std::env::var_os("TUCK_VERIFY_FIXTURE")
                .unwrap_or_else(|| "/tmp/tuck-verification/en.wav".into()),
        );
        let engine = CaptureEngine::new(directory.clone(), directory.clone(), Arc::new(|_, _| {}));
        let segments = Arc::new(Mutex::new(Vec::new()));
        let recorded = segments.clone();
        let meeting = format!("native-verify-{}", uuid::Uuid::new_v4());
        let start = Instant::now();
        let duration = engine
            .import_file(
                fixture.to_str().unwrap(),
                &meeting,
                Arc::new(move |segment| {
                    lock(&recorded).push(segment);
                    Ok(())
                }),
                2,
            )
            .unwrap();
        let transcript = lock(&segments)
            .iter()
            .map(|v| v["text"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            transcript.to_lowercase().contains("ask not"),
            "{transcript}"
        );
        let expected_duration = std::env::var("TUCK_VERIFY_DURATION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(3.845);
        assert!(
            (duration - expected_duration).abs() < 0.002,
            "Expected {expected_duration}s, decoded {duration}s"
        );
        assert!(
            lock(&segments)
                .iter()
                .all(|v| v["start"].as_f64().unwrap() >= 0.0
                    && v["end"].as_f64().unwrap() <= duration)
        );
        assert_eq!(engine.status()["activeMeetingId"], Value::Null);
        println!(
            "{}",
            json!({"duration":duration,"elapsed":start.elapsed().as_secs_f64(),"segments":*lock(&segments),"preserved":directory.join("audio").join(meeting).join("import.wav")})
        );
    }
}
/// Enumerate identifiers without opening a stream or requesting microphone access.
pub fn capture_devices() -> Result<Value> {
    let host = cpal::default_host();
    let default = host
        .default_input_device()
        .and_then(|d| d.id().ok())
        .map(|id| id.to_string());
    let microphones: Vec<Value> = host.input_devices()
        .map_err(|_| error("Microphones could not be listed. Reconnect your device and try again."))?
        .filter_map(|device| Some(json!({"id":device.id().ok()?.to_string(),"name":device.description().ok()?.name(),"isDefault":Some(device.id().ok()?.to_string()) == default})))
        .take(128).collect();
    #[cfg(target_os = "windows")]
    let system_outputs: Vec<Value> = {
        let default = host
            .default_output_device()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());
        host.output_devices().map_err(|_| error("System outputs could not be listed."))?
            .filter_map(|d|Some(json!({"id":d.id().ok()?.to_string(),"name":d.description().ok()?.name(),"isDefault":Some(d.id().ok()?.to_string())==default}))).take(128).collect()
    };
    #[cfg(target_os = "macos")]
    let system_outputs =
        vec![json!({"id":"system-mix","name":"All system audio","isDefault":true})];
    #[cfg(target_os = "linux")]
    let system_outputs = vec![
        json!({"id":"@DEFAULT_MONITOR@","name":"Default PulseAudio / PipeWire monitor","isDefault":true}),
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let system_outputs: Vec<Value> = vec![];
    Ok(json!({"microphones":microphones,"systemOutputs":system_outputs}))
}
#[derive(Default)]
struct Meter {
    peak: f64,
    squares: f64,
    count: u64,
}
impl Meter {
    fn feed(&mut self, samples: &[f32]) -> f64 {
        let mut squares = 0.0;
        for &sample in samples {
            let v = if sample.is_finite() {
                (sample as f64).clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.peak = self.peak.max(v.abs());
            squares += v * v;
        }
        self.squares += squares;
        self.count += samples.len() as u64;
        if samples.is_empty() {
            0.0
        } else {
            (squares / samples.len() as f64).sqrt()
        }
    }
    fn result(&self) -> Value {
        let rms = if self.count == 0 {
            0.0
        } else {
            (self.squares / self.count as f64).sqrt()
        };
        json!({"peak":self.peak,"rms":rms,"heard":self.peak > 0.005,"samples":self.count})
    }
}
impl CaptureEngine {
    pub fn readiness(&self) -> Result<Value> {
        self.readiness_for_sources(true)
    }
    pub fn readiness_for_dictation(&self) -> Result<Value> {
        self.readiness_for_sources(false)
    }
    fn readiness_for_sources(&self, system_audio: bool) -> Result<Value> {
        let devices = capture_devices()?;
        let selected = lock(&self.devices).clone();
        let mic_present = devices["microphones"].as_array().is_some_and(|v| {
            v.iter()
                .any(|d| selected.microphone.is_empty() || d["id"] == selected.microphone)
        });
        let system_present = devices["systemOutputs"].as_array().is_some_and(|v| {
            v.iter()
                .any(|d| selected.system.is_empty() || d["id"] == selected.system)
        });
        let model_ready = self.models.installed("parakeet");
        let helper_ready = !cfg!(target_os = "macos")
            || (helper_path("tuck-calendar", "TUCK_CALENDAR_HELPER").is_ok()
                && (!system_audio || helper_path("tuck-audio", "TUCK_AUDIO_HELPER").is_ok())
                && (!self.models.apple_installed()
                    || helper_path("tuck-speech", "TUCK_SPEECH_HELPER").is_ok()));
        let mut issues = vec![];
        if !model_ready {
            issues.push(json!({"code":"model_missing","message":"Install the local speech model in Settings before recording."}));
        }
        if !helper_ready {
            issues.push(json!({"code":"helper_missing","message":"A native audio or speech helper is missing. Rebuild or reinstall Tuck."}));
        }
        if !mic_present {
            issues.push(json!({"code":"microphone_missing","message":"The selected microphone is unavailable. Choose another device."}));
        }
        if system_audio && !system_present {
            issues.push(json!({"code":"system_missing","message":"The selected system audio source is unavailable."}));
        }
        // Status only: the helper must never request permission for readiness.
        #[cfg(target_os = "macos")]
        let permissions = super::calendar::run_helper(if system_audio { "permissions" } else { "microphone-permission" }, &[], Duration::from_secs(3))
            .unwrap_or_else(|_| json!({"microphone":"unknown","system":false}));
        #[cfg(not(target_os = "macos"))]
        let permissions = json!({"microphone":"unknown","system":true});
        if permissions["microphone"] != "authorized" && cfg!(target_os = "macos") {
            issues.push(json!({"code":"microphone_permission","message":"Microphone permission is not confirmed. Use Test audio to request it, or allow Tuck in System Settings → Privacy & Security → Microphone."}));
        }
        if system_audio && permissions["system"] == false && cfg!(target_os = "macos") {
            issues.push(json!({"code":"system_permission","message":"System audio permission is not confirmed. Use Test audio or allow Tuck in System Settings → Privacy & Security → Screen & System Audio Recording."}));
        }
        Ok(
            json!({"modelReady":model_ready,"helperReady":helper_ready,"microphoneAvailable":mic_present && (!cfg!(target_os="macos") || permissions["microphone"] == "authorized"),"systemAvailable":system_present && (!cfg!(target_os="macos") || permissions["system"] == true),"permissions":permissions,"issues":issues}),
        )
    }
    /// Explicit five-second level check. Frames only enter bounded memory and are discarded.
    /// This deliberately creates neither a Writer nor a SpeechRuntime.
    pub fn test_audio(&self, microphone: bool, system: bool) -> Result<Value> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| error("Another audio operation is in progress."))?;
        if lock(&self.session).is_some() {
            return Err(error("Stop the recording before testing audio."));
        }
        if !microphone && !system {
            return Err(error("Choose at least one source to test."));
        }
        #[cfg(target_os = "macos")]
        if microphone {
            let permission =
                super::calendar::run_helper("request-microphone", &[], Duration::from_secs(60))?;
            if permission["granted"] != true {
                return Err(error("Microphone access was not granted. Allow Tuck in System Settings → Privacy & Security → Microphone, then try Test audio again."));
            }
        }
        let devices = lock(&self.devices).clone();
        let emit = self.context.emit.clone();
        // CPAL device and stream handles live and die on this owner thread.
        thread::spawn(move || -> Result<Value> {
            let context = Context { state: Arc::new(Mutex::new(State {meeting:Some("audio-test".into()),state:"starting",..Default::default()})), emit: Arc::new(|_,_|{}) };
            let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
            let mut sources = start_sources(system,microphone,&devices,&context,&tx)?;
            if let Err(e) = context.activation() { stop_sources(&mut sources); return Err(e); }
            let mut mic = Meter::default(); let mut output = Meter::default();
            let started = Instant::now(); let mut last_event = Instant::now();
            let mut levels = HashMap::new();
            while started.elapsed() < Duration::from_secs(5) {
                if let Ok(frame) = rx.recv_timeout(Duration::from_millis(20)) {
                    let meter = if frame.source == "microphone" { &mut mic } else { &mut output };
                    levels.insert(frame.source, (meter.feed(&frame.samples)*4.0).min(1.0));
                }
                if last_event.elapsed() >= Duration::from_millis(100) {
                    emit("audio_test_level",json!({"microphone":levels.get("microphone").copied().unwrap_or(0.0),"system":levels.get("system").copied().unwrap_or(0.0),"elapsed":started.elapsed().as_secs_f64()}));
                    last_event = Instant::now(); levels.clear();
                }
            }
            lock(&context.state).deactivate(); stop_sources(&mut sources);
            if let Some(failure) = lock(&context.state).error.clone() { return Err(failure); }
            let mut warnings = vec![];
            if microphone && mic.result()["heard"] != true { warnings.push("No microphone signal was heard. Check the device, mute switch, and microphone permission."); }
            if system && output.result()["heard"] != true { warnings.push("No system audio was heard. Play some meeting audio and check system audio permission."); }
            let result = json!({"duration":5,"microphone":mic.result(),"system":output.result(),"warnings":warnings});
            emit("audio_test_complete",result.clone());
            Ok(result)
        }).join().map_err(|_|error("The audio test could not finish. Try reconnecting your device."))?
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::*;
    #[test]
    fn test_meter_discards_invalid_samples_and_has_no_storage_dependency() {
        let mut meter = Meter::default();
        let rms = meter.feed(&[f32::NAN, f32::INFINITY, 0.5, -0.5]);
        assert!((rms - (0.5_f64 / 4.0).sqrt()).abs() < 0.00001);
        assert_eq!(meter.result()["peak"], 0.5);
        assert_eq!(meter.result()["heard"], true);
        assert_eq!(meter.result()["samples"], 4);
        assert_eq!(Meter::default().result()["heard"], false);
    }
    #[test]
    fn silence_is_a_soft_warning_only_after_thirty_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let events = Arc::new(Mutex::new(vec![]));
        let observed = events.clone();
        let context = Context {
            state: Arc::new(Mutex::new(State {
                meeting: Some("fixture".into()),
                state: "recording",
                accepting: true,
                accumulated: 31.0,
                ..Default::default()
            })),
            emit: Arc::new(move |event, payload| lock(&observed).push((event.to_owned(), payload))),
        };
        let mut writers = HashMap::new();
        writers.insert(
            "microphone",
            Writer::create(&dir.path().join("fixture.wav")).unwrap(),
        );
        let (tx, rx) = mpsc::sync_channel(4);
        let mut levels = HashMap::new();
        let mut last = Instant::now();
        for _ in 0..2 {
            process_frame(
                Frame {
                    source: "microphone",
                    samples: vec![0.0; 1600],
                    rate: 16000,
                    start: 0.0,
                },
                &mut writers,
                &tx,
                &context,
                &mut levels,
                &mut last,
            )
            .unwrap();
        }
        let events = lock(&events);
        assert_eq!(
            events
                .iter()
                .filter(|(e, _)| e == "recording_warning")
                .count(),
            1
        );
        assert!(lock(&context.state).accepting);
        assert!(lock(&context.state).error.is_none());
        assert!(rx.try_recv().is_ok());
    }
}

#[cfg(test)]
mod quiet_turn_tests {
    use super::*;
    #[test]
    fn one_silent_source_does_not_warn_while_another_has_sound() {
        let dir = tempfile::tempdir().unwrap();
        let observed = Arc::new(Mutex::new(Vec::new()));
        let events = observed.clone();
        let context = Context {
            state: Arc::new(Mutex::new(State {
                meeting: Some("fixture".into()),
                state: "recording",
                accepting: true,
                accumulated: 31.0,
                ..Default::default()
            })),
            emit: Arc::new(move |event, payload| lock(&events).push((event.to_owned(), payload))),
        };
        let mut writers = HashMap::new();
        for source in ["microphone", "system"] {
            writers.insert(
                source,
                Writer::create(&dir.path().join(format!("{source}.wav"))).unwrap(),
            );
        }
        let (tx, _rx) = mpsc::sync_channel(8);
        let mut levels = HashMap::new();
        let mut last = Instant::now();
        for (source, signal) in [("system", 0.2), ("microphone", 0.0), ("microphone", 0.0)] {
            process_frame(
                Frame {
                    source,
                    samples: vec![signal; 1600],
                    rate: 16000,
                    start: 0.0,
                },
                &mut writers,
                &tx,
                &context,
                &mut levels,
                &mut last,
            )
            .unwrap();
        }
        assert!(!lock(&observed)
            .iter()
            .any(|(event, _)| event == "recording_warning"));
        // A new 30-second quiet period warns once, then any signal resets it.
        lock(&context.state).accumulated = 62.0;
        for _ in 0..2 {
            process_frame(
                Frame {
                    source: "microphone",
                    samples: vec![0.0; 1600],
                    rate: 16000,
                    start: 0.0,
                },
                &mut writers,
                &tx,
                &context,
                &mut levels,
                &mut last,
            )
            .unwrap();
        }
        assert_eq!(
            lock(&observed)
                .iter()
                .filter(|(event, _)| event == "recording_warning")
                .count(),
            1
        );
        process_frame(
            Frame {
                source: "microphone",
                samples: vec![0.2; 1600],
                rate: 16000,
                start: 0.0,
            },
            &mut writers,
            &tx,
            &context,
            &mut levels,
            &mut last,
        )
        .unwrap();
        lock(&context.state).accumulated = 93.0;
        process_frame(
            Frame {
                source: "system",
                samples: vec![0.0; 1600],
                rate: 16000,
                start: 0.0,
            },
            &mut writers,
            &tx,
            &context,
            &mut levels,
            &mut last,
        )
        .unwrap();
        assert_eq!(
            lock(&observed)
                .iter()
                .filter(|(event, _)| event == "recording_warning")
                .count(),
            2
        );
    }
}
