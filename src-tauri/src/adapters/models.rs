//! Explicit, checksum-pinned local model provisioning. Status never uses the network.
use bzip2::read::BzDecoder;
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tuck_core::{
    ports::{Emit, Result},
    AppError,
};

pub const PARAKEET_NAME: &str = "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8";
pub const CPU_FILES: &[(&str, &str)] = &[
    (
        "encoder.int8.onnx",
        "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
    ),
    (
        "decoder.int8.onnx",
        "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
    ),
    (
        "joiner.int8.onnx",
        "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
    ),
    (
        "tokens.txt",
        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    ),
];
const BASE: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
const VAD_HASH: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
const SPEAKER_HASH: &str = "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b";
pub const SPEAKER_FILE: &str = "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx";

pub fn error(message: impl Into<String>) -> AppError {
    AppError::new("model_error", message)
}
fn io_error(_: impl std::fmt::Display) -> AppError {
    error("The local model files could not be read or written. Check available storage and reinstall the model.")
}
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 128 * 1024];
    loop {
        let n = file.read(&mut buffer).map_err(io_error)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
fn regular(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
}
pub fn verify_file(path: &Path, expected: &str) -> Result<()> {
    if !regular(path) || sha256_file(path)? != expected {
        return Err(error(
            "A local model failed its integrity check. Reinstall Parakeet in Settings.",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AppleManifest {
    pub repo: String,
    pub revision: String,
    pub files: Vec<AppleFile>,
    pub total_bytes: u64,
}
#[derive(Deserialize)]
pub struct AppleFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}
pub fn apple_manifest() -> Result<AppleManifest> {
    serde_json::from_str(include_str!("../../../native/apple-speech/models.json")).map_err(io_error)
}
pub fn diarization_manifest() -> Result<AppleManifest> {
    serde_json::from_str(include_str!(
        "../../../native/apple-speech/diarization-models.json"
    ))
    .map_err(io_error)
}
pub enum BackendModels {
    Cpu(PathBuf),
    Apple(PathBuf),
}

pub struct ModelManager {
    pub root: PathBuf,
    emit: Emit,
    downloading: AtomicBool,
    progress: Mutex<f64>,
    install_lock: Mutex<()>,
}
impl ModelManager {
    pub fn new(data_dir: &Path, emit: Emit) -> Arc<Self> {
        Arc::new(Self {
            root: data_dir.join("models"),
            emit,
            downloading: AtomicBool::new(false),
            progress: Mutex::new(0.0),
            install_lock: Mutex::new(()),
        })
    }
    pub fn diarization_dir(&self) -> PathBuf {
        self.root.join("diarization-coreml")
    }
    pub fn offline_speakers_installed(&self) -> bool {
        diarization_manifest().is_ok_and(|m| {
            m.files.iter().all(|f| {
                fs::metadata(self.diarization_dir().join(&f.path))
                    .is_ok_and(|s| s.is_file() && s.len() == f.bytes)
            })
        })
    }
    pub fn verify_offline_speakers(&self) -> Result<()> {
        if !self.offline_speakers_installed() {
            return Err(error("Set up meeting refinement in Settings first."));
        }
        for f in diarization_manifest()?.files {
            verify_file(&self.diarization_dir().join(f.path), &f.sha256)?;
        }
        Ok(())
    }
    pub fn cpu_dir(&self) -> PathBuf {
        self.root.join("parakeet-v3-int8")
    }
    pub fn apple_dir(&self) -> PathBuf {
        self.root.join("parakeet-v3-coreml")
    }
    pub fn vad_path(&self) -> PathBuf {
        let shared = self.root.join("silero_vad.onnx");
        if shared.is_file() {
            shared
        } else {
            self.cpu_dir().join("silero_vad.onnx")
        }
    }
    pub fn speaker_path(&self) -> PathBuf {
        self.root.join(SPEAKER_FILE)
    }
    pub fn apple_installed(&self) -> bool {
        cfg!(all(target_os = "macos", target_arch = "aarch64"))
            && self.apple_dir().join("verified.json").is_file()
            && apple_manifest().is_ok_and(|m| {
                m.files.iter().all(|f| {
                    fs::metadata(self.apple_dir().join(&f.path))
                        .is_ok_and(|s| s.is_file() && s.len() == f.bytes)
                })
            })
    }
    pub fn installed(&self, kind: &str) -> bool {
        if kind == "speakers" {
            return fs::metadata(self.speaker_path())
                .is_ok_and(|m| m.is_file() && m.len() == 29_596_978);
        }
        self.vad_path().is_file()
            && (self.apple_installed()
                || (self.cpu_dir().join("verified.json").is_file()
                    && CPU_FILES
                        .iter()
                        .all(|(name, _)| regular(&self.cpu_dir().join(name)))))
    }
    pub fn status(&self) -> Value {
        json!({"offlineSpeakerModelInstalled":self.offline_speakers_installed(),"modelInstalled":self.installed("parakeet"),"speakerModelInstalled":self.installed("speakers"),"modelDownloading":self.downloading.load(Ordering::Acquire),"modelProgress":*self.progress.lock().unwrap_or_else(|p|p.into_inner()),"speechBackend":if self.apple_installed(){"coreml"}else{"cpu"}})
    }
    pub fn verify_vad(&self) -> Result<()> {
        verify_file(&self.vad_path(), VAD_HASH)
    }
    pub fn verify(&self) -> Result<BackendModels> {
        if !self.installed("parakeet") {
            return Err(error(
                "Install Parakeet in Settings before recording or importing audio.",
            ));
        }
        self.verify_vad()?;
        if self.apple_installed() {
            for f in apple_manifest()?.files {
                verify_file(&self.apple_dir().join(&f.path), &f.sha256)?;
            }
            return Ok(BackendModels::Apple(self.apple_dir()));
        }
        for (name, hash) in CPU_FILES {
            verify_file(&self.cpu_dir().join(name), hash)?;
        }
        Ok(BackendModels::Cpu(self.cpu_dir()))
    }
    pub fn verify_speaker(&self) -> Result<()> {
        verify_file(&self.speaker_path(), SPEAKER_HASH)
    }
    fn progress(&self, kind: &str, progress: f64, stage: &str) {
        *self.progress.lock().unwrap_or_else(|p| p.into_inner()) = progress;
        (self.emit)(
            "model_progress",
            json!({"kind":kind,"progress":progress,"stage":stage}),
        );
    }
    #[allow(clippy::too_many_arguments)] // Asset identity plus explicit UI progress interval.
    fn download(
        &self,
        client: &Client,
        url: &str,
        dest: &Path,
        hash: &str,
        size: u64,
        kind: &str,
        lower: f64,
        upper: f64,
    ) -> Result<()> {
        let mut response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| {
                error("The model could not be downloaded. Check your connection and try again.")
            })?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dest)
            .map_err(io_error)?;
        let mut digest = Sha256::new();
        let mut total = 0u64;
        let mut buffer = [0; 128 * 1024];
        let mut last = std::time::Instant::now();
        loop {
            let count = response.read(&mut buffer).map_err(io_error)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > size {
                return Err(error(
                    "The model download does not match its published size.",
                ));
            }
            file.write_all(&buffer[..count]).map_err(io_error)?;
            digest.update(&buffer[..count]);
            if last.elapsed() > Duration::from_millis(100) {
                self.progress(
                    kind,
                    lower + (upper - lower) * total as f64 / size as f64,
                    "Downloading",
                );
                last = std::time::Instant::now();
            }
        }
        if total != size || format!("{:x}", digest.finalize()) != hash {
            return Err(error(
                "The model download failed its integrity check. Try installing again.",
            ));
        }
        file.sync_all().map_err(io_error)
    }
    pub fn install(&self, kind: &str) -> Result<Value> {
        if !matches!(
            kind,
            "parakeet" | "speakers" | "parakeet-cpu" | "speakers-offline"
        ) {
            return Err(error("Choose Parakeet or speaker recognition."));
        }
        let _guard = self
            .install_lock
            .try_lock()
            .map_err(|_| error("A model download is already in progress."))?;
        self.downloading.store(true, Ordering::Release);
        self.progress(kind, 0.0, "Preparing");
        let result = self.install_inner(kind);
        self.downloading.store(false, Ordering::Release);
        match &result {
            Ok(_) => self.progress(kind, 1.0, "Installed"),
            Err(_) => {
                let progress = *self.progress.lock().unwrap_or_else(|p| p.into_inner());
                self.progress(kind, progress, "Failed")
            }
        };
        result
    }
    fn install_inner(&self, kind: &str) -> Result<Value> {
        fs::create_dir_all(&self.root).map_err(io_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
        }
        let stage = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&self.root)
            .map_err(io_error)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(900))
            .build()
            .map_err(io_error)?;
        if kind == "speakers-offline" {
            if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
                return Err(error(
                    "Meeting refinement currently requires Apple silicon.",
                ));
            }
            let manifest = diarization_manifest()?;
            let model = stage.path().join("model");
            fs::create_dir(&model).map_err(io_error)?;
            let mut received = 0;
            for file in &manifest.files {
                safe_relative(&file.path)?;
                let target = model.join(&file.path);
                fs::create_dir_all(target.parent().unwrap()).map_err(io_error)?;
                self.download(
                    &client,
                    &format!(
                        "https://huggingface.co/{}/resolve/{}/{}",
                        manifest.repo, manifest.revision, file.path
                    ),
                    &target,
                    &file.sha256,
                    file.bytes,
                    kind,
                    received as f64 / manifest.total_bytes as f64,
                    (received + file.bytes) as f64 / manifest.total_bytes as f64,
                )?;
                received += file.bytes;
            }
            replace_directory(&model, &self.diarization_dir(), false)?;
        } else if kind == "speakers" {
            let path = stage.path().join(SPEAKER_FILE);
            self.download(
                &client,
                &format!("{BASE}/speaker-recongition-models/{SPEAKER_FILE}"),
                &path,
                SPEAKER_HASH,
                29_596_978,
                kind,
                0.0,
                0.99,
            )?;
            replace_file(&path, &self.speaker_path())?;
        } else {
            let model = stage.path().join("model");
            fs::create_dir(&model).map_err(io_error)?;
            let apple =
                kind == "parakeet" && cfg!(all(target_os = "macos", target_arch = "aarch64"));
            if apple {
                let manifest = apple_manifest()?;
                let mut received = 0;
                for file in &manifest.files {
                    safe_relative(&file.path)?;
                    let target = model.join(&file.path);
                    fs::create_dir_all(target.parent().unwrap()).map_err(io_error)?;
                    self.download(
                        &client,
                        &format!(
                            "https://huggingface.co/{}/resolve/{}/{}",
                            manifest.repo, manifest.revision, file.path
                        ),
                        &target,
                        &file.sha256,
                        file.bytes,
                        kind,
                        received as f64 / manifest.total_bytes as f64 * 0.88,
                        (received + file.bytes) as f64 / manifest.total_bytes as f64 * 0.88,
                    )?;
                    received += file.bytes;
                }
                self.progress(
                    kind,
                    0.89,
                    "Optimizing for this Mac (first install may take a few minutes)",
                );
            } else {
                let archive = stage.path().join("parakeet.tar.bz2");
                self.download(
                    &client,
                    &format!("{BASE}/asr-models/{PARAKEET_NAME}.tar.bz2"),
                    &archive,
                    "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
                    487_170_055,
                    kind,
                    0.0,
                    0.88,
                )?;
                self.progress(kind, 0.89, "Unpacking");
                extract_cpu(&archive, &model)?;
                for (name, hash) in CPU_FILES {
                    verify_file(&model.join(name), hash)?;
                }
            }
            let vad = stage.path().join("silero_vad.onnx");
            if let Ok(()) = verify_file(&self.vad_path(), VAD_HASH) {
                fs::copy(self.vad_path(), &vad).map_err(io_error)?;
            } else {
                self.download(
                    &client,
                    &format!("{BASE}/asr-models/silero_vad.onnx"),
                    &vad,
                    VAD_HASH,
                    643_854,
                    kind,
                    0.96,
                    0.99,
                )?;
            }
            let destination = if apple {
                self.apple_dir()
            } else {
                self.cpu_dir()
            };
            replace_directory(&model, &destination, apple)?;
            replace_file(&vad, &self.root.join("silero_vad.onnx"))?;
        }
        Ok(json!({"kind":kind,"installed":true}))
    }
}
fn safe_relative(value: &str) -> Result<()> {
    if value.is_empty()
        || Path::new(value)
            .components()
            .any(|x| !matches!(x, Component::Normal(_)))
    {
        return Err(error("The model package contains an unsafe path."));
    }
    Ok(())
}
fn replace_file(source: &Path, target: &Path) -> Result<()> {
    // Unix rename replaces atomically; Windows requires moving an existing file aside.
    #[cfg(not(windows))]
    {
        fs::rename(source, target).map_err(io_error)
    }
    #[cfg(windows)]
    {
        let backup = target.with_extension("previous");
        if target.exists() {
            fs::rename(target, &backup).map_err(io_error)?;
        }
        match fs::rename(source, target) {
            Ok(()) => {
                let _ = fs::remove_file(backup);
                Ok(())
            }
            Err(e) => {
                let _ = fs::rename(backup, target);
                Err(io_error(e))
            }
        }
    }
}
fn replace_directory(source: &Path, target: &Path, apple: bool) -> Result<()> {
    replace_directory_with(source, target, |target| {
        if apple {
            super::speech::prepare_apple(target)?;
        }
        Ok(())
    })
}
fn replace_directory_with(
    source: &Path,
    target: &Path,
    prepare: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let backup = target.with_extension(format!("previous-{}", uuid::Uuid::new_v4()));
    if fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(error("The model destination is a symbolic link."));
    }
    let existed = target.exists();
    if existed {
        fs::rename(target, &backup).map_err(io_error)?;
    }
    if let Err(e) = fs::rename(source, target) {
        if existed {
            let _ = fs::rename(&backup, target);
        }
        return Err(io_error(e));
    }
    // CoreML can key optimized state by path. Prepare at the final verified path;
    // publish the installed marker only after optimization succeeds. Roll back on error.
    let prepared = (|| -> Result<()> {
        prepare(target)?;
        fs::write(target.join("verified.json"), b"{\"verified\":true}").map_err(io_error)
    })();
    if let Err(problem) = prepared {
        let _ = fs::remove_dir_all(target);
        if existed {
            let _ = fs::rename(&backup, target);
        }
        return Err(problem);
    }
    if existed {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}
pub fn extract_cpu(archive: &Path, destination: &Path) -> Result<()> {
    let mut package = tar::Archive::new(BzDecoder::new(File::open(archive).map_err(io_error)?));
    let mut found = HashSet::new();
    for entry in package.entries().map_err(io_error)? {
        let mut entry = entry.map_err(io_error)?;
        let path = entry.path().map_err(io_error)?.into_owned();
        let selected = CPU_FILES
            .iter()
            .find(|(name, _)| path == Path::new(PARAKEET_NAME).join(name));
        if let Some((name, _)) = selected {
            let size = entry.size();
            if !entry.header().entry_type().is_file()
                || size == 0
                || size > 1_500_000_000
                || !found.insert(*name)
            {
                return Err(error("The speech package contains an invalid model file."));
            }
            let target = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(destination.join(name))
                .map_err(io_error)?;
            let mut target = std::io::BufWriter::new(target);
            std::io::copy(&mut entry, &mut target).map_err(io_error)?;
            target.flush().map_err(io_error)?;
            target.get_ref().sync_all().map_err(io_error)?;
        }
    }
    if found.len() != CPU_FILES.len() {
        return Err(error("The speech package is incomplete."));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_paths_reject_traversal() {
        for path in ["/tmp/x", "../x", "a/../b", ""] {
            assert!(safe_relative(path).is_err());
        }
        assert!(safe_relative("Encoder.mlmodelc/weights/weight.bin").is_ok());
    }
    #[test]
    fn status_never_installs() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(dir.path(), Arc::new(|_, _| {}));
        assert_eq!(manager.status()["modelInstalled"], false);
        assert!(!dir.path().join("models").exists());
    }
    #[test]
    fn symlink_is_not_verified() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model");
        fs::write(&file, b"a").unwrap();
        assert!(verify_file(&file, &sha256_file(&file).unwrap()).is_ok());
        assert!(verify_file(&file, "bad").is_err());
    }
    #[test]
    fn failed_prepare_restores_previous_verified_install() {
        let temp = tempfile::tempdir().unwrap();
        let stage = temp.path().join("stage");
        let target = temp.path().join("installed");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(stage.join("model"), b"new").unwrap();
        fs::write(target.join("model"), b"previous").unwrap();
        fs::write(target.join("verified.json"), b"previous-marker").unwrap();
        let result = replace_directory_with(&stage, &target, |path| {
            assert_eq!(path, target);
            assert_eq!(fs::read(path.join("model")).unwrap(), b"new");
            assert!(!path.join("verified.json").exists());
            Err(error("optimization failed"))
        });
        assert!(result.is_err());
        assert_eq!(fs::read(target.join("model")).unwrap(), b"previous");
        assert_eq!(
            fs::read(target.join("verified.json")).unwrap(),
            b"previous-marker"
        );
    }
    #[test]
    fn prepared_install_gets_marker_after_prepare() {
        let temp = tempfile::tempdir().unwrap();
        let stage = temp.path().join("stage");
        let target = temp.path().join("installed");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("model"), b"verified").unwrap();
        replace_directory_with(&stage, &target, |path| {
            assert!(!path.join("verified.json").exists());
            Ok(())
        })
        .unwrap();
        assert!(target.join("verified.json").is_file());
        assert!(!stage.exists());
    }
    #[test]
    fn extraction_rejects_model_symlink_without_writing_outside() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("models.tar.bz2");
        let encoder = bzip2::write::BzEncoder::new(
            File::create(&archive).unwrap(),
            bzip2::Compression::fast(),
        );
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o600);
        tar.append_link(
            &mut header,
            format!("{PARAKEET_NAME}/encoder.int8.onnx"),
            "../outside",
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
        let destination = temp.path().join("out");
        fs::create_dir(&destination).unwrap();
        assert!(extract_cpu(&archive, &destination).is_err());
        assert!(fs::read_dir(destination).unwrap().next().is_none());
        assert!(!temp.path().join("outside").exists());
    }
}
