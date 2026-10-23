//! Portable snapshots: bounded ZIP staging, no credentials, no extraction into the library.
use super::{archive::LocalArchive, audio};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tuck_core::{ports::Result, AppError};
use uuid::Uuid;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};
const MANIFEST_LIMIT: u64 = 128 * 1024 * 1024;
const TOTAL_LIMIT: u64 = 20 * 1024 * 1024 * 1024;
const ENTRY_LIMIT: usize = 10_000;
/// Cap serialized metadata while writing, before allocating an entire JSON byte
/// buffer. The caller's temporary ZIP is discarded when this writer fails.
struct LimitedWriter<W> {
    inner: W,
    remaining: u64,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(std::io::Error::other("Manifest metadata limit exceeded"));
        }
        let written = self.inner.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
/// ZIP metadata must be bounded before the crate has collected its directory.
/// The shared budget is disabled only after entry-count and header validation.
struct MetadataReader {
    file: File,
    budget: Arc<AtomicU64>,
}
impl Read for MetadataReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let remaining = self.budget.load(Ordering::Relaxed);
        if remaining == u64::MAX {
            return self.file.read(buffer);
        }
        if remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ZIP metadata exceeds its safety budget",
            ));
        }
        let width = buffer.len().min(remaining as usize);
        let read = self.file.read(&mut buffer[..width])?;
        self.budget.fetch_sub(read as u64, Ordering::Relaxed);
        Ok(read)
    }
}
impl Seek for MetadataReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}
fn error(message: &str) -> AppError {
    AppError::new("backup_error", message)
}
fn failed<T>(_: T) -> AppError {
    error("The backup could not be read or written. Check the selected file and available storage.")
}
fn zip_target(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if !path.is_absolute()
        || path
            .extension()
            .is_none_or(|e| !e.eq_ignore_ascii_case("zip"))
        || fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
    {
        return Err(error(
            "Choose an absolute .zip backup filename using the file picker.",
        ));
    }
    Ok(path)
}
fn manifest_ids(manifest: &Value) -> Result<HashSet<String>> {
    if manifest["format"] != "tuck-library" || manifest["version"] != 1 {
        return Err(error("This is not a supported Tuck library backup."));
    }
    let rows = manifest["meetings"]
        .as_array()
        .ok_or_else(|| error("This backup has no meeting list."))?;
    if rows.len() > 10_000 {
        return Err(error("Restore at most 10,000 meetings at a time."));
    }
    let mut ids = HashSet::new();
    for row in rows {
        let id = row["meeting"]["id"]
            .as_str()
            .ok_or_else(|| error("A backup meeting has no identifier."))?;
        if !ids.insert(id.into()) {
            return Err(error("The backup contains duplicate meeting identifiers."));
        }
    }
    Ok(ids)
}
fn audio_entry(name: &str) -> Result<(&str, &str)> {
    if name.contains('\\') || name.contains('\0') || name.contains('%') {
        return Err(error("The backup contains an unsafe audio path."));
    }
    let fields: Vec<_> = name.split('/').collect();
    if fields.len() != 3 || fields[0] != "audio" || !audio::AUDIO_IDS.contains(&fields[2]) {
        return Err(error("The backup contains an unexpected file."));
    }
    let id = Uuid::parse_str(fields[1])
        .map_err(|_| error("The backup contains an invalid audio meeting ID."))?;
    if id.to_string() != fields[1] {
        return Err(error(
            "The backup contains a noncanonical audio meeting ID.",
        ));
    }
    Ok((fields[1], fields[2]))
}
impl LocalArchive {
    pub(crate) fn backup_write(
        &self,
        path: &str,
        manifest: &Value,
        include_audio: bool,
    ) -> Result<Value> {
        let target = zip_target(path)?;
        let ids = manifest_ids(manifest)?;
        let parent = target
            .parent()
            .ok_or_else(|| error("Choose a backup folder."))?;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(failed)?;
        let mut snapshot = manifest.clone();
        let mut index = vec![];
        let mut warnings = vec![];
        let mut paths = vec![];
        let mut total = 0u64;
        if include_audio {
            for id in ids.iter() {
                if !self.root.join(id).exists() {
                    continue;
                }
                let files = match self.audio_list(id) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(format!("Audio for {id} was skipped: {}", e.message));
                        continue;
                    }
                };
                for entry in files {
                    let audio_id = entry["id"].as_str().unwrap();
                    let path = audio::owned_path(&self.root, id, audio_id)?;
                    let bytes = entry["bytes"].as_u64().unwrap_or(0);
                    total = total
                        .checked_add(bytes)
                        .ok_or_else(|| error("This audio backup is too large."))?;
                    if total > TOTAL_LIMIT || paths.len() + 1 >= ENTRY_LIMIT {
                        return Err(error("This backup exceeds the 20 GB or 10,000-file safety limit. Back up without audio."));
                    }
                    let name = format!("audio/{id}/{audio_id}");
                    index.push(json!({"meetingId":id,"id":audio_id,"path":name,"bytes":bytes}));
                    paths.push((name, path, bytes));
                }
            }
        }
        snapshot["audio"] = json!(index);
        {
            let mut writer = ZipWriter::new(staged.as_file_mut());
            let options = SimpleFileOptions::default()
                .compression_method(CompressionMethod::Deflated)
                .unix_permissions(0o600);
            writer
                .start_file("manifest.json", options)
                .map_err(failed)?;
            serde_json::to_writer(LimitedWriter {inner:&mut writer, remaining:MANIFEST_LIMIT}, &snapshot)
                .map_err(|_| error("This library snapshot exceeds the 128 MiB metadata safety limit or could not be saved. Check available storage."))?;
            // PCM is copied with bounded std::io buffers, never loaded as a whole.
            for (name, _path, size) in &paths {
                let (meeting, audio_id) = audio_entry(name)?;
                let file = audio::open_owned(&self.root, meeting, audio_id)?;
                if file.metadata().map_err(failed)?.len() != *size {
                    return Err(error("Audio changed while the backup was being prepared. Stop recording and try again."));
                }
                writer
                    .start_file(
                        name,
                        SimpleFileOptions::default()
                            .compression_method(CompressionMethod::Stored)
                            .unix_permissions(0o600)
                            .large_file(true),
                    )
                    .map_err(failed)?;
                let copied =
                    std::io::copy(&mut file.take(*size + 1), &mut writer).map_err(failed)?;
                if copied != *size {
                    return Err(error("Audio changed while the backup was being written. Try again after recording finishes."));
                }
            }
            writer
                .finish()
                .map_err(failed)?
                .sync_all()
                .map_err(failed)?;
        }
        staged.persist(target).map_err(failed)?;
        Ok(
            json!({"path":target,"meetingCount":ids.len(),"audioIncluded":include_audio,"audioFiles":paths.len(),"warnings":warnings}),
        )
    }
    pub(crate) fn backup_read(&self, path: &str) -> Result<Value> {
        let path = zip_target(path)?;
        let file = audio::open_regular(path)?;
        if file.metadata().map_err(failed)?.len() > TOTAL_LIMIT + MANIFEST_LIMIT + 16 * 1024 * 1024
        {
            return Err(error("This ZIP exceeds the backup file safety limit."));
        }
        let budget = Arc::new(AtomicU64::new(16 * 1024 * 1024));
        let mut archive = ZipArchive::new(MetadataReader {
            file,
            budget: budget.clone(),
        })
        .map_err(failed)?;
        if archive.is_empty() || archive.len() > ENTRY_LIMIT {
            return Err(error("This backup has too many files or is empty."));
        }
        archive.validate_header_consistency().map_err(|_| {
            error("This ZIP has duplicate, overlapping, or inconsistent file headers.")
        })?;
        budget.store(u64::MAX, Ordering::Relaxed);
        let mut names = HashSet::new();
        let mut total = 0u64;
        let mut manifest_index = None;
        // Inspect every directory entry before opening an extraction destination.
        for i in 0..archive.len() {
            let entry = archive.by_index(i).map_err(failed)?;
            let name = entry.name().map_err(failed)?.into_owned();
            if !names.insert(name.clone())
                || entry.encrypted()
                || entry.is_symlink()
                || !entry.is_file()
                || entry
                    .unix_mode()
                    .is_some_and(|mode| mode & 0o170000 != 0 && mode & 0o170000 != 0o100000)
            {
                return Err(error(
                    "This backup contains duplicate, encrypted, or nonregular files.",
                ));
            }
            let limit = if name == "manifest.json" {
                manifest_index = Some(i);
                MANIFEST_LIMIT
            } else {
                audio_entry(&name)?;
                u32::MAX as u64
            };
            if entry.size() > limit {
                return Err(error("A backup entry exceeds its safe size limit."));
            }
            total = total
                .checked_add(entry.size())
                .ok_or_else(|| error("This backup is too large."))?;
            if total > TOTAL_LIMIT + MANIFEST_LIMIT {
                return Err(error("This backup exceeds the 20 GB safety limit."));
            }
        }
        let i = manifest_index.ok_or_else(|| error("This ZIP contains no Tuck manifest."))?;
        let mut bytes = vec![];
        archive
            .by_index(i)
            .map_err(failed)?
            .take(MANIFEST_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(failed)?;
        if bytes.len() as u64 > MANIFEST_LIMIT {
            return Err(error("This backup manifest is too large."));
        }
        let fingerprint = format!("{:x}", Sha256::digest(&bytes));
        let manifest: Value = serde_json::from_slice(&bytes)
            .map_err(|_| error("The backup manifest is unreadable."))?;
        let ids = manifest_ids(&manifest)?;
        let stage = tempfile::Builder::new()
            .prefix("tuck-restore-")
            .tempdir()
            .map_err(failed)?;
        let mut count = 0usize;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(failed)?;
            let entry_name = entry.name().map_err(failed)?.into_owned();
            if entry_name == "manifest.json" {
                continue;
            }
            let (name, id) = {
                let (meeting, id) = audio_entry(&entry_name)?;
                (meeting.to_owned(), id.to_owned())
            };
            if !ids.contains(&name) {
                return Err(error(
                    "Backup audio belongs to a meeting missing from its manifest.",
                ));
            }
            let directory = stage.path().join("audio").join(&name);
            fs::create_dir_all(&directory).map_err(failed)?;
            let target = directory.join(&id);
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .map_err(failed)?;
            let declared = entry.size();
            let copied = std::io::copy(&mut entry.by_ref().take(declared + 1), &mut output)
                .map_err(failed)?;
            if copied != declared {
                return Err(error("A backup entry has an inconsistent size."));
            }
            output.sync_all().map_err(failed)?;
            drop(output);
            let mut audio = audio::open_regular(&target)?;
            audio::inspect(&mut audio)?;
            count += 1;
        }
        let token = Uuid::new_v4().to_string();
        let mut stages = self
            .stages
            .lock()
            .map_err(|_| error("Backup staging is unavailable."))?;
        if stages.len() >= 2 {
            return Err(error(
                "Finish the current restore before opening another backup.",
            ));
        }
        stages.insert(token.clone(), stage);
        Ok(
            json!({"manifest":manifest,"fingerprint":fingerprint,"stagingToken":token,"audioCount":count,"warnings":[]}),
        )
    }
    pub(crate) fn backup_commit_audio(&self, token: &str, id_map: &Value) -> Result<Value> {
        let stage = self
            .stages
            .lock()
            .map_err(|_| error("Backup staging is unavailable."))?
            .remove(token)
            .ok_or_else(|| error("The staged backup has expired. Open the backup again."))?;
        let mapping = id_map
            .as_object()
            .ok_or_else(|| error("Invalid restore audio mapping."))?;
        let mut warnings = vec![];
        let mut restored = 0usize;
        for (old, new) in mapping {
            let new = new
                .as_str()
                .ok_or_else(|| error("Invalid restore meeting ID."))?;
            if Uuid::parse_str(old).is_err() || Uuid::parse_str(new).is_err() || old != new {
                warnings.push(format!(
                    "Audio for {old} was skipped because its restore identity is invalid."
                ));
                continue;
            }
            let source = stage.path().join("audio").join(old);
            if !source.exists() {
                continue;
            }
            let installed = (|| -> Result<usize> {
                if self.root.exists() {
                    let meta = fs::symlink_metadata(&self.root).map_err(failed)?;
                    if !meta.is_dir() || meta.file_type().is_symlink() {
                        return Err(error("The library audio folder is unsafe."));
                    }
                } else {
                    fs::create_dir(&self.root).map_err(failed)?;
                }
                let target = self.root.join(new);
                if target.try_exists().map_err(failed)? {
                    return Err(error(
                        "Existing or orphan audio was kept; no audio was overwritten.",
                    ));
                }
                // A private temporary sibling keeps partial copy failures out of the final path.
                let destination = tempfile::Builder::new()
                    .prefix(".restore-")
                    .tempdir_in(&self.root)
                    .map_err(failed)?;
                let mut count = 0;
                for id in audio::AUDIO_IDS {
                    let input = source.join(id);
                    if !input.exists() {
                        continue;
                    }
                    audio::copy_exact(&input, &destination.path().join(id))?;
                    count += 1;
                }
                // reserve the never-existing destination before moving files; rollback only
                // our newly created directory if any installation step fails.
                fs::create_dir(&target)
                    .map_err(|_| error("Existing audio was kept; no audio was overwritten."))?;
                let installation = (|| -> Result<()> {
                    for id in audio::AUDIO_IDS {
                        let input = destination.path().join(id);
                        if !input.exists() {
                            continue;
                        }
                        // hard_link cannot replace an existing file. Both paths
                        // are on the same volume because staging is a sibling.
                        fs::hard_link(&input, target.join(id)).map_err(failed)?;
                    }
                    Ok(())
                })();
                if let Err(problem) = installation {
                    let _ = fs::remove_dir_all(&target);
                    return Err(problem);
                }
                Ok(count)
            })();
            match installed {
                Ok(count) => restored += count,
                Err(problem) => warnings.push(format!(
                    "Audio for {old} was not restored: {}",
                    problem.message
                )),
            }
        }
        Ok(json!({"audioFilesRestored":restored,"warnings":warnings}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_serialization_stops_at_its_output_budget() {
        let mut bytes = Vec::new();
        let value = json!({"notes":"x".repeat(128)});
        assert!(serde_json::to_writer(
            LimitedWriter {
                inner: &mut bytes,
                remaining: 64
            },
            &value
        )
        .is_err());
        assert!(bytes.len() <= 64);
        let mut complete = Vec::new();
        serde_json::to_writer(
            LimitedWriter {
                inner: &mut complete,
                remaining: 256,
            },
            &value,
        )
        .unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&complete).unwrap(), value);
    }
    use tuck_core::ports::AudioArchivePort;
    fn manifest(id: &str) -> Value {
        json!({"format":"tuck-library","version":1,"settings":{"provider":"local"},"meetings":[{"meeting":{"id":id,"title":"Fixture","notes":"Preserve my notes"},"segments":[],"corrections":[],"speakers":[],"messages":[],"bookmarks":[],"revisions":[]}]})
    }
    #[test]
    fn portable_backup_restores_audio_without_overwrite_or_original_changes() {
        let origin = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        let owned = origin.path().join("audio").join(&id);
        fs::create_dir_all(&owned).unwrap();
        audio::tests::fixture(&owned.join("microphone.wav"), 32000, true);
        let original = fs::read(owned.join("microphone.wav")).unwrap();
        let source = LocalArchive::new(origin.path());
        let destination = LocalArchive::new(target.path());
        let output = origin.path().join("backup.zip");
        let report = source
            .write_backup(output.to_str().unwrap(), &manifest(&id), true)
            .unwrap();
        assert_eq!(report["audioFiles"], 1);
        assert_eq!(fs::read(owned.join("microphone.wav")).unwrap(), original);
        let stage = destination.read_backup(output.to_str().unwrap()).unwrap();
        assert_eq!(stage["audioCount"], 1);
        assert_eq!(stage["fingerprint"].as_str().unwrap().len(), 64);
        assert_eq!(
            stage["manifest"]["meetings"][0]["meeting"]["notes"],
            "Preserve my notes"
        );
        let token = stage["stagingToken"].as_str().unwrap();
        let result = destination
            .commit_restore_audio(token, &json!({&id:&id}))
            .unwrap();
        assert_eq!(result["audioFilesRestored"], 1);
        assert!(result["warnings"].as_array().unwrap().is_empty());
        assert_eq!(
            fs::read(target.path().join("audio").join(&id).join("microphone.wav")).unwrap(),
            original
        );
        assert!(destination
            .commit_restore_audio(token, &json!({&id:&id}))
            .is_err());
        let retained =
            fs::read(target.path().join("audio").join(&id).join("microphone.wav")).unwrap();
        let stage = destination.read_backup(output.to_str().unwrap()).unwrap();
        let result = destination
            .commit_restore_audio(stage["stagingToken"].as_str().unwrap(), &json!({&id:&id}))
            .unwrap();
        assert_eq!(result["audioFilesRestored"], 0);
        assert_eq!(result["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(
            fs::read(target.path().join("audio").join(&id).join("microphone.wav")).unwrap(),
            retained
        );
    }
    #[test]
    fn metadata_only_backup_is_portable_and_staging_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let archive = LocalArchive::new(dir.path());
        let id = Uuid::new_v4().to_string();
        let output = dir.path().join("metadata.zip");
        archive
            .write_backup(output.to_str().unwrap(), &manifest(&id), false)
            .unwrap();
        let stage = archive.read_backup(output.to_str().unwrap()).unwrap();
        assert_eq!(stage["audioCount"], 0);
        let token = stage["stagingToken"].as_str().unwrap();
        archive.discard_restore(token).unwrap();
        assert!(archive.commit_restore_audio(token, &json!({})).is_err());
    }
    #[test]
    fn archive_paths_and_unexpected_entries_are_rejected_before_restore() {
        let dir = tempfile::tempdir().unwrap();
        let archive = LocalArchive::new(dir.path());
        let id = Uuid::new_v4().to_string();
        for (i, name) in [
            "../escape.wav".into(),
            "/tmp/escape.wav".into(),
            format!("audio/{id}/../../escape.wav"),
            format!("audio/{id}/secret.txt"),
            "credentials.json".into(),
        ]
        .iter()
        .enumerate()
        {
            let output = dir.path().join(format!("unsafe-{i}.zip"));
            let mut writer = ZipWriter::new(fs::File::create(&output).unwrap());
            writer
                .start_file("manifest.json", SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(manifest(&id).to_string().as_bytes())
                .unwrap();
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"untrusted").unwrap();
            writer.finish().unwrap();
            assert!(
                archive.read_backup(output.to_str().unwrap()).is_err(),
                "{name}"
            );
        }
        assert!(!dir.path().join("escape.wav").exists());
    }
    #[test]
    fn valid_wav_container_metadata_restores_byte_for_byte() {
        use std::io::{Seek, SeekFrom};
        let origin = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        let folder = origin.path().join("audio").join(&id);
        fs::create_dir_all(&folder).unwrap();
        let original = folder.join("import.wav");
        audio::tests::fixture(&original, 32000, false);
        let mut file = OpenOptions::new().append(true).open(&original).unwrap();
        file.write_all(b"LIST\x10\0\0\0INFOINAM\x04\0\0\0SGL\0")
            .unwrap();
        drop(file);
        let length = fs::metadata(&original).unwrap().len();
        let mut file = OpenOptions::new().write(true).open(&original).unwrap();
        file.seek(SeekFrom::Start(4)).unwrap();
        file.write_all(&((length - 8) as u32).to_le_bytes())
            .unwrap();
        drop(file);
        let expected = Sha256::digest(fs::read(&original).unwrap());
        let archive = origin.path().join("metadata-preserved.zip");
        LocalArchive::new(origin.path())
            .write_backup(archive.to_str().unwrap(), &manifest(&id), true)
            .unwrap();
        let destination = LocalArchive::new(target.path());
        let staged = destination.read_backup(archive.to_str().unwrap()).unwrap();
        assert_eq!(
            destination
                .commit_restore_audio(staged["stagingToken"].as_str().unwrap(), &json!({&id:&id}))
                .unwrap()["audioFilesRestored"],
            1
        );
        let restored = target.path().join("audio").join(&id).join("import.wav");
        assert_eq!(Sha256::digest(fs::read(restored).unwrap()), expected);
        assert_eq!(Sha256::digest(fs::read(original).unwrap()), expected);
    }
    #[test]
    fn duplicate_headers_and_symlinks_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let archive = LocalArchive::new(dir.path());
        let id = Uuid::new_v4().to_string();
        let output = dir.path().join("duplicate.zip");
        let mut writer = ZipWriter::new(fs::File::create(&output).unwrap());
        for name in ["manifest.json", "manifesx.json"] {
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer
                .write_all(manifest(&id).to_string().as_bytes())
                .unwrap();
        }
        writer.finish().unwrap();
        let mut bytes = fs::read(&output).unwrap();
        let needle = b"manifesx.json";
        let offsets: Vec<_> = bytes
            .windows(needle.len())
            .enumerate()
            .filter_map(|(i, part)| (part == needle).then_some(i))
            .collect();
        for offset in offsets {
            bytes[offset..offset + needle.len()].copy_from_slice(b"manifest.json");
        }
        fs::write(&output, bytes).unwrap();
        assert!(archive.read_backup(output.to_str().unwrap()).is_err());
        let output = dir.path().join("symlink.zip");
        let mut writer = ZipWriter::new(fs::File::create(&output).unwrap());
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(manifest(&id).to_string().as_bytes())
            .unwrap();
        writer
            .add_symlink(
                format!("audio/{id}/microphone.wav"),
                "/etc/passwd",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer.finish().unwrap();
        assert!(archive.read_backup(output.to_str().unwrap()).is_err());
    }
}
