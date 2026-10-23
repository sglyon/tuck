//! Small, streaming PCM WAV primitives shared by playback, recovery and backups.
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
use tuck_core::{ports::Result, AppError};
use uuid::Uuid;
pub const AUDIO_IDS: &[&str] = &["microphone.wav", "system.wav", "import.wav"];
pub const MAX_CHUNK: u64 = 1024 * 1024;
fn error(message: &str) -> AppError {
    AppError::new("audio_archive_error", message)
}
pub fn owned_path(root: &Path, meeting: &str, audio: &str) -> Result<PathBuf> {
    let id = Uuid::parse_str(meeting).map_err(|_| error("Invalid meeting audio identifier."))?;
    if id.to_string() != meeting || !AUDIO_IDS.contains(&audio) {
        return Err(error("Choose an audio source belonging to this meeting."));
    }
    for directory in [root.to_path_buf(), root.join(meeting)] {
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|_| error("This meeting has no retained audio."))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(error("The meeting audio folder is unsafe."));
        }
    }
    let path = root.join(meeting).join(audio);
    let metadata =
        fs::symlink_metadata(&path).map_err(|_| error("This audio source is unavailable."))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(error("This audio source is not an owned regular file."));
    }
    Ok(path)
}
/// On Unix, directory descriptors anchor each lookup so swapping an owned
/// directory for a symlink between validation and open cannot escape the root.
pub fn open_owned(root: &Path, meeting: &str, audio: &str) -> Result<File> {
    owned_path(root, meeting, audio)?;
    #[cfg(unix)]
    {
        use std::{
            ffi::CString,
            os::{
                fd::{AsRawFd, FromRawFd},
                unix::fs::OpenOptionsExt,
            },
        };
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(root)
            .map_err(|_| error("The audio root could not be opened safely."))?;
        fn child(parent: &File, name: &str, flags: i32) -> Result<File> {
            let name = CString::new(name).map_err(|_| error("Invalid audio identifier."))?;
            // parent and the NUL-terminated name remain live during openat. A
            // successful descriptor is transferred exactly once into File.
            let fd = unsafe {
                libc::openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                return Err(error("The owned audio source could not be opened safely."));
            }
            Ok(unsafe { File::from_raw_fd(fd) })
        }
        let meeting = child(&directory, meeting, libc::O_RDONLY | libc::O_DIRECTORY)?;
        let file = child(&meeting, audio, libc::O_RDONLY | libc::O_NONBLOCK)?;
        if !file
            .metadata()
            .map_err(|_| error("Audio metadata is unavailable."))?
            .is_file()
        {
            return Err(error("The audio source is not a regular file."));
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        open_regular(&root.join(meeting).join(audio))
    }
}
pub fn open_regular(path: &Path) -> Result<File> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| error("This audio file could not be opened."))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(error("Choose a regular audio file, not a symbolic link."));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| error("This audio file could not be opened safely."))?;
    if !file
        .metadata()
        .map_err(|_| error("Audio metadata is unavailable."))?
        .is_file()
    {
        return Err(error("Choose a regular audio file."));
    }
    Ok(file)
}
#[derive(Clone, Debug)]
pub struct PcmWav {
    pub channels: u16,
    pub rate: u32,
    pub offset: u64,
    pub bytes: u64,
    pub repairable: bool,
}
impl PcmWav {
    pub fn frames(&self) -> u64 {
        self.bytes / (self.channels as u64 * 2)
    }
    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate as f64
    }
    pub fn length(&self) -> u64 {
        44 + self.bytes
    }
    pub fn header(&self) -> [u8; 44] {
        let mut h = [0u8; 44];
        h[..4].copy_from_slice(b"RIFF");
        h[4..8].copy_from_slice(&((self.bytes + 36) as u32).to_le_bytes());
        h[8..12].copy_from_slice(b"WAVE");
        h[12..16].copy_from_slice(b"fmt ");
        h[16..20].copy_from_slice(&16u32.to_le_bytes());
        h[20..22].copy_from_slice(&1u16.to_le_bytes());
        h[22..24].copy_from_slice(&self.channels.to_le_bytes());
        h[24..28].copy_from_slice(&self.rate.to_le_bytes());
        h[28..32].copy_from_slice(&(self.rate * self.channels as u32 * 2).to_le_bytes());
        h[32..34].copy_from_slice(&(self.channels * 2).to_le_bytes());
        h[34..36].copy_from_slice(&16u16.to_le_bytes());
        h[36..40].copy_from_slice(b"data");
        h[40..44].copy_from_slice(&(self.bytes as u32).to_le_bytes());
        h
    }
    pub fn read_range(&self, file: &mut File, start: u64, count: u64) -> Result<Vec<u8>> {
        if start >= self.length() || count > MAX_CHUNK {
            return Err(error("This audio range is invalid."));
        }
        let end = start.saturating_add(count).min(self.length());
        let mut out = Vec::with_capacity((end - start) as usize);
        let mut cursor = start;
        if cursor < 44 {
            let h = self.header();
            let stop = end.min(44);
            out.extend_from_slice(&h[cursor as usize..stop as usize]);
            cursor = stop;
        }
        if cursor < end {
            file.seek(SeekFrom::Start(self.offset + cursor - 44))
                .map_err(|_| error("Audio could not be read."))?;
            file.take(end - cursor)
                .read_to_end(&mut out)
                .map_err(|_| error("Audio could not be read."))?;
        }
        if out.len() as u64 != end - start {
            return Err(error("The audio changed or is incomplete."));
        }
        Ok(out)
    }
}
/// Scan only bounded chunk metadata. Unfinished canonical Tuck headers and
/// truncated PCM data can be replayed/recovered without modifying the original.
pub fn inspect(file: &mut File) -> Result<PcmWav> {
    let len = file
        .metadata()
        .map_err(|_| error("Audio metadata is unavailable."))?
        .len();
    if !(44..=u32::MAX as u64).contains(&len) {
        return Err(error("This WAV is empty or too large."));
    }
    file.rewind()
        .map_err(|_| error("Audio could not be read."))?;
    let mut riff = [0u8; 12];
    file.read_exact(&mut riff)
        .map_err(|_| error("This WAV header is incomplete."))?;
    if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        return Err(error("This file is not a PCM WAV recording."));
    }
    let declared = u32::from_le_bytes(riff[4..8].try_into().unwrap()) as u64 + 8;
    let mut at = 12u64;
    let mut spec = None;
    for _ in 0..128 {
        if at + 8 > len || at > 1024 * 1024 {
            break;
        }
        file.seek(SeekFrom::Start(at))
            .map_err(|_| error("Audio could not be read."))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)
            .map_err(|_| error("This WAV header is incomplete."))?;
        let size = u32::from_le_bytes(chunk[4..8].try_into().unwrap()) as u64;
        let offset = at + 8;
        if &chunk[..4] == b"fmt " {
            if !(16..=4096).contains(&size) || offset + size > len {
                return Err(error("This WAV format is unsupported."));
            }
            let mut fmt = [0u8; 16];
            file.read_exact(&mut fmt)
                .map_err(|_| error("This WAV format is incomplete."))?;
            let format = u16::from_le_bytes(fmt[..2].try_into().unwrap());
            let channels = u16::from_le_bytes(fmt[2..4].try_into().unwrap());
            let rate = u32::from_le_bytes(fmt[4..8].try_into().unwrap());
            let align = u16::from_le_bytes(fmt[12..14].try_into().unwrap());
            let bits = u16::from_le_bytes(fmt[14..16].try_into().unwrap());
            if format != 1
                || !matches!(channels, 1 | 2)
                || bits != 16
                || align != channels * 2
                || !(8000..=192000).contains(&rate)
            {
                return Err(error(
                    "Use mono or stereo 16-bit PCM WAV audio at 8–192 kHz.",
                ));
            }
            spec = Some((channels, rate));
        } else if &chunk[..4] == b"data" {
            let (channels, rate) =
                spec.ok_or_else(|| error("This WAV has no supported format header."))?;
            let available = len - offset;
            let unfinished =
                size == 0 && offset == 44 && channels == 1 && rate == 16000 && declared < len;
            let raw = if unfinished {
                available
            } else {
                size.min(available)
            };
            let bytes = raw - raw % (channels as u64 * 2);
            if bytes == 0 {
                return Err(error("This WAV contains no complete audio frames."));
            }
            return Ok(PcmWav {
                channels,
                rate,
                offset,
                bytes,
                repairable: unfinished || size > available || bytes != size,
            });
        }
        at = offset
            .checked_add(size)
            .and_then(|v| v.checked_add(size % 2))
            .ok_or_else(|| error("Invalid WAV chunk."))?;
    }
    Err(error("This WAV contains no supported audio data."))
}
/// Restore and ordinary imports preserve the exact validated source bytes,
/// including container metadata and unfinished headers. Repair is a separate act.
pub fn copy_exact(source: &Path, target: &Path) -> Result<u64> {
    let mut source = open_regular(source)?;
    inspect(&mut source)?;
    let length = source
        .metadata()
        .map_err(|_| error("Audio metadata is unavailable."))?
        .len();
    source
        .rewind()
        .map_err(|_| error("Audio could not be read."))?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| error("The audio copy could not be created."))?;
    let copied = std::io::copy(&mut source.take(length + 1), &mut destination)
        .map_err(|_| error("The audio copy could not be written."))?;
    if copied != length {
        return Err(error("The audio changed while it was copied."));
    }
    destination
        .sync_all()
        .map_err(|_| error("The audio copy could not be saved."))?;
    Ok(copied)
}
pub fn copy_normalized(source: &Path, target: &Path) -> Result<PcmWav> {
    let mut file = open_regular(source)?;
    let info = inspect(&mut file)?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| error("The recovery copy could not be created."))?;
    destination
        .write_all(&info.header())
        .map_err(|_| error("The recovery copy could not be written."))?;
    file.seek(SeekFrom::Start(info.offset))
        .map_err(|_| error("Audio could not be read."))?;
    let copied = std::io::copy(&mut file.take(info.bytes), &mut destination)
        .map_err(|_| error("The recovery copy could not be written."))?;
    if copied != info.bytes {
        return Err(error("The original audio changed while it was copied."));
    }
    destination
        .sync_all()
        .map_err(|_| error("The recovery copy could not be saved."))?;
    Ok(info)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn fixture(path: &Path, bytes: u64, unfinished: bool) {
        let info = PcmWav {
            channels: 1,
            rate: 16000,
            offset: 44,
            bytes,
            repairable: false,
        };
        let mut header = info.header();
        if unfinished {
            header[4..8].copy_from_slice(&36u32.to_le_bytes());
            header[40..44].copy_from_slice(&0u32.to_le_bytes());
        }
        let mut file = File::create(path).unwrap();
        file.write_all(&header).unwrap();
        file.set_len(44 + bytes).unwrap();
    }
    #[test]
    fn virtual_header_and_copy_repair_preserve_original() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        fixture(&source, 32000, true);
        let original = fs::read(&source).unwrap();
        let mut file = open_regular(&source).unwrap();
        let info = inspect(&mut file).unwrap();
        assert!(info.repairable);
        assert_eq!(info.duration(), 1.0);
        let mut header = std::io::Cursor::new(info.read_range(&mut file, 0, 44).unwrap());
        let reader = hound::WavReader::new(&mut header).unwrap();
        assert_eq!(reader.duration(), 16000);
        let target = dir.path().join("recovered.wav");
        copy_normalized(&source, &target).unwrap();
        assert_eq!(hound::WavReader::open(&target).unwrap().duration(), 16000);
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(copy_normalized(&source, &target).is_err());
    }
    #[test]
    fn only_owned_regular_audio_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        let id = Uuid::new_v4().to_string();
        let root = dir.path().join("audio");
        fs::create_dir_all(root.join(&id)).unwrap();
        fixture(&root.join(&id).join("microphone.wav"), 3200, false);
        assert!(owned_path(&root, &id, "microphone.wav").is_ok());
        assert!(open_owned(&root, &id, "microphone.wav").is_ok());
        for audio in [
            "../microphone.wav",
            "source.mp3",
            "/etc/passwd",
            "system.wav/../microphone.wav",
        ] {
            assert!(owned_path(&root, &id, audio).is_err());
        }
        assert!(owned_path(&root, "../other", "microphone.wav").is_err());
        #[cfg(unix)]
        {
            let external = dir.path().join("external.wav");
            fixture(&external, 3200, false);
            std::os::unix::fs::symlink(&external, root.join(&id).join("system.wav")).unwrap();
            assert!(owned_path(&root, &id, "system.wav").is_err());
            assert!(open_regular(&root.join(&id).join("system.wav")).is_err());
        }
    }
    #[test]
    fn malformed_and_empty_audio_are_not_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.wav");
        fixture(&source, 0, false);
        assert!(inspect(&mut File::open(&source).unwrap()).is_err());
        fs::write(&source, b"not audio").unwrap();
        assert!(inspect(&mut File::open(&source).unwrap()).is_err());
    }
}
