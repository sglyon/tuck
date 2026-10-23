# Live meeting agents

Tuck’s meeting agents watch recent speech and offer optional help during an explicitly enabled call. They are configurable monitors inside the existing Meeting Workspace, using its selected IntelligencePort. They do not add an agent framework, a separate service, or a new persistence schema.

## Product behavior

Two editable profiles ship initially: **Quiet meeting helper** and **Sales coach**. Users can save up to eight agents, each with a name, instructions, and a quiet or active pace. A call can select up to three saved agents. Custom instructions determine what to notice; they cannot authorize tools, external messages, autonomous tasks, invented commitments, or changes to the reliability rules.

Agents run only when a regular meeting is actively recording and the user enables them for that call. Voice notes, imports, drafts, paused recordings and stopped recordings never initiate inference. Pausing invalidates an outstanding response; resuming keeps the per-call opt-in. Stopping, interruption, shutdown or restart turns monitoring off. Changing AI provider, model, hosted consent, local endpoint, ChatGPT account, or OpenAI credential also turns it off so a newly selected destination requires a fresh per-call opt-in. Editing agent goals invalidates an outstanding completion and the next eligible snapshot uses the changed instructions.

No meeting notes, title, participant metadata, calendar data, audio, chat history, or full transcript are included in automatic agent prompts. The payload contains the selected agents’ instructions, a bounded window of recent transcript evidence, and a few earlier agent tips. Hosted processing still requires the existing hosted-AI consent. No web tools are enabled. A transcript source remains untrusted evidence even if it contains instructions addressed to an assistant.

## Resource policy

- A host worker may call `coach_tick()` periodically. The core contains no timer, detached worker or provider client.
- One completion serves all currently eligible selected profiles. There are no per-agent threads or model instances.
- The first check waits at least 30 seconds after the first enabled worker tick. Provider request starts are at least 30 seconds apart, including repeated enable or profile-selection actions. Manual chat/recap work prevents a background request from starting and invalidates pending background output.
- Initial evidence needs at least twelve words. After a successful snapshot, each agent needs at least eight newly observed words or changed goals before being considered again. An empty model response is a successful snapshot and does not cause unchanged speech to be resent.
- Quiet agents produce at most one card per 90 seconds; active agents at most one per 30 seconds. Each completion can produce zero to two cards, with at most one per eligible agent. Three open cards across all agents stops further inference until the user clears or saves one.
- Repository reads hydrate at most 64 latest effective segments, including transcript corrections, in chronological order. SQL applies the limit before decoding documents. Context further retains only the latest five minutes and at most 6,000 UTF-8 bytes of encoded evidence.
- Selected profile JSON is capped at 6,000 UTF-8 bytes. Overlarge selections receive an instruction to shorten the goals or select fewer agents; instructions are never silently truncated.
- The system prompt plus serialized messages never exceeds 12,000 UTF-8 bytes. Earlier tips and then oldest transcript segments are removed to fit. An insufficient remaining evidence window skips inference. Arbitrary Unicode and JSON escaping count toward the actual encoded budget.
- The native adapter separately owns inference context, output, thread and timeout limits, and gives manual requests priority. An adapter without an explicitly bounded `complete_coach` implementation fails closed.
- Failures use 60, 120, then 240 seconds of retry backoff, with a maximum of 300 seconds in the policy. Repeated enable cannot reset the current call’s cadence or backoff. Recording continues independently of AI availability.

These are deterministic work bounds, not a claimed minimum-machine benchmark or a guarantee of useful model judgment.

## Evidence and follow-through

A completion must be JSON `null` or an array of no more than two objects containing exactly `agentId`, `kind`, `text`, `segmentId` and `quote`. Each agent ID must be eligible for that batch, each agent can appear once, and kinds are `question`, `follow_up`, `reminder` or `insight`. Text is at most 280 characters, the source quote is an exact 12–240 character excerpt from a supplied segment, and the complete answer is at most 3,000 characters. Invalid batches are rejected as a unit. Model timestamps are never accepted; the core derives the source time from stored evidence.

Before persisting output, the core checks recording state, per-call generation, current settings and the effective source text. Disable, pause/resume, stop, a manual AI request, credential changes, altered agent goals and evidence changes invalidate pending results. A new speech segment does not invalidate unchanged evidence already sent. Corrections made after recording dismiss open advice when its quote no longer occurs in the corrected source. A task already saved by the user remains user-owned.

Suggestions are optional interpretations, not verified facts. Exact quote validation proves a source exists; it does not prove that a generated inference follows from it. The prompt prohibits invented attribution, commitments, owners and deadlines, and the interface preserves the quoted source for review.

There are at most three open suggestions and 24 retained suggestions per meeting. This is a rolling retained-history limit, not a lifetime quota; the oldest closed suggestion is evicted when necessary. Normalized text and same-agent/source quotes deduplicate the retained history. Each suggestion snapshots its agent name and ID so later profile edits do not rewrite history.

Dismiss is explicit. Saving a `follow_up` or `reminder` explicitly creates one existing ActionItem, with no inferred owner or date, and records its ID on the source suggestion. Repeating Save is idempotent. Questions and insights do not create tasks. Saving does not schedule an OS notification or send any message.

## Contract

Settings extend the existing validated `update_settings` path:

```json
{"meetingAgents":[{"id":"UUID","name":"Sales coach","instructions":"Notice unclear buying criteria.","pace":"active"}]}
```

Profiles have exactly those four fields; names are at most 80 characters, instructions at most 4,000 characters, IDs unique UUIDs, and pace either `quiet` or `active`. Empty profile lists are valid; an active call with no remaining selected saved agents reports that another selection is required.

- `set_live_coach {meetingId, enabled, agentIds?}` → coach state. At most three selected IDs; enabling requires a live regular meeting. Without explicit IDs, reuse that call’s selection or select the first available profile.
- `get_live_coach {meetingId}` → `{enabled,status,message,selectedAgentIds,suggestions}`.
- `dismiss_coach_suggestion {meetingId,id}` → updated coach state.
- `save_coach_follow_up {meetingId,id}` → `{coach,meeting}`.
- `coach_updated {meetingId,coach}` follows committed changes and inference-state transitions.

Statuses are `off`, `waiting`, `analyzing`, `paused`, `unavailable` and `stopped`. `unavailable` describes a temporary provider problem or missing selected profiles; it does not imply that transcription failed. The host calls `stop_live_coaching()` before joining its worker at shutdown. `coach_tick_at(device_time_ms)` is a deterministic test seam and is not an RPC.

Durable `Meeting.coachSuggestions` defaults to an empty list for existing documents. Enabled state, pending state, selections, cadence, backoff and evidence cursors stay in transient service state. Backups validate bounded suggestion bodies, identities and actual source IDs/times. Open advice must still quote raw or effective evidence. Closed history retains its bounded historical quote even after later corrections replace that wording; saved tasks remain user-owned. Restore never enables monitoring. Meeting deletion removes suggestions with their aggregate. Notion publication continues to include only its explicit allow-listed content; unsaved agent advice is not published.

## Architecture selection

Trustworthy meeting understanding remains Tuck’s Core Domain. Cadence and opt-in policy are supporting application rules within that same context; external model schemas remain behind IntelligencePort. Meeting owns bounded suggestion history and explicit follow-through, while the host owns scheduling and the adapter owns inference resource limits. Existing Store/Repository transactions preserve atomic Save and evidence checks. This follows the DDD selecting-patterns guidance to use the already established context and scale tactical modeling to the actual consistency requirements.

## Verification

Core contract tests run against both in-memory and SQLite repositories where storage behavior matters. They cover per-call/restart gating, actual source grounding, exact timestamp derivation, multi-profile batches, individual paces, three-open backpressure, deduplication, malformed batches, retry backoff, stale lifecycle/settings results, bounded ordered corrected evidence, UTF-8 request budgets, private-note exclusion, meaningful incremental speech, explicit idempotent tasks, correction retirement, and portable backup validation.

Provider transport/resource tests and browser/native interface verification are separate acceptance evidence; fake-provider core tests do not establish model quality or live-call performance.
