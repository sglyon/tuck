"""Use omp authentication in ephemeral sessions. No other inference backend."""
import json
import hashlib
from pathlib import Path
import subprocess
import tempfile
import time
from .schema import Suggestions

POLICY = Path(__file__).with_name("policy.txt").read_text()
SHAPE = 'Return ONLY JSON {"suggestions":[{"speakerId":"B","name":"Sarah","strength":"tentative","evidenceSegmentIds":["s1","s2"]}]}. Return {"suggestions":[]} when abstaining.'


def parse_events(stdout):
    events = [json.loads(line) for line in stdout.splitlines() if line.startswith("{")]
    if any(e.get("type") == "tool_execution_start" for e in events):
        raise ValueError("Unexpected tool execution in inference-only evaluation.")
    messages = [e["message"] for e in events if e.get("type") == "message_end"
                and isinstance(e.get("message"), dict) and e["message"].get("role") == "assistant"]
    if not messages or messages[-1].get("stopReason") != "stop":
        raise ValueError("omp did not complete a successful assistant response.")
    message = messages[-1]
    text = "".join(b.get("text", "") for b in message.get("content", []) if b.get("type") == "text").strip()
    if text.startswith("```json\n") and text.endswith("```"):
        text = text[8:-3].strip()
    output = Suggestions.model_validate_json(text).model_dump()
    # Store sanitized model output/usage only, not CLI session IDs, credential IDs or environment metadata.
    return output, {"model": message.get("model"), "usage": message.get("usage"),
                    "inferenceSeconds": message.get("duration", 0) / 1000}


def infer(state, model, candidates=None):
    instruction = "Infer conservative speaker-name suggestions. "
    payload = state.copy()
    if candidates is not None:
        payload["candidates"] = [{"speakerId": s["speakerId"], "name": s["name"]} for s in candidates]
        instruction = "Independently verify these untrusted candidates against all supplied segments; look for counterevidence and abstain on ambiguity. "
    prompt = instruction + SHAPE + "\n\n" + json.dumps(payload)
    args = ["omp", "-p", "--model", model, "--no-tools", "--no-lsp", "--no-extensions",
            "--no-skills", "--no-rules", "--no-session", "--no-title", "--thinking", "low",
            "--max-time", "180", "--mode", "json", "--system-prompt", POLICY, prompt]
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="tuck-name-eval-") as cwd:
        result = subprocess.run(args, text=True, capture_output=True, cwd=cwd, timeout=210)
    if result.returncode:
        raise RuntimeError(f"omp failed (exit {result.returncode}). Check omp authentication/capacity.")
    output, metadata = parse_events(result.stdout)
    metadata["elapsedSeconds"] = time.monotonic() - started
    metadata["promptSha256"] = hashlib.sha256((POLICY + "\n" + prompt).encode()).hexdigest()
    return output, metadata
