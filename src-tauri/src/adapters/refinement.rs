//! Saved-audio refinement keeps ASR context, clusters full-track speech, and
//! merges echoing inputs. Original audio and transcript are never edited.
use super::{
    models::ModelManager,
    speech::{helper_path, Segmenter, SpeechRuntime, SAMPLE_RATE},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use tuck_core::{ports::Result, AppError};
fn error(message: &str) -> AppError {
    AppError::new("refinement_error", message)
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Turn {
    pub start: f64,
    pub end: f64,
    pub speaker_id: String,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VoiceChunk {
    #[serde(default)]
    pub speaker_id: String,
    pub index: usize,
    pub start: f64,
    pub end: f64,
    pub embedding: Vec<f32>,
}
#[derive(Deserialize)]
pub(crate) struct Diarization {
    pub segments: Vec<Turn>,
    #[serde(default)]
    pub profiles: HashMap<String, Vec<f32>>,
    #[serde(default)]
    pub chunks: Vec<VoiceChunk>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Word {
    text: String,
    start: f64,
    end: f64,
    speaker_id: String,
    source: String,
}
fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn speaker(turns: &[Turn], start: f64, end: f64) -> String {
    let center = (start + end) / 2.;
    turns
        .iter()
        .min_by(|a, b| {
            let distance = |t: &Turn| (t.start - center).max(0.).max(center - t.end);
            distance(a).total_cmp(&distance(b))
        })
        .filter(|t| (t.start - center).max(0.).max(center - t.end) <= 0.75)
        .map(|t| t.speaker_id.clone())
        .unwrap_or_else(|| "unassigned".into())
}
pub(crate) fn analyze(
    models: &ModelManager,
    path: &Path,
    source: &str,
    live: bool,
    cancel: Option<&AtomicBool>,
) -> Result<Diarization> {
    let mut child = Command::new(helper_path("tuck-speech", "TUCK_SPEECH_HELPER")?)
        .arg(if live { "--identify" } else { "--diarize" })
        .arg(models.diarization_dir())
        .arg(path)
        .arg(source)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| error("Speaker refinement could not start."))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("Speaker refinement has no output."))?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let result = stdout
            .take(64 * 1024 * 1024)
            .read_to_string(&mut text)
            .map(|_| text);
        let _ = tx.send(result);
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(300);
    let response = loop {
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) || std::time::Instant::now() > deadline
        {
            let _ = child.kill();
            break Err(mpsc::RecvTimeoutError::Timeout);
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            result => break result,
        }
    };
    let status = child
        .wait()
        .map_err(|_| error("Speaker refinement was interrupted."))?;
    let text = response
        .map_err(|_| error("Speaker refinement timed out. Original audio is preserved."))?
        .map_err(|_| error("Speaker refinement returned invalid output."))?;
    if !status.success() {
        return Err(error(
            "Speaker refinement failed. Original audio is preserved.",
        ));
    }
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("TUCK_DIARIZATION:"))
        .ok_or_else(|| error("Speaker refinement returned no timeline."))?;
    let result: Value = serde_json::from_str(payload)
        .map_err(|_| error("Speaker refinement returned an invalid timeline."))?;
    let diarization: Diarization = serde_json::from_value(result)
        .map_err(|_| error("Speaker refinement returned an invalid timeline."))?;
    if diarization
        .segments
        .iter()
        .any(|t| !t.start.is_finite() || !t.end.is_finite() || t.start < 0. || t.end < t.start)
    {
        return Err(error("Speaker refinement returned invalid timestamps."));
    }
    Ok(diarization)
}
fn words(segment: &Value, turns: &[Turn]) -> Result<Vec<Word>> {
    let start = segment["start"].as_f64().unwrap_or(0.);
    let raw=segment["words"].as_array().ok_or_else(||error("Meeting refinement needs Apple speech word timings. Install Parakeet for Apple silicon in Settings."))?;
    let mut output = Vec::new();
    for w in raw {
        let text = w["word"]
            .as_str()
            .ok_or_else(|| error("Invalid word timings."))?
            .to_owned();
        let a = start
            + w["startTime"]
                .as_f64()
                .ok_or_else(|| error("Invalid word timings."))?;
        let b = start
            + w["endTime"]
                .as_f64()
                .ok_or_else(|| error("Invalid word timings."))?;
        if !a.is_finite() || !b.is_finite() || b < a {
            return Err(error("Invalid word timings."));
        }
        output.push(Word {
            text,
            start: a,
            end: b,
            speaker_id: speaker(turns, a, b),
            source: segment["source"].as_str().unwrap_or("import").into(),
        });
    }
    if normalize(
        &output
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    ) != normalize(segment["text"].as_str().unwrap_or(""))
    {
        return Err(error(
            "Word timings did not preserve the recognized text. Original audio is preserved.",
        ));
    }
    Ok(output)
}
/// Ordered word matches, restricted to simultaneous audio. The time constraint
/// prevents repeated phrases at different points in the meeting from matching.
fn alignment(a: &[Word], b: &[Word]) -> Vec<(usize, usize)> {
    let n = b.len() + 1;
    let mut table = vec![0u16; (a.len() + 1) * n];
    for i in 0..a.len() {
        for j in 0..b.len() {
            table[(i + 1) * n + j + 1] = if (a[i].start - b[j].start).abs() < 0.8
                && normalize(&a[i].text) == normalize(&b[j].text)
            {
                table[i * n + j] + 1
            } else {
                table[i * n + j + 1].max(table[(i + 1) * n + j])
            };
        }
    }
    let (mut i, mut j) = (a.len(), b.len());
    let mut result = Vec::new();
    while i > 0 && j > 0 {
        if (a[i - 1].start - b[j - 1].start).abs() < 0.8
            && normalize(&a[i - 1].text) == normalize(&b[j - 1].text)
        {
            result.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if table[(i - 1) * n + j] >= table[i * n + j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    result.reverse();
    result
}
// Accept a small spelling discrepancy only inside a strongly aligned echo.
// Protect negations, common speaker/ownership pronouns, and numeric contrasts.
fn echo_variant(a: &str, b: &str) -> bool {
    let a = normalize(a);
    let b = normalize(b);
    if a == b {
        return !a.is_empty();
    }
    let protected = [
        "no", "not", "never", "cannot", "cant", "wont", "dont", "didnt", "isnt", "arent",
        "shouldnt", "wouldnt", "couldnt", "mustnt", "wasnt", "werent", "we", "you", "he", "she",
        "they", "my", "our", "your", "his", "her", "their",
    ];
    if protected.contains(&a.as_str())
        || protected.contains(&b.as_str())
        || !a.chars().all(char::is_alphabetic)
        || !b.chars().all(char::is_alphabetic)
    {
        return false;
    }
    let (a, b) = (a.chars().collect::<Vec<_>>(), b.chars().collect::<Vec<_>>());
    if a.len().max(b.len()) < 3 || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    if a.len() == b.len() {
        return a.iter().zip(&b).filter(|(a, b)| a != b).count() == 1;
    }
    let (short, long) = if a.len() < b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    (0..long.len()).any(|skip| {
        short.iter().eq(long
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (i != skip).then_some(c)))
    })
}
fn suppress_unidentified_echo(
    utterance: &[Word],
    nearby: &[Word],
    matches: &[(usize, usize)],
    suppress: &mut [bool],
) {
    let mut run: Vec<(usize, usize)> = vec![];
    let apply = |run: &[(usize, usize)], suppress: &mut [bool]| {
        if run.len() < 3
            || run
                .iter()
                .filter(|&&(a, _)| normalize(&utterance[a].text).len() >= 4)
                .count()
                < 2
        {
            return;
        }
        let offsets = run
            .iter()
            .map(|&(a, b)| utterance[a].start - nearby[b].start)
            .collect::<Vec<_>>();
        let low = offsets.iter().copied().fold(f64::INFINITY, f64::min);
        let high = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if high - low > 0.2 {
            return;
        }
        for &(a, b) in run {
            suppress[a] = true;
            // Only adjacent, time-aligned small spelling variants can extend a run.
            // Unmatched interjections and unique boundaries stay in the transcript.
            for direction in [-1isize, 1] {
                for step in 1..=2 {
                    let Some(i) = a.checked_add_signed(direction * step) else {
                        break;
                    };
                    let Some(j) = b.checked_add_signed(direction * step) else {
                        break;
                    };
                    let (Some(mic), Some(sys)) = (utterance.get(i), nearby.get(j)) else {
                        break;
                    };
                    let offset = mic.start - sys.start;
                    if mic.speaker_id != "unassigned"
                        || sys.speaker_id != nearby[b].speaker_id
                        || offset.abs() >= 0.25
                        || offset < low - 0.1
                        || offset > high + 0.1
                        || !echo_variant(&mic.text, &sys.text)
                    {
                        break;
                    }
                    suppress[i] = true;
                }
            }
        }
    };
    for &(a, b) in matches {
        let eligible = utterance[a].speaker_id == "unassigned"
            && (utterance[a].start - nearby[b].start).abs() < 0.25;
        let contiguous = run.last().is_none_or(|&(x, y)| {
            a - x <= 2
                && b - y <= 2
                && nearby[b].speaker_id == nearby[y].speaker_id
                && utterance[a].start - utterance[x].end < 1.0
        });
        if !eligible || !contiguous {
            apply(&run, suppress);
            run.clear();
        }
        if eligible {
            run.push((a, b));
        }
    }
    apply(&run, suppress);
}
fn fuse(system: Vec<Word>, microphone: Vec<Vec<Word>>, other: Vec<Word>) -> Vec<Word> {
    let mut votes: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut decisions = Vec::new();
    for utterance in microphone {
        if utterance.is_empty() {
            continue;
        }
        let nearby: Vec<_> = system
            .iter()
            .filter(|w| {
                w.start >= utterance[0].start - 0.8
                    && w.start <= utterance.last().unwrap().end + 0.8
            })
            .cloned()
            .collect();
        let matches = alignment(&utterance, &nearby);
        if matches.len() >= 3 {
            for &(a, b) in &matches {
                if utterance[a].speaker_id != "unassigned" && nearby[b].speaker_id != "unassigned" {
                    *votes
                        .entry(utterance[a].speaker_id.clone())
                        .or_default()
                        .entry(nearby[b].speaker_id.clone())
                        .or_default() += 1;
                }
            }
        }
        // Require contextual evidence for longer utterances. A short phrase is
        // suppressed only when every word matches tightly in time.
        let duplicate = if utterance.len() >= 4 {
            matches.len() * 4 >= utterance.len() * 3
        } else {
            matches.len() == utterance.len()
                && matches
                    .iter()
                    .all(|&(a, b)| (utterance[a].start - nearby[b].start).abs() < 0.25)
        };
        decisions.push((utterance, nearby, matches, duplicate));
    }

    let mapping: HashMap<_, _> = votes
        .into_iter()
        .filter_map(|(mic, v)| {
            let total: usize = v.values().sum();
            let (sys, count) = v.into_iter().max_by_key(|(_, n)| *n)?;
            (count >= 12 && count * 5 >= total * 4).then_some((mic, sys))
        })
        .collect();
    let mut output = system;
    output.extend(other);
    for (utterance, nearby, matches, duplicate) in decisions {
        let mut suppress = vec![false; utterance.len()];
        if duplicate {
            if let Some((first, last)) = matches.first().zip(matches.last()) {
                for index in first.0..=last.0 {
                    let before = matches.iter().rev().find(|&&(a, _)| a <= index).unwrap().0;
                    let after = matches.iter().find(|&&(a, _)| a >= index).unwrap().0;
                    // A different voice can interject inside an otherwise
                    // echoed phrase. Only bridge ASR variants of the same voice.
                    suppress[index] = utterance[index].speaker_id == utterance[before].speaker_id
                        && utterance[index].speaker_id == utterance[after].speaker_id;
                }
            }
        } else {
            // A single ASR substitution can push an entire utterance below the
            // coverage threshold. Detect locally aligned phrase runs as well.
            // Require established cross-track speaker identity before treating
            // intervening ASR variants as another rendering of the same voice.
            let mut run = Vec::new();
            let suppress_run = |run: &[(usize, usize)], suppress: &mut [bool]| {
                if run.len() < 3 {
                    return;
                }
                let first = run.first().unwrap().0;
                let last = run.last().unwrap().0;
                for index in first..=last {
                    if mapping
                        .get(&utterance[index].speaker_id)
                        .is_some_and(|mapped| {
                            run.iter().any(|&(_, b)| &nearby[b].speaker_id == mapped)
                        })
                    {
                        suppress[index] = true;
                    }
                }
            };
            for &(a, b) in &matches {
                let consistent =
                    mapping.get(&utterance[a].speaker_id) == Some(&nearby[b].speaker_id);
                let contiguous = run
                    .last()
                    .is_none_or(|&(prior_a, prior_b): &(usize, usize)| {
                        a - prior_a <= 3
                            && b - prior_b <= 3
                            && utterance[a].start - utterance[prior_a].end < 1.0
                            && utterance[a].speaker_id == utterance[prior_a].speaker_id
                    });
                if !consistent || !contiguous {
                    suppress_run(&run, &mut suppress);
                    run.clear();
                }
                if consistent {
                    run.push((a, b));
                }
            }
            suppress_run(&run, &mut suppress);
        }
        // Echo detection does not require inventing an identity for an unknown mic voice.
        suppress_unidentified_echo(&utterance, &nearby, &matches, &mut suppress);
        for (index, mut word) in utterance.into_iter().enumerate() {
            if suppress[index] {
                continue;
            }
            word.speaker_id =
                if word.speaker_id == "unassigned" || word.speaker_id.starts_with("voice-") {
                    word.speaker_id.clone()
                } else {
                    mapping
                        .get(&word.speaker_id)
                        .cloned()
                        .unwrap_or_else(|| format!("microphone-{}", word.speaker_id))
                };
            output.push(word);
        }
    }
    output.sort_by(|a, b| a.start.total_cmp(&b.start));
    output
}
fn segments(words: Vec<Word>, meeting: &str) -> Vec<Value> {
    let mut output: Vec<Value> = Vec::new();
    for w in words {
        if let Some(last) = output.last_mut() {
            if last["speakerId"] == w.speaker_id
                && w.start - last["end"].as_f64().unwrap_or(0.) < 1.
                && w.end - last["start"].as_f64().unwrap_or(0.) < 30.
            {
                if last["source"] != w.source {
                    last["source"] = json!("mixed");
                }
                last["text"] = json!(format!(
                    "{} {}",
                    last["text"].as_str().unwrap_or(""),
                    w.text
                ));
                last["end"] = json!(w.end.max(last["end"].as_f64().unwrap_or(0.)));
                continue;
            }
        }
        output.push(json!({"id":uuid::Uuid::new_v4().to_string(),"meetingId":meeting,"start":w.start,"end":w.end,"text":w.text,"speakerId":w.speaker_id,"source":w.source}));
    }
    output
}
/// Use only strong stored-profile evidence for the delayed live view. New or
/// ambiguous voices stay unidentified; live batches never enroll new profiles.
pub(crate) fn live_projection(
    raw: &[Value],
    analyses: &HashMap<String, Diarization>,
    meeting: &str,
    memory: &super::voice_memory::VoiceMemory,
    identify_before: f64,
    window_start: f64,
    previous: Option<&Value>,
) -> Result<Value> {
    let matcher = memory.matcher()?;
    let mut chunks: HashMap<String, Vec<(&VoiceChunk, Option<String>)>> = HashMap::new();
    for (source, analysis) in analyses {
        let mut identities = HashMap::new();
        for (id, embedding) in &analysis.profiles {
            let evidence: f64 = analysis
                .segments
                .iter()
                .filter(|t| &t.speaker_id == id)
                .map(|t| t.end - t.start)
                .sum();
            if evidence >= 15. {
                if let Some(identity) = matcher.identify(embedding) {
                    identities.insert(id, identity.0);
                }
            }
        }
        chunks.insert(
            source.clone(),
            analysis
                .chunks
                .iter()
                .map(|c| {
                    let identity = identities
                        .get(&c.speaker_id)
                        .filter(|id| matcher.supports(id, &c.embedding))
                        .cloned();
                    (c, identity)
                })
                .collect(),
        );
    }
    let (mut system, mut microphone, mut other) = (vec![], vec![], vec![]);
    let mut retained = serde_json::Map::new();
    for segment in raw {
        let source = segment["source"].as_str().unwrap_or("import");
        let empty = Vec::new();
        let timeline = analyses.get(source).map(|a| &a.segments).unwrap_or(&empty);
        let mut ws = words(segment, timeline)?;
        for word in &mut ws {
            word.speaker_id = "unassigned".into();
            let key = format!(
                "{}:{:.6}:{:.6}:{}",
                word.source, word.start, word.end, word.text
            );
            if word.end < window_start {
                if let Some(id) = previous.and_then(|p| p["_voiceWords"][&key].as_str()) {
                    word.speaker_id = id.into();
                    retained.insert(key, json!(id));
                }
                continue;
            }
            if word.end > identify_before {
                continue;
            }
            let center = (word.start + word.end) / 2.;
            let mut votes: HashMap<&str, usize> = HashMap::new();
            let mut indices = std::collections::HashSet::new();
            let mut total = 0;
            for (chunk, identity) in chunks.get(source).into_iter().flatten() {
                if chunk.start <= center && chunk.end >= center {
                    total += 1;
                    if let Some(id) = identity {
                        *votes.entry(id).or_default() += 1;
                        indices.insert((id, chunk.index));
                    }
                }
            }
            if let Some((id, count)) = votes.into_iter().max_by_key(|(_, n)| *n) {
                let independent = indices.iter().filter(|(voice, _)| *voice == id).count();
                if independent >= 2 && count * 10 >= total * 9 {
                    word.speaker_id = id.into();
                    retained.insert(key, json!(id));
                }
            }
        }
        match source {
            "system" => system.extend(ws),
            "microphone" => microphone.push(ws),
            _ => other.extend(ws),
        }
    }
    let output = segments(fuse(system, microphone, other), meeting);
    let used: std::collections::HashSet<_> = output
        .iter()
        .filter_map(|s| s["speakerId"].as_str())
        .collect();
    let speakers = matcher
        .names()
        .into_iter()
        .filter(|s| used.contains(s["id"].as_str().unwrap_or("")))
        .collect::<Vec<_>>();
    Ok(
        json!({"kind":"speaker_projection","_voiceWords":retained,"segments":output,"rawSegmentIds":raw.iter().map(|s|s["id"].clone()).collect::<Vec<_>>(),"speakers":speakers}),
    )
}
pub fn refine(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    meeting: &str,
    threads: u32,
) -> Result<(f64, Vec<Value>)> {
    refine_with_memory(models, paths, meeting, threads, None)
}
pub fn transcribe_saved(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    meeting: &str,
    threads: u32,
) -> Result<(f64, Vec<Value>)> {
    let mut runtime = SpeechRuntime::load(models, threads, true)?;
    let mut duration: f64 = 0.;
    let mut output = vec![];
    for (source, path) in paths {
        let mut reader =
            hound::WavReader::open(path).map_err(|_| error("Saved audio could not be read."))?;
        let spec = reader.spec();
        if spec.channels != 1
            || spec.sample_rate != SAMPLE_RATE
            || spec.bits_per_sample != 16
            || spec.sample_format != hound::SampleFormat::Int
            || reader.duration() > SAMPLE_RATE * 7200
        {
            return Err(error(
                "Refinement requires mono 16 kHz PCM saved audio, up to two hours per track.",
            ));
        }
        let audio = reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| error("Saved audio is incomplete."))?;
        duration = duration.max(audio.len() as f64 / SAMPLE_RATE as f64);
        let mut segmenter = Segmenter::new(&models.vad_path())?;
        let mut consume = |chunks: Vec<super::speech::SpeechChunk>| -> Result<()> {
            for chunk in chunks {
                if let Some(s) = runtime.transcribe(&chunk.samples, chunk.start, source, meeting)? {
                    output.push(s);
                }
            }
            Ok(())
        };
        for (index, block) in audio.chunks(1600).enumerate() {
            consume(segmenter.feed(block, index as f64 * 0.1))?;
        }
        consume(segmenter.flush())?;
    }
    Ok((duration, output))
}
pub fn refine_with_memory(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    meeting: &str,
    threads: u32,
    memory: Option<&super::voice_memory::VoiceMemory>,
) -> Result<(f64, Vec<Value>)> {
    models.verify_offline_speakers()?;
    let (duration, raw) = transcribe_saved(models, paths, meeting, threads)?;
    refine_transcript(models, paths, meeting, memory, duration, &raw)
}
pub(crate) fn refine_transcript(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    meeting: &str,
    memory: Option<&super::voice_memory::VoiceMemory>,
    duration: f64,
    raw: &[Value],
) -> Result<(f64, Vec<Value>)> {
    let mut analyses = HashMap::new();
    for (source, path) in paths {
        let mut analysis = analyze(models, path, source, false, None)?;
        if let Some(memory) = memory {
            let mut mapping = HashMap::new();
            for (id, embedding) in &analysis.profiles {
                let seconds = analysis
                    .segments
                    .iter()
                    .filter(|t| &t.speaker_id == id)
                    .map(|t| t.end - t.start)
                    .sum();
                mapping.insert(
                    id.clone(),
                    memory
                        .enroll_or_match(embedding, seconds)?
                        .map(|p| p.0)
                        .unwrap_or_else(|| "unassigned".into()),
                );
            }
            for turn in &mut analysis.segments {
                turn.speaker_id = mapping
                    .get(&turn.speaker_id)
                    .cloned()
                    .unwrap_or_else(|| "unassigned".into());
            }
        }
        analyses.insert(source.clone(), analysis);
    }
    let (mut system, mut microphone, mut other) = (vec![], vec![], vec![]);
    for segment in raw {
        let source = segment["source"].as_str().unwrap_or("import");
        let analysis = analyses
            .get(source)
            .ok_or_else(|| error("A transcript source has no retained audio."))?;
        let ws = words(segment, &analysis.segments)?;
        match source {
            "system" => system.extend(ws),
            "microphone" => microphone.push(ws),
            _ => other.extend(ws),
        }
    }
    let mut output = segments(fuse(system, microphone, other), meeting);
    if let Some(memory) = memory {
        super::short_speech::recover(models, paths, &analyses, memory, &mut output);
        let names = memory.names()?;
        for segment in &mut output {
            if let Some(n) = names.iter().find(|n| n["id"] == segment["speakerId"]) {
                segment["speakerName"] = n["name"].clone();
                segment["speakerColor"] = n["color"].clone();
            }
        }
    }
    Ok((duration, output))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn word(text: &str, time: f64, source: &str, speaker: &str) -> Word {
        Word {
            text: text.into(),
            start: time,
            end: time + 0.1,
            source: source.into(),
            speaker_id: speaker.into(),
        }
    }
    #[test]
    #[ignore = "explicit private saved-transcript refinement replay with local models"]
    fn evaluate_saved_transcript_refinement() {
        use std::{fs, io::Write, sync::Arc};
        let env_path = |name| PathBuf::from(std::env::var_os(name).expect(name));
        let case = env_path("TUCK_DELAYED_CASE");
        let data = env_path("TUCK_DELAYED_MODELS");
        let saved: Value =
            serde_json::from_slice(&fs::read(env_path("TUCK_DELAYED_RAW")).unwrap()).unwrap();
        let stage = tempfile::tempdir_in(&case).unwrap();
        if let Some(path) = std::env::var_os("TUCK_REFINEMENT_VOICES") {
            fs::copy(path, stage.path().join("voice-memory.json")).unwrap();
        }
        let memory = super::super::voice_memory::VoiceMemory::new(stage.path());
        let models = ModelManager::new(&data, Arc::new(|_, _| {}));
        models.verify_offline_speakers().unwrap();
        let paths = ["system", "microphone"]
            .into_iter()
            .map(|source| (source.to_owned(), case.join(format!("{source}.wav"))))
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let (duration, segments) = refine_transcript(
            &models,
            &paths,
            "evaluation",
            Some(&memory),
            saved["duration"].as_f64().unwrap(),
            saved["rawSegments"].as_array().unwrap(),
        )
        .unwrap();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(env_path("TUCK_DELAYED_OUTPUT")).unwrap();
        serde_json::to_writer(&mut output, &json!({"duration":duration,"elapsedSeconds":started.elapsed().as_secs_f64(),"segments":segments})).unwrap();
        output.write_all(b"\n").unwrap();
        output.sync_all().unwrap();
    }
    #[test]
    #[ignore = "explicit local saved-audio replay; requires installed Apple models and private case paths"]
    fn evaluate_delayed_saved_recordings() {
        use std::sync::Arc;
        let case = PathBuf::from(std::env::var_os("TUCK_DELAYED_CASE").expect("TUCK_DELAYED_CASE"));
        let prior =
            PathBuf::from(std::env::var_os("TUCK_DELAYED_PRIOR").expect("TUCK_DELAYED_PRIOR"));
        let data =
            PathBuf::from(std::env::var_os("TUCK_DELAYED_MODELS").expect("TUCK_DELAYED_MODELS"));
        let output =
            PathBuf::from(std::env::var_os("TUCK_DELAYED_OUTPUT").expect("TUCK_DELAYED_OUTPUT"));
        let models = ModelManager::new(&data, Arc::new(|_, _| {}));
        models.verify_offline_speakers().unwrap();
        let memory_root = tempfile::Builder::new()
            .prefix("voice-trial-")
            .tempdir_in(&case)
            .unwrap();
        let memory = super::super::voice_memory::VoiceMemory::new(memory_root.path());
        let prior_analysis =
            analyze(&models, &prior.join("system.wav"), "system", false, None).unwrap();
        let mut prior_names = Vec::new();
        for (id, embedding) in &prior_analysis.profiles {
            let seconds = prior_analysis
                .segments
                .iter()
                .filter(|t| &t.speaker_id == id)
                .map(|t| t.end - t.start)
                .sum();
            if let Some(profile) = memory.enroll_or_match(embedding, seconds).unwrap() {
                prior_names.push(profile.0);
            }
        }
        let paths = ["system", "microphone"]
            .into_iter()
            .filter_map(|source| {
                let path = case.join(format!("{source}.wav"));
                path.exists().then_some((source.to_owned(), path))
            })
            .collect::<Vec<_>>();
        let started = std::time::Instant::now();
        let (duration, raw) = if let Some(path) = std::env::var_os("TUCK_DELAYED_RAW") {
            let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
            (
                saved["duration"].as_f64().unwrap(),
                saved["rawSegments"].as_array().unwrap().clone(),
            )
        } else {
            transcribe_saved(&models, &paths, "delayed-evaluation", 2).unwrap()
        };
        assert!(raw.iter().all(|s| s["speakerId"] == "unassigned"));
        let mut checkpoints = Vec::new();
        let mut previous: Option<Value> = None;
        let mut times = (90..duration as usize)
            .step_by(30)
            .map(|n| n as f64)
            .collect::<Vec<_>>();
        times.push(duration);
        for through in times {
            let window = (through - 300.).max(0.);
            let stage = tempfile::tempdir_in(&case).unwrap();
            let mut analyses = HashMap::new();
            for (source, path) in &paths {
                let target = stage.path().join(format!("{source}.wav"));
                if let Some(offset) =
                    super::super::audio::copy_window(path, &target, window, through).unwrap()
                {
                    let mut analysis = analyze(&models, &target, source, true, None).unwrap();
                    for t in &mut analysis.segments {
                        t.start += offset;
                        t.end += offset;
                    }
                    for c in &mut analysis.chunks {
                        c.start += offset;
                        c.end += offset;
                    }
                    analyses.insert(source.clone(), analysis);
                }
            }
            let available = raw
                .iter()
                .filter(|s| s["end"].as_f64().unwrap() <= through)
                .cloned()
                .collect::<Vec<_>>();
            let projection = live_projection(
                &available,
                &analyses,
                "delayed-evaluation",
                &memory,
                through - 90.,
                window,
                previous.as_ref(),
            )
            .unwrap();
            let identified = projection["segments"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["speakerId"] != "unassigned")
                .count();
            checkpoints.push(json!({"through":through,"identifyBefore":through-90.,"segments":projection["segments"],"speakers":projection["speakers"]}));
            previous = Some(projection);
            println!(
                "Delayed checkpoint {:.0}s: {} identified turns",
                through, identified
            );
        }
        assert_eq!(
            memory.names().unwrap().len(),
            prior_names.len(),
            "live processing must not enroll new voices"
        );
        let (_, final_segments) = refine_transcript(
            &models,
            &paths,
            "delayed-evaluation",
            Some(&memory),
            duration,
            &raw,
        )
        .unwrap();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(output).unwrap();
        serde_json::to_writer(file,&json!({"duration":duration,"elapsedSeconds":started.elapsed().as_secs_f64(),"priorIdentities":prior_names,"rawSegments":raw,"checkpoints":checkpoints,"segments":final_segments,"storedProfiles":memory.names().unwrap()})).unwrap();
    }
    #[test]
    fn live_identification_waits_and_abstains_on_unknown_conflicting_or_single_window_evidence() {
        let root = tempfile::tempdir().unwrap();
        let memory = super::super::voice_memory::VoiceMemory::new(root.path());
        let mut embedding = vec![0.; 256];
        embedding[0] = 1.;
        let known = memory.enroll_or_match(&embedding, 20.).unwrap().unwrap();
        let raw = json!({"id":"raw","meetingId":"meeting","source":"system","start":0.,"end":1.,"text":"Hello.","speakerId":"unassigned","words":[{"word":"Hello.","startTime":0.1,"endTime":0.8}]});
        let chunk = |index, embedding: Vec<f32>| VoiceChunk {
            speaker_id: "S1".into(),
            index,
            start: 0.,
            end: 2.,
            embedding,
        };
        let mut analyses = HashMap::new();
        analyses.insert(
            "system".into(),
            Diarization {
                segments: vec![Turn {
                    start: 0.,
                    end: 20.,
                    speaker_id: "S1".into(),
                }],
                profiles: HashMap::from([("S1".into(), embedding.clone())]),
                chunks: vec![chunk(0, embedding.clone()), chunk(1, embedding.clone())],
            },
        );
        let project = |analyses: &HashMap<String, Diarization>, before| {
            live_projection(
                &[raw.clone()],
                analyses,
                "meeting",
                &memory,
                before,
                0.,
                None,
            )
            .unwrap()
        };
        assert_eq!(
            project(&analyses, 0.)["segments"][0]["speakerId"],
            "unassigned"
        );
        assert_eq!(project(&analyses, 2.)["segments"][0]["speakerId"], known.0);
        let previous = project(&analyses, 2.);
        let after_window = live_projection(
            &[raw.clone()],
            &HashMap::new(),
            "meeting",
            &memory,
            400.,
            300.,
            Some(&previous),
        )
        .unwrap();
        assert_eq!(
            after_window["segments"][0]["speakerId"], known.0,
            "rolling context must retain earlier verified labels"
        );
        analyses.get_mut("system").unwrap().chunks.pop();
        assert_eq!(
            project(&analyses, 2.)["segments"][0]["speakerId"],
            "unassigned"
        );
        let mut stranger = vec![0.; 256];
        stranger[1] = 1.;
        analyses
            .get_mut("system")
            .unwrap()
            .chunks
            .push(chunk(1, stranger.clone()));
        assert_eq!(
            project(&analyses, 2.)["segments"][0]["speakerId"],
            "unassigned"
        );
        analyses.get_mut("system").unwrap().chunks =
            vec![chunk(0, stranger.clone()), chunk(1, stranger)];
        assert_eq!(
            project(&analyses, 2.)["segments"][0]["speakerId"],
            "unassigned"
        );
        assert_eq!(
            memory.names().unwrap().len(),
            1,
            "live analysis must not enroll unknown voices"
        );
    }
    #[test]
    fn echo_removed_unique_interjection_preserved_and_repeated_phrase_kept() {
        let phrase = ["we", "can", "ship", "this"];
        let system: Vec<_> = phrase
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "S1"))
            .collect();
        let echo = system
            .iter()
            .cloned()
            .map(|mut w| {
                w.source = "microphone".into();
                w.start += 0.1;
                w
            })
            .collect();
        let unique = vec![word("Actually", 0.5, "microphone", "S2")];
        let later: Vec<_> = phrase
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, 10. + i as f64 * 0.2, "microphone", "S2"))
            .collect();
        let output = fuse(system, vec![echo, unique, later], vec![]);
        assert_eq!(output.len(), 9);
        assert!(output.iter().any(|w| w.text == "Actually"));
        assert_eq!(output.iter().filter(|w| w.text == "ship").count(), 2);
    }
    #[test]
    fn unique_tail_of_echoing_phrase_is_preserved() {
        let phrase = ["we", "can", "ship", "this"];
        let system: Vec<_> = phrase
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "S1"))
            .collect();
        let mut echo = system
            .iter()
            .cloned()
            .map(|mut w| {
                w.source = "microphone".into();
                w
            })
            .collect::<Vec<_>>();
        echo.push(word("tomorrow", 0.9, "microphone", "S1"));
        let output = fuse(system, vec![echo], vec![]);
        assert_eq!(output.len(), 5);
        assert_eq!(output.last().unwrap().text, "tomorrow");
    }
    #[test]
    fn partial_echo_with_asr_variants_does_not_interleave_duplicate_words() {
        let context = [
            "the",
            "first",
            "part",
            "establishes",
            "the",
            "shared",
            "voice",
            "on",
            "these",
            "two",
            "recording",
            "tracks",
        ];
        let system: Vec<_> = context
            .iter()
            .chain(
                [
                    "it", "gets", "deployed", "to", "the", "server", "very", "soon",
                ]
                .iter(),
            )
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "S1"))
            .collect();
        let echo = context
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "microphone", "M1"))
            .collect();
        let variant = [
            "it", "gets", "deployed", "t", "the", "sever", "very", "soon", "our", "local", "copy",
            "works",
        ]
        .iter()
        .enumerate()
        .map(|(i, t)| word(t, 2.4 + i as f64 * 0.2, "microphone", "M1"))
        .collect();
        let output = fuse(system, vec![echo, variant], vec![]);
        for text in ["it", "gets", "deployed"] {
            assert_eq!(output.iter().filter(|w| w.text == text).count(), 1);
        }
        assert!(!output.iter().any(|w| w.text == "sever"));
        assert!(
            output.iter().any(|w| w.text == "copy"),
            "unique trailing speech must survive"
        );
    }
    #[test]
    fn unidentified_partial_echo_with_spelling_variants_stays_one_turn() {
        let system = ["built", "an", "efficient", "cloud", "service"]
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "voice-known"))
            .collect();
        let mic = ["build", "and", "efficient", "cloud", "service"]
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2 + 0.064, "microphone", "unassigned"))
            .collect();
        let output = segments(fuse(system, vec![mic], vec![]), "test");
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["text"], "built an efficient cloud service");
        assert_eq!(output[0]["speakerId"], "voice-known");
    }
    #[test]
    fn unidentified_echo_does_not_erase_unique_interjections_or_negation() {
        let system = ["we", "can", "build", "efficient", "cloud", "services"]
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "voice-known"))
            .collect();
        let mut mic = ["we", "cant", "build", "efficient", "cloud", "services"]
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2 + 0.064, "microphone", "unassigned"))
            .collect::<Vec<_>>();
        mic.insert(4, word("Wait!", 0.75, "microphone", "unassigned"));
        mic.push(word("tomorrow", 1.4, "microphone", "unassigned"));
        let output = fuse(system, vec![mic], vec![]);
        for text in ["cant", "Wait!", "tomorrow"] {
            assert!(output.iter().any(|w| w.text == text));
        }
        assert!(output
            .iter()
            .filter(|w| w.source == "microphone")
            .all(|w| w.speaker_id == "unassigned"));
        assert!(!echo_variant("25", "26"));
    }
    #[test]
    fn different_voice_inside_echoing_utterance_is_preserved() {
        let phrase = ["we", "can", "ship", "this"];
        let system: Vec<_> = phrase
            .iter()
            .enumerate()
            .map(|(i, t)| word(t, i as f64 * 0.2, "system", "A"))
            .collect();
        let mut echo = system
            .iter()
            .cloned()
            .map(|mut w| {
                w.source = "microphone".into();
                w
            })
            .collect::<Vec<_>>();
        echo.insert(2, word("Wait!", 0.3, "microphone", "B"));
        let output = fuse(system, vec![echo], vec![]);
        assert_eq!(output.len(), 5);
        assert!(output.iter().any(|w| w.text == "Wait!"));
    }
    #[test]
    fn continuous_speaker_turn_groups_across_sources_with_honest_provenance() {
        let output = segments(
            vec![
                word("One", 0., "system", "A"),
                word("continuous", 0.2, "microphone", "A"),
                word("sentence.", 0.4, "system", "A"),
            ],
            "meeting",
        );
        assert_eq!(output.len(), 1);
        assert_eq!(output[0]["text"], "One continuous sentence.");
        assert_eq!(output[0]["source"], "mixed");
        tuck_core::domain::TranscriptSegment::from_dto(output[0].clone()).unwrap();
    }
    #[test]
    fn word_boundary_changes_speaker_without_retranscribing() {
        let turns = vec![
            Turn {
                start: 0.,
                end: 1.,
                speaker_id: "A".into(),
            },
            Turn {
                start: 1.,
                end: 2.,
                speaker_id: "B".into(),
            },
        ];
        let ws=words(&json!({"start":0.,"source":"system","text":"Hello there.","words":[{"word":"Hello","startTime":0.1,"endTime":0.5},{"word":"there.","startTime":1.2,"endTime":1.5}]}),&turns).unwrap();
        let out = segments(ws, "meeting");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["speakerId"], "A");
        assert_eq!(out[1]["speakerId"], "B");
    }
}
