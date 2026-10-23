use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tuck_core::{
    ports::{AudioArchivePort, Result},
    AppError,
};
use uuid::Uuid;

/// Only meeting-owned audio may be removed. Exports are explicit Save As destinations.
pub struct LocalArchive {
    pub(crate) root: PathBuf,
    pub(crate) stages: Mutex<HashMap<String, tempfile::TempDir>>,
}
impl LocalArchive {
    pub fn new(data: &Path) -> Self {
        Self {
            root: data.join("audio"),
            stages: Mutex::new(HashMap::new()),
        }
    }
}
impl LocalArchive {
    pub fn audio_list(&self, meeting_id: &str) -> Result<Vec<serde_json::Value>> {
        use serde_json::json;
        Uuid::parse_str(meeting_id)
            .map_err(|_| AppError::invalid("Invalid meeting identifier."))?;
        if !self.root.join(meeting_id).exists() {
            return Ok(vec![]);
        }
        let mut result = vec![];
        for audio_id in super::audio::AUDIO_IDS {
            let candidate = self.root.join(meeting_id).join(audio_id);
            if !candidate
                .try_exists()
                .map_err(|_| AppError::unavailable("This audio source could not be checked."))?
            {
                continue;
            }
            super::audio::owned_path(&self.root, meeting_id, audio_id)?;
            let mut file = super::audio::open_owned(&self.root, meeting_id, audio_id)?;
            let info = super::audio::inspect(&mut file)?;
            result.push(json!({"id":audio_id,"name":match *audio_id {"microphone.wav"=>"Microphone","system.wav"=>"Computer audio",_=>"Imported audio"},"bytes":file.metadata().map_err(|_|AppError::unavailable("Audio metadata is unavailable."))?.len(),"duration":info.duration(),"repairable":info.repairable}));
        }
        Ok(result)
    }
}
impl AudioArchivePort for LocalArchive {
    fn list(&self, meeting_id: &str) -> Result<Vec<serde_json::Value>> {
        self.audio_list(meeting_id)
    }
    fn resolve(&self, meeting_id: &str, audio_id: &str) -> Result<String> {
        let path = super::audio::owned_path(&self.root, meeting_id, audio_id)?;
        let mut file = super::audio::open_owned(&self.root, meeting_id, audio_id)?;
        super::audio::inspect(&mut file)?;
        path.to_str()
            .map(str::to_owned)
            .ok_or_else(|| AppError::unavailable("This audio path cannot be opened."))
    }
    fn write_backup(
        &self,
        path: &str,
        manifest: &serde_json::Value,
        include_audio: bool,
    ) -> Result<serde_json::Value> {
        self.backup_write(path, manifest, include_audio)
    }
    fn read_backup(&self, path: &str) -> Result<serde_json::Value> {
        self.backup_read(path)
    }
    fn commit_restore_audio(
        &self,
        token: &str,
        id_map: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.backup_commit_audio(token, id_map)
    }
    fn discard_restore(&self, token: &str) -> Result<()> {
        self.stages
            .lock()
            .map_err(|_| AppError::unavailable("Backup staging is unavailable."))?
            .remove(token);
        Ok(())
    }

    fn remove(&self, meeting_id: &str) -> Result<()> {
        Uuid::parse_str(meeting_id)
            .map_err(|_| AppError::invalid("Invalid meeting identifier."))?;
        let path = self.root.join(meeting_id);
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Ok(meta) if meta.file_type().is_symlink() => fs::remove_file(path)
                .map_err(|_| AppError::unavailable("Could not remove this meeting's audio link.")),
            Ok(meta) if meta.is_dir() => fs::remove_dir_all(path)
                .map_err(|_| AppError::unavailable("Could not remove this meeting's audio.")),
            Ok(_) => Err(AppError::unavailable(
                "This meeting's audio directory is invalid.",
            )),
            Err(_) => Err(AppError::unavailable(
                "Could not access this meeting's audio.",
            )),
        }
    }
    fn export(&self, path: &str, content: &str) -> Result<()> {
        let target = Path::new(path);
        if !target.is_absolute()
            || !target
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "md" | "json" | "srt"))
            || fs::symlink_metadata(target).is_ok_and(|m| m.file_type().is_symlink())
        {
            return Err(AppError::invalid(
                "Choose a Markdown, JSON, or SRT file using Save As.",
            ));
        }
        let parent = target
            .parent()
            .ok_or_else(|| AppError::invalid("Choose an export folder."))?;
        let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(|_| {
            AppError::unavailable(
                "Could not create the export file. Check the folder's permissions.",
            )
        })?;
        staged
            .write_all(content.as_bytes())
            .and_then(|_| staged.as_file().sync_all())
            .map_err(|_| {
                AppError::unavailable("Could not finish the export. Check available storage.")
            })?;
        staged
            .persist(target)
            .map_err(|_| AppError::unavailable("Could not save the export file."))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refuses_paths_outside_owned_audio() {
        let dir = tempfile::tempdir().unwrap();
        let archive = LocalArchive::new(dir.path());
        assert!(archive.remove("../other").is_err());
        assert!(archive.export("relative.md", "notes").is_err());
        assert!(archive
            .export(dir.path().join("bad.exe").to_str().unwrap(), "notes")
            .is_err());
        let destination = dir.path().join("notes.md");
        archive
            .export(destination.to_str().unwrap(), "saved")
            .unwrap();
        assert_eq!(fs::read_to_string(destination).unwrap(), "saved");
    }
    #[cfg(unix)]
    #[test]
    fn unlinking_audio_never_follows_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("keep"), "audio").unwrap();
        fs::create_dir(dir.path().join("audio")).unwrap();
        let id = Uuid::new_v4().to_string();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("audio").join(&id)).unwrap();
        LocalArchive::new(dir.path()).remove(&id).unwrap();
        assert!(outside.path().join("keep").is_file());
    }
}
