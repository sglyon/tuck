# Native audio import

The native importer uses [Symphonia 0.6.1](https://docs.rs/symphonia/0.6.1/symphonia/)
and accepts `.wav`, `.mp3`, `.flac`, `.m4a`, `.mp4`, `.aac`
and `.adts`. Compressed codec support is MP3, FLAC and AAC-LC. A container
extension does not guarantee its codec is supported: protected media, ALAC,
Opus and Vorbis are not enabled. Only the default audio track is imported.

Imports are local and bounded to 2 GiB, 12 hours, one or two channels and
8–192 kHz. Symphonia 0.6.1 runs synchronously under the existing import
operation guard. Parser reads are limited to 32 MiB during probing and
16 MiB per packet operation; encoded packets are limited to 16 MiB,
decoded packets to 65,536 frames, and the entire stream to ten million
packets. Metadata tags are limited to 64 KiB; embedded visuals are disabled.
The reader uses a fixed file-length snapshot, and seeks do not replenish
the read budget. Unsupported or corrupt files produce a readable failure.

Ordinary supported 16-bit PCM WAV is copied byte-for-byte into the new
meeting. Repairable interrupted WAV headers are rebuilt in the new copy.
Other accepted files stream through a decoder, stereo-to-mono conversion
and the existing band-limited native resampler into 16 kHz mono PCM16 WAV.
Decoded buffers are reused; the entire recording is never loaded at once.
The selected original is never modified or removed. A WAV checkpoint is
flushed once per decoded second and on ordinary decoder errors, preserving
partial local evidence for the existing new-meeting recovery action.

AAC container edit lists are not applied by Symphonia 0.6.1. Encoder priming
or padding can therefore remain in the normalized audio. The displayed
duration and transcript timestamps refer to the actual retained normalized
WAV. The public Kennedy M4A check contained 123 ms of extra padding compared
with its source WAV; this importer does not claim sample-exact AAC trimming.

`src-tauri/test-data/audio` contains tiny synthetic AAC-LC M4A/raw AAC,
FLAC and MP3 fixtures. Tests additionally generate PCM24/32/float WAV and
check conversion, original-byte preservation, no overwrite, parser bounds,
unsupported inputs and a late corrupt FLAC packet leaving usable partial
audio. An opt-in public MP3 sine-vector check also passed. No test reads
the user's library or opens an audio device.

For a real local speech integration check, explicitly provision the existing
public Kennedy fixture and verified weights, convert it to M4A with macOS
`afconvert -f m4af -d aac`, and run:

```sh
TUCK_VERIFY_DATA=/absolute/isolated-library \
TUCK_VERIFY_FIXTURE=/absolute/public-speech.m4a \
TUCK_VERIFY_DURATION=3.968 \
cargo test --manifest-path src-tauri/Cargo.toml --lib \
  adapters::capture::tests::real_speech_fixture -- --ignored --nocapture
```

The opt-in ASR test retains its original WAV expectation of 3.845 seconds
unless `TUCK_VERIFY_DURATION` is supplied for an encoded fixture.

Portable ZIP backup metadata streams directly into the staged ZIP through
a size-limited serializer. It is limited to 128 MiB, at most
10,000 entries and 20 GiB of audio. Backups and restore preserve the exact
owned WAV bytes; conversion happens only during explicit audio import or
recovery, not during backup/restore.
