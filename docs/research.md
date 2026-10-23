# Product and implementation research

Reviewed October 23, 2026. Vendor descriptions are feature claims, not independent performance or privacy audits. No competitor code was copied. This is a representative survey of leading meeting and dictation products, not an exhaustive ranking.

| Product | Relevant strength | Lesson for Tuck |
| --- | --- | --- |
| [Muesli](https://github.com/Muesli-HQ/muesli) | Native local meeting transcription and dictation, separate mic/system sources, on-demand speech models. | Make capture and local processing reliable before adding automation. Keep names distinct from speaker clusters. |
| [Granola](https://www.granola.ai/) | Bot-free computer audio, personal notes enhanced into meeting notes, contextual chat. | Notes should remain editable and human-owned; AI should sit beside the conversation. |
| [Wispr Flow](https://wisprflow.ai/) | Low-friction dictation across apps, polished writing, a meeting-notes offering. | Make the primary task one obvious action. Avoid asking users to think about speech engines. |
| [Superwhisper](https://superwhisper.com/) | Offline voice-to-text, custom vocabulary and task-specific modes. | Local transcription must remain useful without a cloud account. Purposeful templates beat a giant prompt editor. |
| [Fathom](https://www.fathom.ai/) | Meeting summaries, follow-ups, searchable conversation knowledge; offers bot and bot-free capture. | Saved evidence should lead directly to decisions and next steps. Bot-free alone is not differentiation. |
| [Jamie](https://www.meetjamie.ai/) | Bot-free capture, preparation and post-meeting workflows, including in-person conversations. | The workflow begins and ends with the person's work, not a transcript viewer. |
| [Bluedot](https://www.bluedothq.com/) | Bot-free capture and downstream CRM/Notion updates. | Publishing should be a configured destination and a clear status, not a repeated export chore. |
| [Otter](https://help.otter.ai/hc/en-us/articles/35973988280215-Otter-Desktop-App-Mac-Windows) | Desktop bot-free system capture with live transcripts and meeting detection. | Modern competitors also avoid bots; earn trust with transparent local processing and useful evidence. |
| [Fireflies](https://fireflies.ai/blog/bot-free-meeting-recorder) | Desktop, extension and platform capture plus downstream workflows. | Separate capture method from processing location; “bot-free” does not itself mean “local AI.” |

## Product decisions

Tuck combines local capture and recognition with a simple meeting desk: library, notes/transcript/recap, and a live assistant. There is no live recording animation pretending to process audio in the browser. Examples are explicit and labeled. The first native launch has an empty library.

Private transcription is the default. AI is optional and uses either local Ollama or a deliberately connected hosted provider. Hosted processing and web lookup have separate consent controls. A model's ability to search is checked through real request success, never inferred from its name. Returned web citations become source links.

No per-token processing of raw live audio in an LLM: speech is segmented by a local VAD, decoded by one bounded inference worker, then persisted as evidence. Chat reads a transcript snapshot. Model weights load only when needed, threads are limited, silence does not run ASR, queues are bounded, and WAV audio is written incrementally. Resource claims must come from measurements on specified hardware.

## Technical sources

- [Tauri command bridge](https://v2.tauri.app/develop/calling-rust/): a small native host owns local runtime access, while the webview receives public DTOs and events.
- [Sherpa Parakeet models](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html): NVIDIA Parakeet TDT has a quantized ONNX path with `nemo_transducer`, providing a CPU baseline without installing PyTorch/NeMo. Chunked offline decoding is not advertised as a token-streaming ASR model.
- [OpenAI sign-in cookbook](https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt): eligible users may authorize ChatGPT plan usage in local/open-source apps. It does not expose their ChatGPT conversations.
- [Registration and sign-in](https://developers.openai.com/siwc/token-sharing-open-source/sign-in): installation host ID, dynamic initial registration, loopback callback, PKCE/state/nonce, issued-client retention and ID-token verification.
- [Models and inference](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference): account model catalog and public Responses endpoint with `store:false`, `stream:true`; successful completion requires the terminal completion event.
- [Preview limits](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations): use instructions and full request context; omit unsupported fields; web lookup depends on account/model policy. No use of undocumented ChatGPT backend endpoints.
- [Account sessions](https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions): separate registrations, serialized refresh rotation and revocation on sign-out. Tuck reports when remote revocation cannot be confirmed.
- [Notion page creation](https://developers.notion.com/reference/post-page): publish under a data source and map its actual title property, with page/block size limits. Database links are resolved explicitly; multiple sources require an exact selection.
- [Ollama chat](https://docs.ollama.com/api/chat): local NDJSON streaming, explicit completion, bounded context and short model keep-alive.

Context7 was requested as the preferred documentation source but was not available in this session's tool catalog. Official documentation and upstream repositories were used instead.

## Mobile boundary

Tauri provides a route to shared interface code, but desktop system-audio APIs are not mobile call-capture APIs. Native mobile work needs local platform inference/audio adapters, microphone permission handling and background recording lifecycle. Cross-app call/system audio is restricted by mobile operating systems; desktop capture parity is not promised. Reusable meeting use cases and stable IDs help, but do not make mobile support automatic.

## What requires real-world validation

Check microphone permission denial/revocation, system recording permission, Bluetooth/default-device changes, sleep/wake, long calls, overlapping speech and loudspeaker echo on each OS. Verify supported language accuracy and the privacy implications of hosted providers with real account configurations. Automated fakes establish lifecycle and boundary correctness, not audio quality or meeting-platform certification.

## Native runtime follow-up

The initial Python prototype delegated inference to native C++ already. SGL correctly challenged its packaging and resource footprint. Production is being replaced with a portable Rust core and native adapters. The controlled comparison and crate evaluation are in [Rust evaluation](rust-evaluation.md); the separately measured Apple Core ML path is in [Apple speech evaluation](apple-speech-evaluation.md). Language alone did not explain the model memory cost; model conversion and execution backend matter more. Do not generalize one public clip on a 32 GiB Mac to minimum-spec or long-meeting qualification.
