//! Bounded, local-only decoding into the same recoverable PCM evidence used by capture.
use super::{audio, speech::PcmResampler};
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Read, Seek, SeekFrom},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use symphonia::core::{
    codecs::audio::{AudioDecoder, AudioDecoderOptions},
    common::Limit,
    formats::{probe::Hint, FormatOptions, FormatReader, TrackType},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
};
use tuck_core::{ports::Result, AppError};

pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_SECONDS: u64 = 12 * 60 * 60;
const PROBE_BYTES: u64 = 32 * 1024 * 1024;
const PACKET_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 65_536;
const MAX_PACKETS: u64 = 10_000_000;

fn error(message: &str) -> AppError {
    AppError::new("audio_import_error", message)
}
fn decode_error(_: impl std::fmt::Display) -> AppError {
    error("This audio is unsupported, protected, corrupt, or exceeds the import limits. Use an unprotected WAV, MP3, FLAC, or AAC-LC file. Any decoded audio has been preserved for recovery.")
}
fn save_error(_: impl std::fmt::Display) -> AppError {
    error("Decoded audio could not be saved. Check available storage. The selected original and any saved partial audio have been preserved.")
}

/// Limit actual reads per parser operation, including metadata and packets whose
/// declared lengths are malicious. Seeking never replenishes this shared budget.
struct BudgetFile {
    file: File,
    len: u64,
    position: u64,
    budget: Arc<AtomicU64>,
}
impl Read for BudgetFile {
    fn read(&mut self, target: &mut [u8]) -> io::Result<usize> {
        if target.is_empty() {
            return Ok(0);
        }
        if self.position >= self.len {
            return Ok(0);
        }
        let remaining = self.budget.load(Ordering::Relaxed);
        if remaining == 0 {
            return Err(io::Error::other("Audio parser read limit exceeded"));
        }
        let limit = target
            .len()
            .min(remaining as usize)
            .min((self.len - self.position) as usize);
        let read = self.file.read(&mut target[..limit])?;
        self.position += read as u64;
        self.budget.fetch_sub(read as u64, Ordering::Relaxed);
        Ok(read)
    }
}
impl Seek for BudgetFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let next = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(n) => self.len.checked_add_signed(n),
            SeekFrom::Current(n) => self.position.checked_add_signed(n),
        };
        let next = next
            .filter(|n| *n <= self.len)
            .ok_or_else(|| io::Error::other("Seek outside selected audio"))?;
        self.position = self.file.seek(SeekFrom::Start(next))?;
        Ok(self.position)
    }
}
impl MediaSource for BudgetFile {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.len)
    }
}

pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    rate: Option<u32>,
    channels: Option<usize>,
    budget: Arc<AtomicU64>,
}
impl Decoder {
    pub fn open(path: &Path) -> Result<Self> {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !["wav", "mp3", "flac", "m4a", "mp4", "aac", "adts"].contains(&extension.as_str()) {
            return Err(error("Choose WAV, MP3, FLAC, or AAC-LC audio (.m4a, .mp4, .aac, .adts). Other codecs, including protected audio, are not supported."));
        }
        let file = audio::open_regular(path)?;
        let len = file.metadata().map_err(decode_error)?.len();
        if len == 0 || len > MAX_FILE_BYTES {
            return Err(error(
                "Choose an audio file smaller than 2 GiB and no longer than 12 hours.",
            ));
        }
        let budget = Arc::new(AtomicU64::new(PROBE_BYTES));
        let source = BudgetFile {
            file,
            len,
            position: 0,
            budget: budget.clone(),
        };
        let stream = MediaSourceStream::new(Box::new(source), Default::default());
        let mut hint = Hint::new();
        hint.with_extension(&extension);
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default()
                    .limit_tag_bytes(Limit::Maximum(64 * 1024))
                    .limit_visual_bytes(Limit::Maximum(0)),
            )
            .map_err(decode_error)?;
        let track = format
            .default_track(TrackType::Audio)
            .ok_or_else(|| error("This file has no supported audio track."))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| error("This file has no supported audio codec."))?;
        let rate = params.sample_rate;
        let channels = params.channels.as_ref().map(|c| c.count());
        validate_spec(rate, channels)?;
        if params
            .max_frames_per_packet
            .is_some_and(|n| n > MAX_FRAMES as u64)
            || params
                .extra_data
                .as_ref()
                .is_some_and(|v| v.len() > 64 * 1024)
            || track
                .num_frames
                .zip(rate)
                .is_some_and(|(n, r)| n > MAX_SECONDS * r as u64)
        {
            return Err(error(
                "This audio exceeds the supported packet or 12-hour duration limit.",
            ));
        }
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default().verify(true))
            .map_err(decode_error)?;
        let track_id = track.id;
        Ok(Self {
            format,
            decoder,
            track_id,
            rate,
            channels,
            budget,
        })
    }
    /// The target is always newly created. Checkpoint once per decoded second
    /// and flush on an ordinary error, avoiding a header seek for every packet.
    /// No source file is modified or removed.
    pub fn normalize_to(mut self, target: &Path) -> Result<f64> {
        let output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)
            .map_err(save_error)?;
        let sync = output.try_clone().map_err(save_error)?;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::new(BufWriter::new(output), spec).map_err(save_error)?;
        let mut samples = Vec::<f32>::new();
        let mut mono = Vec::<f32>::new();
        let mut resampler = None;
        let mut count = 0u64;
        let mut packets = 0u64;
        let mut checkpoint = 0u64;
        let decoded: Result<()> = (|| {
            loop {
                self.budget.store(PACKET_BYTES, Ordering::Relaxed);
                let Some(packet) = self.format.next_packet().map_err(decode_error)? else {
                    break;
                };
                packets += 1;
                if packets > MAX_PACKETS || packet.data.len() > PACKET_BYTES as usize {
                    return Err(error("This audio exceeds the supported packet limit. Saved partial audio remains available for recovery."));
                }
                if packet.track_id != self.track_id {
                    continue;
                }
                let buffer = self.decoder.decode(&packet).map_err(decode_error)?;
                let rate = buffer.spec().rate();
                let channels = buffer.spec().channels().count();
                validate_spec(Some(rate), Some(channels))?;
                if self.rate.is_some_and(|r| r != rate)
                    || self.channels.is_some_and(|c| c != channels)
                    || buffer.frames() > MAX_FRAMES
                {
                    return Err(error("Audio format changes or oversized frames are not supported. Saved partial audio has been preserved."));
                }
                self.rate = Some(rate);
                self.channels = Some(channels);
                if resampler.is_none() {
                    resampler = Some(PcmResampler::new(rate)?);
                }
                samples.resize(buffer.samples_interleaved(), 0.0);
                buffer.copy_to_slice_interleaved(&mut samples);
                mono.clear();
                mono.extend(samples.chunks_exact(channels).map(|frame| {
                    frame
                        .iter()
                        .map(|s| {
                            if s.is_finite() {
                                s.clamp(-1.0, 1.0)
                            } else {
                                0.0
                            }
                        })
                        .sum::<f32>()
                        / channels as f32
                }));
                write_pcm(
                    &mut writer,
                    &resampler.as_ref().unwrap().feed(&mono, false),
                    &mut count,
                )?;
                if count - checkpoint >= 16_000 {
                    writer.flush().map_err(save_error)?;
                    checkpoint = count;
                }
            }
            if let Some(resampler) = resampler {
                write_pcm(&mut writer, &resampler.feed(&[], true), &mut count)?;
            }
            if count == 0 {
                return Err(error("The selected file contains no decodable audio."));
            }
            if self.decoder.finalize().verify_ok == Some(false) {
                return Err(decode_error("Audio checksum failed"));
            }
            Ok(())
        })();
        if let Err(problem) = decoded {
            let _ = writer.flush();
            let _ = sync.sync_all();
            return Err(problem);
        }
        writer.finalize().map_err(save_error)?;
        sync.sync_all().map_err(save_error)?;
        Ok(count as f64 / 16_000.0)
    }
}
fn validate_spec(rate: Option<u32>, channels: Option<usize>) -> Result<()> {
    if rate.is_some_and(|r| !(8_000..=192_000).contains(&r))
        || channels.is_some_and(|c| !matches!(c, 1 | 2))
    {
        return Err(error(
            "Import supports mono or stereo audio from 8 kHz to 192 kHz.",
        ));
    }
    Ok(())
}
fn write_pcm(
    writer: &mut hound::WavWriter<BufWriter<File>>,
    samples: &[f32],
    count: &mut u64,
) -> Result<()> {
    if *count + samples.len() as u64 > MAX_SECONDS * 16_000 {
        return Err(error(
            "Audio imports are limited to 12 hours. Saved partial audio has been preserved.",
        ));
    }
    for sample in samples {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16)
            .map_err(save_error)?;
    }
    *count += samples.len() as u64;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, bits: u16, float: bool) {
        let mut wav = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: bits,
                sample_format: if float {
                    hound::SampleFormat::Float
                } else {
                    hound::SampleFormat::Int
                },
            },
        )
        .unwrap();
        for frame in 0..4_800 {
            let sample = ((frame as f32 * 0.06).sin() * 0.4).clamp(-1.0, 1.0);
            for _ in 0..2 {
                if float {
                    wav.write_sample(sample).unwrap();
                } else {
                    wav.write_sample((sample * ((1u64 << (bits - 1)) - 1) as f32) as i32)
                        .unwrap();
                }
            }
        }
        wav.finalize().unwrap();
    }
    #[test]
    fn wide_pcm_and_float_are_streamed_and_original_is_untouched() {
        let temp = tempfile::tempdir().unwrap();
        for (bits, float) in [(24, false), (32, false), (32, true)] {
            let input = temp.path().join(format!("source-{bits}-{float}.wav"));
            let output = temp.path().join(format!("out-{bits}-{float}.wav"));
            fixture(&input, bits, float);
            let original = std::fs::read(&input).unwrap();
            let duration = Decoder::open(&input)
                .unwrap()
                .normalize_to(&output)
                .unwrap();
            assert!((duration - 0.1).abs() < 0.002);
            assert_eq!(std::fs::read(&input).unwrap(), original);
            let mut wav = hound::WavReader::open(&output).unwrap();
            assert_eq!(wav.spec().sample_rate, 16_000);
            assert_eq!(wav.spec().channels, 1);
            assert!(wav.samples::<i16>().any(|v| v.unwrap().abs() > 100));
            assert!(Decoder::open(&input)
                .unwrap()
                .normalize_to(&output)
                .is_err());
        }
    }
    #[test]
    fn malformed_unsupported_and_bounded_sources_fail_readably() {
        let temp = tempfile::tempdir().unwrap();
        let bad = temp.path().join("bad.mp3");
        std::fs::write(&bad, b"garbage").unwrap();
        assert!(Decoder::open(&bad).is_err());
        let huge = temp.path().join("huge.wav");
        File::create(&huge)
            .unwrap()
            .set_len(MAX_FILE_BYTES + 1)
            .unwrap();
        assert!(Decoder::open(&huge).is_err());
        assert!(validate_spec(Some(192_001), Some(2)).is_err());
        assert!(validate_spec(Some(48_000), Some(6)).is_err());
        let budget = Arc::new(AtomicU64::new(3));
        let mut source = BudgetFile {
            file: File::open(&bad).unwrap(),
            len: 7,
            position: 0,
            budget,
        };
        let mut buffer = [0; 8];
        assert_eq!(source.read(&mut buffer).unwrap(), 3);
        source.seek(SeekFrom::Start(0)).unwrap();
        assert!(source.read(&mut buffer).is_err());
        assert!(source.seek(SeekFrom::End(1)).is_err());
    }
    #[test]
    fn encoded_fixtures_decode_to_bounded_pcm() {
        let temp = tempfile::tempdir().unwrap();
        for (name, bytes) in [
            (
                "tone.m4a",
                include_bytes!("../../test-data/audio/tone.m4a").as_slice(),
            ),
            (
                "tone.aac",
                include_bytes!("../../test-data/audio/tone.aac").as_slice(),
            ),
            (
                "tone.flac",
                include_bytes!("../../test-data/audio/tone.flac").as_slice(),
            ),
            (
                "silence.mp3",
                include_bytes!("../../test-data/audio/silence.mp3").as_slice(),
            ),
        ] {
            let input = temp.path().join(name);
            let output = temp.path().join(format!("{name}.wav"));
            std::fs::write(&input, bytes).unwrap();
            let duration = Decoder::open(&input)
                .unwrap()
                .normalize_to(&output)
                .unwrap();
            assert!(duration > 0.05 && duration < 2.0, "{name}: {duration}");
            assert_eq!(std::fs::read(&input).unwrap(), bytes);
            let wav = hound::WavReader::open(&output).unwrap();
            assert_eq!(wav.spec().bits_per_sample, 16);
            assert_eq!(wav.spec().sample_rate, 16_000);
        }
    }
    #[test]
    fn corrupt_late_packet_preserves_partial_pcm_and_source() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("bad.flac");
        let output = temp.path().join("partial.wav");
        let mut bytes = include_bytes!("../../test-data/audio/tone.flac").to_vec();
        let end = bytes.len() - 4;
        bytes[end] ^= 0x40;
        std::fs::write(&input, &bytes).unwrap();
        assert!(Decoder::open(&input)
            .unwrap()
            .normalize_to(&output)
            .is_err());
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
        let mut saved = audio::open_regular(&output).unwrap();
        assert!(audio::inspect(&mut saved).unwrap().duration() > 0.1);
    }
    #[test]
    #[ignore = "Requires an explicitly chosen public test fixture; never reads a private library"]
    fn external_public_encoded_fixture() {
        let input = std::env::var_os("TUCK_PUBLIC_CODEC_FIXTURE")
            .expect("Set TUCK_PUBLIC_CODEC_FIXTURE to a public audio fixture");
        let temp = tempfile::tempdir().unwrap();
        let duration = Decoder::open(Path::new(&input))
            .unwrap()
            .normalize_to(&temp.path().join("normalized.wav"))
            .unwrap();
        assert!(duration > 0.0 && duration <= MAX_SECONDS as f64);
    }
}
