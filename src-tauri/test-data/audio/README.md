# Synthetic decoder fixtures

These small fixtures contain no recorded speech or private audio. `tone.m4a`
and `tone.aac` are a 0.1-second 440 Hz synthetic tone, generated as mono
44.1 kHz PCM16 and encoded with macOS `afconvert -f m4af -d aac` and
`afconvert -f adts -d aac`. `tone.flac` is the same tone for one second,
encoded with `afconvert -f flac -d flac` (the shorter FLAC does not flush
a complete OS encoder block).

`silence.mp3` is constructed directly: three MPEG-1 Layer III mono
128 kbps/44.1 kHz frames, each `ff fb 90 c0` followed by 413 zero bytes.
It exercises the MP3 decoder without third-party media. All fixtures are
dedicated to the public domain under CC0-1.0.

The opt-in `external_public_encoded_fixture` additionally checks an explicitly
provided public source. The development check used the upstream minimp3
sine-wave conformance vector at
https://github.com/lieff/minimp3/blob/master/vectors/l3-sin1k0db.bit,
downloaded only to a temporary directory and renamed `.mp3`.
