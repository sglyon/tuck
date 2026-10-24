# Tuck runtime contract

Tauri exposes `engine_call({method,params})` and calls the Rust application core in-process. The diagnostic native `tuck-runtime` also supports JSON-lines over stdin/stdout. Request: `{id,method,params}`. Reply: `{id,result}` or `{id,error:{code,message}}`. Event: `{event,payload}`. Never print diagnostics, credentials, or transcripts outside this protocol. The desktop host emits `engine-event` after committed application updates. Methods use snake_case; JSON data uses camelCase.

## Data

Meeting `{id,title,createdAt,duration,status,notes,summary,template,starred,isExample,notionUrl,notionStatus,segments,speakers,messages,bookmarks}`.
Status: `draft|recording|paused|processing|complete|interrupted`.
Notion status: `idle|syncing|synced|error|uncertain`.
Segment `{id,meetingId,start,end,text,speakerId,source}`. Source `microphone|system|import`. Times in seconds. Never derive names from diarization.
Speaker `{id,name,color}`. First speaker `you` / `You` for microphone, remote `speaker-1` / `Speaker 1` etc. Color numeric 0..5. Speaker names are meeting-scoped and editable.
Message `{id,role,content,createdAt}` role `user|assistant`. Bookmarks `{id,time,label}`.
Settings `{provider,model,localUrl,hostedConsent,webSearch,autoSummary,notionAutoSync,notionTarget,threads,keepAudio}`. provider `local|chatgpt|openai`. Default local; model `qwen3:4b`; localUrl `http://127.0.0.1:11434`; hostedConsent=false; webSearch=false; autoSummary=false; notionAutoSync=false; notionTarget=""; threads=2; keepAudio=true. Extra public flags `hasOpenAIKey,hasNotionToken`.
Status `{engineVersion,platform,modelInstalled,modelDownloading,modelProgress,speakerModelInstalled,activeMeetingId,recordingState,captureAvailable}` plus optional `error`. No fabricated memory/latency metrics.
ChatGPT account `{id,email,label,connected,sharing,active}`. No credentials in public DTOs.
Model option `{id,name}`.

## RPC

- bootstrap -> `{meetings:Meeting[],settings:Settings,status:Status,accounts:Account[]}` (all local data; no network on startup)
- create_meeting `{title?:string}` -> Meeting
- update_meeting `{id,changes:{title?,notes?,template?,starred?}}` -> Meeting
- delete_meeting `{id}` -> null (reject active; removes owned audio)
- rename_speaker `{meetingId,speakerId,name}` -> Meeting
- add_bookmark `{meetingId,time,label}` -> Meeting
- remove_bookmark `{meetingId,id}` -> Meeting
- load_example -> Meeting (explicit user action; synthetic and labeled)
- start_recording `{meetingId,systemAudio:boolean,microphone:boolean}` -> Meeting (fail closed if requested source unavailable)
- pause_recording `{meetingId}` -> Meeting
- resume_recording `{meetingId}` -> Meeting
- stop_recording `{meetingId}` -> Meeting (drain inference before complete; preserve recorded audio on failure)
- import_audio `{path,title?}` -> Meeting (native file dialog path, validate WAV supported, local copy)
- install_model `{kind:"parakeet"|"speakers"}` -> status/result. Explicit large download only. Emit model_progress.
- unload_model -> null (reject recording)
- get_status -> Status
- get_settings -> Settings
- update_settings `{changes:Partial<Settings>}` -> Settings (allowlist and validate)
- save_secret `{kind:"openai"|"notion",value}` -> Settings. Native credential store only.
- chat `{meetingId,prompt,requestId}` -> Message. Context snapshot + prior messages; emit chat_delta `{meetingId,requestId,delta}`; final saved Message returned. No external tools without webSearch and hostedConsent.
- summarize `{meetingId}` -> Meeting; emit meeting_updated when saved. Ground output in transcript, timestamp citations `[mm:ss]`, template-aware; preserve notes.
- publish_notion `{meetingId}` -> Meeting; only summary + notes (no audio), at-most-once automatic create, uncertain network outcome blocks retry to prevent duplicates.
- chatgpt_sign_in `{accountId?:string}` -> Account[] (opens browser from native adapter; loopback OAuth)
- chatgpt_accounts -> Account[]
- chatgpt_select `{id}` -> Account[]
- chatgpt_sign_out `{id}` -> `{accounts:Account[],remoteRevoked:boolean}`
- list_models -> Model[] (explicit provider network request)
- export_meeting `{meetingId,format:"markdown"|"json"|"srt",path?:string}` -> `{content:string,path?:string}`. Native save path; never allow arbitrary read access beyond native chosen import.
- shutdown -> null

Global dictation adds native status/toggle/cancel commands and `global_dictation_updated` with a direct status payload; see the [dictation host contract](dictation-contract.md) for exact fields and delivery boundaries. It reuses the core voice-note lifecycle.

## Events

- meeting_updated `{meeting:Meeting}` (persist BEFORE emitting)
- segment `{meetingId,segment:Segment}` (persist BEFORE emitting)
- recording_state `{meetingId,state}`
- audio_level `{microphone:number,system:number}` values 0..1, <=10Hz
- model_progress `{kind,progress,stage}`
- chat_delta `{meetingId,requestId,delta}`
- engine_error `{code,message,meetingId?}` (safe human text, never raw network response bodies)

## Adapter contracts (reference implementation)

`CaptureEngine(data_dir:Path, emit:Callable[[str,dict],None])` in adapters/capture.py.
`status()->dict` model/capture fields above.
`start(meeting_id:str,on_segment:Callable[[dict],None],system_audio:bool,microphone:bool,threads:int=2,keep_audio:bool=True)->None`.
`pause()->None`, `resume()->None`, `stop()->float` (duration; drains callbacks before return), `shutdown()->None`, `install_model(kind:str)->dict`, `unload()->None`, `import_file(path:str,meeting_id:str,on_segment:Callable[[dict],None],threads:int=2)->float`.
Segment callback dict `{id,meetingId,start,end,text,speakerId,source}`; UUID ids. Engine assigns provisional speaker IDs, no names. Audio lives at `data_dir/audio/{meeting_id}/` only. No domain/repository imports; orchestration owns persistence.

`SecretStore()` in adapters/secrets.py: `.get(kind)->str|None`, `.set(kind,value)->None`, `.delete(kind)->None`; native keyring, fail closed if unavailable.
`ChatGPTAuth(data_dir:Path,secrets:SecretStore)`: `.accounts()->list[dict]`, `.sign_in(account_id=None)->list[dict]`, `.select(id)->list[dict]`, `.sign_out(id)->dict`, `.access_token()->str` (refresh + permissions), `.models()->list[dict]`.
`Intelligence(auth:ChatGPTAuth,secrets:SecretStore)`: `.models(settings:dict)->list[dict]`; `.complete(settings:dict,system:str,messages:list[dict],on_delta:Callable[[str],None])->str`. Hosted consent checked here; public Responses endpoint only, store=false, stream=true. Local Ollama only loopback endpoint unless future explicit remote setting. `.cancel` optional.
`NotionPublisher(secrets:SecretStore)`: `.publish(target:str,meeting:dict)->str` URL. Must throw `PublishUncertain` for network ambiguity AFTER create starts, before returning URL. Domain service persists syncing first and blocks uncertain retry.

## Ownership

Domain, application services, ports, SQLite repository, RPC and Rust bridge: backend agent.
Local capture, model manager, Parakeet/speaker runtime and native helpers: capture agent.
React UI, demo browser adapter, UI tests: interface agent.
OpenAI authorization, AI providers, Notion and secure secrets: root agent.

## Native production ports

The canonical production traits are `crates/tuck-core/src/ports.rs`; the application service owns their semantics. Concrete adapters are composed in `src-tauri/src/runtime.rs`. SQLite schema version 4 opens version 1 libraries through additive migrations, with defaults for newer metadata. Reverse compatibility with the Python reference is not promised. Desktop startup uses the [paged loading contract](library-loading-contract.md); full `bootstrap` remains for diagnostics. See [Rust core](rust-core.md) for persistence, [workflow contracts](workflow-contract.md) for writing and organization, [meeting-day contracts](meeting-day-contract.md) for occurrence preparation and optional reminders, [library safety](library-safety-contract.md) for corrections/playback/archives and [audio import](native-audio-import.md) for supported codecs. Python is not a production runtime dependency.
