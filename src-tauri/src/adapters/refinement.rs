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
    sync::mpsc,
    time::Duration,
};
use tuck_core::{ports::Result, AppError};
fn error(message: &str) -> AppError {
    AppError::new("refinement_error", message)
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Turn {
    start: f64,
    end: f64,
    speaker_id: String,
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
fn diarize(models: &ModelManager, path: &Path, source: &str) -> Result<Vec<Turn>> {
    let mut child = Command::new(helper_path("tuck-speech", "TUCK_SPEECH_HELPER")?)
        .arg("--diarize")
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
            .take(16 * 1024 * 1024)
            .read_to_string(&mut text)
            .map(|_| text);
        let _ = tx.send(result);
    });
    let response = rx.recv_timeout(Duration::from_secs(300));
    if response.is_err() {
        let _ = child.kill();
    }
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
    let turns: Vec<Turn> = serde_json::from_value(result["segments"].clone())
        .map_err(|_| error("Speaker refinement returned an invalid timeline."))?;
    if turns
        .iter()
        .any(|t| !t.start.is_finite() || !t.end.is_finite() || t.start < 0. || t.end < t.start)
    {
        return Err(error("Speaker refinement returned invalid timestamps."));
    }
    Ok(turns)
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
        for (index, mut word) in utterance.into_iter().enumerate() {
            if suppress[index] {
                continue;
            }
            word.speaker_id = if word.speaker_id == "unassigned" {
                "unassigned".into()
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
pub fn refine(
    models: &ModelManager,
    paths: &[(String, PathBuf)],
    meeting: &str,
    threads: u32,
) -> Result<(f64, Vec<Value>)> {
    models.verify_offline_speakers()?;
    let mut runtime = SpeechRuntime::load(models, threads, false)?;
    let (mut system, mut microphone, mut other) = (Vec::new(), Vec::new(), Vec::new());
    let mut duration: f64 = 0.;
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
        let audio: Vec<f32> = reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32768.))
            .collect::<std::result::Result<_, _>>()
            .map_err(|_| error("Saved audio is incomplete."))?;
        duration = duration.max(audio.len() as f64 / SAMPLE_RATE as f64);
        let turns = diarize(models, path, source)?;
        let mut segmenter = Segmenter::new(&models.vad_path())?;
        let mut process = |chunks: Vec<super::speech::SpeechChunk>| -> Result<()> {
            for chunk in chunks {
                if let Some(segment) =
                    runtime.transcribe(&chunk.samples, chunk.start, source, meeting)?
                {
                    let ws = words(&segment, &turns)?;
                    match source.as_str() {
                        "system" => system.extend(ws),
                        "microphone" => microphone.push(ws),
                        _ => other.extend(ws),
                    }
                }
            }
            Ok(())
        };
        for (index, block) in audio.chunks(1600).enumerate() {
            process(segmenter.feed(block, index as f64 * 0.1))?;
        }
        process(segmenter.flush())?;
    }
    Ok((duration, segments(fuse(system, microphone, other), meeting)))
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
