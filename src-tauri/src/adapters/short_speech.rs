//! Final-pass matching of brief omitted replies. No new voices or template writes.
use super::{
    models::ModelManager, refinement::Diarization, speech::helper_path, voice_memory::VoiceMemory,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

fn bounds(start: f64, end: f64, pad: f64, analysis: &Diarization) -> Option<(f64, f64)> {
    let mut left = (start - pad).max(0.);
    let mut right = end + pad;
    for turn in &analysis.segments {
        if turn.speaker_id == "unassigned" {
            continue;
        }
        if turn.start < end && turn.end > start {
            return None;
        }
        if turn.end <= start {
            left = left.max(turn.end);
        }
        if turn.start >= end {
            right = right.min(turn.start);
        }
    }
    (right > left).then_some((left, right))
}
fn eligible(segment: &Value) -> Option<(f64, f64)> {
    if segment["speakerId"] != "unassigned"
        || !matches!(segment["source"].as_str(), Some("system" | "mixed"))
    {
        return None;
    }
    let (a, b) = (segment["start"].as_f64()?, segment["end"].as_f64()?);
    (a.is_finite()
        && b.is_finite()
        && (0.5..=1.5).contains(&(b - a))
        && segment["text"].as_str()?.split_whitespace().count() <= 8)
        .then_some((a, b))
}
#[derive(Deserialize)]
struct Probe {
    id: String,
    embedding: Vec<f32>,
}
#[derive(Deserialize)]
struct Response {
    source: String,
    spans: Vec<Probe>,
}
fn extract(
    models: &ModelManager,
    path: &Path,
    source: &str,
    requests: &[Value],
) -> Option<HashMap<String, Vec<f32>>> {
    if requests.is_empty() || requests.len() > 128 {
        return None;
    }
    let mut child = Command::new(helper_path("tuck-speech", "TUCK_SPEECH_HELPER").ok()?)
        .arg("--embed-spans")
        .arg(models.diarization_dir())
        .arg(path)
        .arg(source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let input = (|| -> std::io::Result<()> {
        let mut pipe = child.stdin.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
        pipe.write_all(&serde_json::to_vec(requests)?)?;
        Ok(())
    })();
    if input.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let r = stdout
            .take(2 * 1024 * 1024 + 1)
            .read_to_string(&mut text)
            .map(|_| text);
        let _ = tx.send(r);
    });
    let output = match rx.recv_timeout(Duration::from_secs(45)) {
        Ok(Ok(v)) if v.len() <= 2 * 1024 * 1024 => v,
        _ => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    if !child.wait().ok()?.success() {
        return None;
    }
    let payload = output
        .lines()
        .find_map(|line| line.strip_prefix("TUCK_SPAN_EMBEDDINGS:"))?;
    let response: Response = serde_json::from_str(payload).ok()?;
    if response.source != source || response.spans.len() > requests.len() {
        return None;
    }
    let ids: HashSet<_> = requests.iter().filter_map(|r| r["id"].as_str()).collect();
    let mut result = HashMap::new();
    for probe in response.spans {
        if !ids.contains(probe.id.as_str())
            || probe.embedding.len() != 256
            || probe.embedding.iter().any(|v| !v.is_finite())
            || result.contains_key(&probe.id)
        {
            return None;
        }
        result.insert(probe.id, probe.embedding);
    }
    Some(result)
}

pub(crate) fn recover(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    analyses: &HashMap<String, Diarization>,
    memory: &VoiceMemory,
    segments: &mut [Value],
) {
    let Ok(matcher) = memory.matcher() else {
        return;
    };
    let allowed: HashSet<_> = analyses
        .values()
        .flat_map(|a| a.segments.iter())
        .filter(|t| t.speaker_id.starts_with("voice-"))
        .map(|t| t.speaker_id.clone())
        .collect();
    if allowed.is_empty() {
        return;
    }
    let candidates = segments
        .iter()
        .enumerate()
        .filter_map(|(i, s)| eligible(s).map(|(a, b)| (i, a, b)))
        .take(64)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return;
    }
    let mut outputs = HashMap::new();
    for source in ["system", "microphone"] {
        let Some((_, path)) = paths.iter().find(|(s, _)| s == source) else {
            continue;
        };
        let Some(analysis) = analyses.get(source) else {
            continue;
        };
        let mut requests = Vec::new();
        for &(index, start, end) in &candidates {
            if source == "microphone" && segments[index]["source"] != "mixed" {
                continue;
            }
            let mut pair = Vec::new();
            for (probe, pad) in [0.15, 0.30].into_iter().enumerate() {
                if let Some((a, b)) = bounds(start, end, pad, analysis) {
                    pair.push(json!({"id":format!("{index}:{probe}"),"start":a,"end":b}));
                }
            }
            if pair.len() == 2 {
                requests.extend(pair);
            }
        }
        if let Some(result) = extract(models, path, source, &requests) {
            outputs.insert(source, result);
        }
    }
    let probes = |source: &str, index: usize| -> Option<Vec<Vec<f32>>> {
        let result = outputs.get(source)?;
        Some(vec![
            result.get(&format!("{index}:0"))?.clone(),
            result.get(&format!("{index}:1"))?.clone(),
        ])
    };
    for (index, _, _) in candidates {
        let Some(primary) = probes("system", index) else {
            continue;
        };
        let Some(identity) = matcher
            .identify_short(&primary)
            .filter(|id| allowed.contains(id))
        else {
            continue;
        };
        // Mixed paragraphs need matching evidence on BOTH tracks, never just
        // the louder track's voice. The second track verifies the chosen ID.
        if segments[index]["source"] == "mixed"
            && !probes("microphone", index).is_some_and(|v| matcher.supports_short(&identity, &v))
        {
            continue;
        }
        segments[index]["speakerId"] = json!(identity);
    }
}

#[cfg(test)]
mod tests {
    use super::super::refinement::Turn;
    use super::*;
    #[test]
    fn short_replies_only_and_padding_never_borrows_recognized_neighbor_audio() {
        let segment = json!({"speakerId":"unassigned","source":"mixed","start":10.,"end":10.8,"text":"A brief answer."});
        assert!(eligible(&segment).is_some());
        for (key, value) in [
            ("source", json!("microphone")),
            ("speakerId", json!("voice-known")),
            ("end", json!(10.2)),
            ("end", json!(12.)),
        ] {
            let mut s = segment.clone();
            s[key] = value;
            assert!(eligible(&s).is_none());
        }
        let mut analysis = Diarization {
            segments: vec![
                Turn {
                    start: 9.,
                    end: 9.9,
                    speaker_id: "voice-a".into(),
                },
                Turn {
                    start: 10.95,
                    end: 12.,
                    speaker_id: "voice-b".into(),
                },
            ],
            profiles: HashMap::new(),
            chunks: vec![],
        };
        assert_eq!(bounds(10., 10.8, 0.3, &analysis), Some((9.9, 10.95)));
        analysis.segments[0].end = 10.1;
        assert!(bounds(10., 10.8, 0.3, &analysis).is_none());
    }
}
