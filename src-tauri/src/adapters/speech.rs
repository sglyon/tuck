//! Local speech engines. One inference worker owns these handles; no shared mutable ASR.
use super::models::{self, BackendModels, ModelManager};
use serde_json::{json, Value};
use sherpa_onnx::{
    LinearResampler, OfflineRecognizer, OfflineRecognizerConfig, SpeakerEmbeddingExtractor,
    SpeakerEmbeddingExtractorConfig, VadModelConfig, VoiceActivityDetector,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
use tuck_core::{ports::Result, AppError};
pub const SAMPLE_RATE: u32 = 16_000;
fn error(message: impl Into<String>) -> AppError {
    AppError::new("speech_error", message)
}

pub fn helper_path(name: &str, override_var: &str) -> Result<PathBuf> {
    let mut candidates = vec![];
    if let Some(value) = std::env::var_os(override_var) {
        candidates.push(PathBuf::from(value));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.extend([
                dir.join(name),
                dir.join("binaries").join(name),
                dir.join("../Resources").join(name),
                dir.join("../Resources/binaries").join(name),
            ]);
        }
    }
    candidates.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join(name),
    );
    candidates.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        error(format!(
            "The native {name} helper is missing. Rebuild the native helpers and restart Tuck."
        ))
    })
}
fn model_directory(directory: &Path) -> Result<PathBuf> {
    // Setup and inference must address the same physical model directory, even
    // when an application-data location is reached through a symbolic link.
    let path = std::fs::canonicalize(directory)
        .map_err(|_| error("The local Apple speech model directory is unavailable."))?;
    if !path.is_dir() {
        return Err(error("The local Apple speech model directory is invalid."));
    }
    Ok(path)
}
pub fn prepare_apple(directory: &Path) -> Result<()> {
    let directory = model_directory(directory)?;
    let mut child = Command::new(helper_path("tuck-speech", "TUCK_SPEECH_HELPER")?)
        .arg("--models")
        .arg(directory)
        .arg("--prepare")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| error("The Apple speech helper could not start."))?;
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(error("Apple speech optimization failed. Reinstall the model and check available storage."))
                }
            }
            Ok(None) => {}
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error("Apple speech optimization was interrupted."));
            }
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error(
                "Apple speech optimization timed out. Retry installation.",
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
struct AppleRecognizer {
    child: Child,
    stdin: ChildStdin,
    responses: Receiver<Value>,
    id: u64,
}
impl AppleRecognizer {
    fn load(directory: &Path) -> Result<Self> {
        let directory = model_directory(directory)?;
        let mut child = Command::new(helper_path("tuck-speech", "TUCK_SPEECH_HELPER")?)
            .arg("--models")
            .arg(directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                error("Apple speech could not start. Check the native helper installation.")
            })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| error("Apple speech has no input pipe."))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| error("Apple speech has no output pipe."))?;
        let (tx, responses) = mpsc::sync_channel(4);
        std::thread::Builder::new()
            .name("tuck-coreml-replies".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let mut line = String::new();
                    match reader.by_ref().take(65_537).read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        _ => {}
                    }
                    if line.len() > 65_536 {
                        break;
                    }
                    if let Some(payload) = line.trim().strip_prefix("TUCK_SPEECH:") {
                        match serde_json::from_str::<Value>(payload) {
                            Ok(value) => {
                                if tx.send(value).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
            })
            .map_err(|_| error("The Apple speech response worker could not start."))?;
        let engine = Self {
            child,
            stdin,
            responses,
            id: 0,
        };
        let ready = engine
            .responses
            .recv_timeout(Duration::from_secs(180))
            .map_err(|_| {
                error(
                    "Apple speech did not become ready. Install or optimize the model in Settings.",
                )
            })?;
        if ready["event"] != "ready" || ready.get("error").is_some() {
            return Err(error(
                "The installed Apple speech model could not load. Reinstall it in Settings.",
            ));
        }
        Ok(engine)
    }
    fn transcribe(&mut self, samples: &[f32]) -> Result<String> {
        if samples.len() > 240_000 {
            return Err(error("The speech segment exceeded the local engine limit."));
        }
        self.id += 1;
        let header = json!({"id":self.id,"op":"transcribe","samples":samples.len()});
        writeln!(self.stdin, "{header}")
            .map_err(|_| error("Apple speech was interrupted. Audio has been preserved."))?;
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        self.stdin
            .write_all(&bytes)
            .and_then(|_| self.stdin.flush())
            .map_err(|_| error("Apple speech was interrupted. Audio has been preserved."))?;
        let response = self
            .responses
            .recv_timeout(Duration::from_secs(120))
            .map_err(|_| {
                error("Apple speech did not finish. Audio has been preserved for recovery.")
            })?;
        if response["id"].as_u64() != Some(self.id) || response.get("error").is_some() {
            return Err(error(
                "Apple speech could not transcribe this segment. Audio has been preserved.",
            ));
        }
        response["text"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| error("Apple speech returned an invalid result."))
    }
}
impl Drop for AppleRecognizer {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "{{\"op\":\"shutdown\"}}");
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
enum Recognizer {
    Cpu(OfflineRecognizer),
    Apple(AppleRecognizer),
}
pub struct SpeechRuntime {
    recognizer: Recognizer,
    extractor: Option<SpeakerEmbeddingExtractor>,
    clusters: SpeakerClusters,
}
fn load_speaker_extractor(
    models: &ModelManager,
    enabled: bool,
) -> Result<Option<SpeakerEmbeddingExtractor>> {
    if !enabled || !models.installed("speakers") {
        return Ok(None);
    }
    models.verify_speaker()?;
    let config = SpeakerEmbeddingExtractorConfig {
        model: Some(models.speaker_path().to_string_lossy().into_owned()),
        ..Default::default()
    };
    Ok(Some(
        SpeakerEmbeddingExtractor::create(&config)
            .ok_or_else(|| error("The speaker model could not load. Reinstall it in Settings."))?,
    ))
}
impl SpeechRuntime {
    pub fn is_alive(&mut self) -> bool {
        match &mut self.recognizer {
            Recognizer::Apple(engine) => engine.child.try_wait().is_ok_and(|status| status.is_none()),
            Recognizer::Cpu(_) => true,
        }
    }
    pub fn reset_session(&mut self) {
        self.clusters = SpeakerClusters::default();
    }
    pub fn load(models: &ModelManager, threads: u32, diarization: bool) -> Result<Self> {
        let recognizer = match models.verify()? {
            BackendModels::Apple(path) => Recognizer::Apple(AppleRecognizer::load(&path)?),
            BackendModels::Cpu(path) => {
                let mut config = OfflineRecognizerConfig::default();
                config.model_config.transducer.encoder = Some(
                    path.join("encoder.int8.onnx")
                        .to_string_lossy()
                        .into_owned(),
                );
                config.model_config.transducer.decoder = Some(
                    path.join("decoder.int8.onnx")
                        .to_string_lossy()
                        .into_owned(),
                );
                config.model_config.transducer.joiner =
                    Some(path.join("joiner.int8.onnx").to_string_lossy().into_owned());
                config.model_config.tokens =
                    Some(path.join("tokens.txt").to_string_lossy().into_owned());
                config.model_config.model_type = Some("nemo_transducer".into());
                config.model_config.provider = Some("cpu".into());
                config.model_config.num_threads = threads.clamp(1, 4) as i32;
                config.decoding_method = Some("greedy_search".into());
                Recognizer::Cpu(OfflineRecognizer::create(&config).ok_or_else(|| {
                    error("The local speech model could not load. Reinstall it in Settings.")
                })?)
            }
        };
        let extractor = load_speaker_extractor(models, diarization)?;
        Ok(Self {
            recognizer,
            extractor,
            clusters: SpeakerClusters::default(),
        })
    }
    pub fn transcribe(
        &mut self,
        samples: &[f32],
        start: f64,
        source: &str,
        meeting: &str,
    ) -> Result<Option<Value>> {
        let text = match &mut self.recognizer {
            Recognizer::Apple(engine) => engine.transcribe(samples)?,
            Recognizer::Cpu(engine) => {
                let stream = engine.create_stream();
                stream.accept_waveform(SAMPLE_RATE as i32, samples);
                engine.decode(&stream);
                stream
                    .get_result()
                    .ok_or_else(|| error("The local speech engine returned no result."))?
                    .text
            }
        };
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let mut speaker = if source == "microphone" {
            "you".into()
        } else {
            "speaker-1".into()
        };
        if source != "microphone" && samples.len() >= SAMPLE_RATE as usize {
            if let Some(extractor) = &self.extractor {
                let stream = extractor
                    .create_stream()
                    .ok_or_else(|| error("The speaker extractor could not create a stream."))?;
                stream.accept_waveform(SAMPLE_RATE as i32, samples);
                stream.input_finished();
                if extractor.is_ready(&stream) {
                    let embedding = extractor
                        .compute(&stream)
                        .ok_or_else(|| error("The speaker extractor returned no embedding."))?;
                    speaker = self.clusters.identify(embedding);
                }
            }
        }
        Ok(Some(
            json!({"id":uuid::Uuid::new_v4().to_string(),"meetingId":meeting,"start":(start.max(0.0)*1000.0).round()/1000.0,"end":((start.max(0.0)+samples.len()as f64/SAMPLE_RATE as f64)*1000.0).round()/1000.0,"text":text,"speakerId":speaker,"source":source}),
        ))
    }
}
#[derive(Default)]
struct SpeakerClusters {
    centers: Vec<Vec<f32>>,
    counts: Vec<usize>,
}
fn normalize(values: &mut [f32]) -> bool {
    let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= 1e-8 {
        return false;
    }
    for value in values {
        *value /= norm;
    }
    true
}
impl SpeakerClusters {
    fn identify(&mut self, mut values: Vec<f32>) -> String {
        if !normalize(&mut values) {
            return "speaker-1".into();
        }
        let best = self
            .centers
            .iter()
            .enumerate()
            .filter(|(_, center)| center.len() == values.len())
            .map(|(i, center)| {
                (
                    i,
                    center.iter().zip(&values).map(|(a, b)| a * b).sum::<f32>(),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((index, score)) = best {
            if score >= 0.65 || self.centers.len() >= 12 {
                let count = self.counts[index].min(20) as f32;
                for (center, value) in self.centers[index].iter_mut().zip(values) {
                    *center = *center * count + value;
                }
                normalize(&mut self.centers[index]);
                self.counts[index] += 1;
                return format!("speaker-{}", index + 1);
            }
        }
        self.centers.push(values);
        self.counts.push(1);
        format!("speaker-{}", self.centers.len())
    }
}
pub struct SpeechChunk {
    pub samples: Vec<f32>,
    pub start: f64,
}
pub struct Segmenter {
    vad: VoiceActivityDetector,
    pending: Vec<f32>,
    offset: Option<f64>,
    total: usize,
}
impl Segmenter {
    pub fn new(model: &Path) -> Result<Self> {
        let mut config = VadModelConfig::default();
        config.silero_vad.model = Some(model.to_string_lossy().into_owned());
        config.silero_vad.threshold = 0.5;
        config.silero_vad.window_size = 512;
        config.silero_vad.min_silence_duration = 0.45;
        config.silero_vad.min_speech_duration = 0.25;
        config.silero_vad.max_speech_duration = 5.0;
        config.sample_rate = SAMPLE_RATE as i32;
        config.num_threads = 1;
        config.provider = Some("cpu".into());
        Ok(Self {
            vad: VoiceActivityDetector::create(&config, 20.0)
                .ok_or_else(|| models::error("The speech activity model could not load."))?,
            pending: vec![],
            offset: None,
            total: 0,
        })
    }
    pub fn feed(&mut self, samples: &[f32], start: f64) -> Vec<SpeechChunk> {
        self.offset.get_or_insert(start);
        self.total += samples.len();
        self.pending.extend_from_slice(samples);
        let mut result = vec![];
        let mut consumed = 0;
        while self.pending.len() - consumed >= 512 {
            self.vad
                .accept_waveform(&self.pending[consumed..consumed + 512]);
            consumed += 512;
            self.drain(&mut result);
        }
        self.pending.drain(..consumed);
        result
    }
    fn drain(&self, result: &mut Vec<SpeechChunk>) {
        while let Some(front) = self.vad.front() {
            let start = front.start().max(0) as usize;
            let length = front.samples().len().min(self.total.saturating_sub(start));
            if length > 0 {
                result.push(SpeechChunk {
                    samples: front.samples()[..length].to_vec(),
                    start: self.offset.unwrap_or(0.0) + start as f64 / SAMPLE_RATE as f64,
                });
            }
            self.vad.pop();
        }
    }
    pub fn flush(&mut self) -> Vec<SpeechChunk> {
        if !self.pending.is_empty() {
            self.pending.resize(512, 0.0);
            self.vad.accept_waveform(&self.pending);
            self.pending.clear();
        }
        self.vad.flush();
        let mut result = vec![];
        self.drain(&mut result);
        self.vad.reset();
        self.offset = None;
        self.total = 0;
        result
    }
}
/// Sherpa's band-limited native resampler preserves filter state between hardware blocks.
pub struct PcmResampler {
    inner: Option<LinearResampler>,
}
impl PcmResampler {
    pub fn new(rate: u32) -> Result<Self> {
        if !(8_000..=192_000).contains(&rate) {
            return Err(error("Use an audio sample rate between 8 kHz and 192 kHz."));
        }
        Ok(Self {
            inner: if rate == SAMPLE_RATE {
                None
            } else {
                Some(
                    LinearResampler::create(rate as i32, SAMPLE_RATE as i32)
                        .ok_or_else(|| error("The audio resampler could not start."))?,
                )
            },
        })
    }
    pub fn feed(&self, samples: &[f32], last: bool) -> Vec<f32> {
        let mut output = match &self.inner {
            Some(inner) => inner.resample(samples, last),
            None => samples.to_vec(),
        };
        // Band-limited resampling can overshoot full scale around a transient.
        // Keep the preserved PCM and the native helper's validated input consistent.
        for sample in &mut output {
            *sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_speaker_recognition_does_not_verify_or_load_an_installed_model() {
        let root = tempfile::tempdir().unwrap();
        let models = ModelManager::new(root.path(), std::sync::Arc::new(|_, _| {}));
        let path = models.speaker_path();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // An installed-size corrupt model would normally fail integrity checks.
        std::fs::File::create(path)
            .unwrap()
            .set_len(29_596_978)
            .unwrap();
        assert!(models.installed("speakers"));
        assert!(load_speaker_extractor(&models, false).unwrap().is_none());
        assert!(load_speaker_extractor(&models, true).is_err());
    }
    #[cfg(unix)]
    #[test]
    fn model_aliases_use_the_same_physical_directory() {
        let root = tempfile::tempdir().unwrap();
        let actual = root.path().join("verified-models");
        std::fs::create_dir(&actual).unwrap();
        for name in ["setup-path", "recording-path"] {
            let alias = root.path().join(name);
            std::os::unix::fs::symlink(&actual, &alias).unwrap();
            assert_eq!(
                model_directory(&alias).unwrap(),
                model_directory(&actual).unwrap()
            );
        }
        let file = root.path().join("not-a-directory");
        std::fs::write(&file, b"invalid").unwrap();
        assert!(model_directory(&file).is_err());
        assert!(model_directory(&root.path().join("missing")).is_err());
    }
    #[test]
    fn provisional_speakers_are_bounded() {
        let mut clusters = SpeakerClusters::default();
        assert_eq!(clusters.identify(vec![1.0, 0.0]), "speaker-1");
        assert_eq!(clusters.identify(vec![0.99, 0.01]), "speaker-1");
        assert_eq!(clusters.identify(vec![-1.0, 0.0]), "speaker-2");
        assert_eq!(clusters.identify(vec![f32::NAN]), "speaker-1");
        for i in 0..30 {
            let mut v = vec![0.0; 30];
            v[i] = 1.0;
            clusters.identify(v);
        }
        assert!(clusters.centers.len() <= 12);
    }
    #[test]
    fn resampler_is_streaming_and_bounded() {
        let resampler = PcmResampler::new(48000).unwrap();
        let mut out = vec![];
        for i in 0..10 {
            out.extend(resampler.feed(&vec![0.25; 4800], i == 9));
        }
        assert!(out.len().abs_diff(16000) <= 1);
        assert!(out.iter().all(|n| n.is_finite()));
        assert!(PcmResampler::new(0).is_err());
    }
}
