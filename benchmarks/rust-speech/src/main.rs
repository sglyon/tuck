//! Same-model, same-PCM CPU baseline. No downloads or application side effects.
use serde_json::json;
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig};
use std::{env, fs, path::PathBuf, time::Instant};

fn decode(recognizer: &OfflineRecognizer, samples: &[f32]) -> String {
    let stream = recognizer.create_stream();
    stream.accept_waveform(16000, samples);
    recognizer.decode(&stream);
    stream.get_result().expect("recognition result").text
}

fn main() {
    let args: Vec<_> = env::args().collect();
    let models = PathBuf::from(args.get(1).expect("model directory"));
    let pcm = fs::read(args.get(2).expect("little-endian mono f32 16kHz PCM file")).unwrap();
    assert_eq!(pcm.len() % 4, 0);
    let samples: Vec<f32> = pcm
        .chunks_exact(4)
        .map(|x| f32::from_le_bytes(x.try_into().unwrap()))
        .collect();
    let threads: i32 = args.get(3).map(|x| x.parse().unwrap()).unwrap_or(2);
    let provider = args.get(4).cloned().unwrap_or_else(|| "cpu".into());
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer.encoder =
        Some(models.join("encoder.int8.onnx").to_str().unwrap().into());
    config.model_config.transducer.decoder =
        Some(models.join("decoder.int8.onnx").to_str().unwrap().into());
    config.model_config.transducer.joiner =
        Some(models.join("joiner.int8.onnx").to_str().unwrap().into());
    config.model_config.tokens = Some(models.join("tokens.txt").to_str().unwrap().into());
    config.model_config.model_type = Some("nemo_transducer".into());
    config.model_config.provider = Some(provider.clone());
    config.model_config.num_threads = threads;
    config.decoding_method = Some("greedy_search".into());
    let start = Instant::now();
    let recognizer = OfflineRecognizer::create(&config).expect("create recognizer");
    let load = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let text = decode(&recognizer, &samples);
    let first = start.elapsed().as_secs_f64();
    assert!(
        text.to_lowercase().contains("ask not"),
        "unexpected transcript: {text}"
    );
    let mut warm = vec![];
    for _ in 0..5 {
        let start = Instant::now();
        assert_eq!(decode(&recognizer, &samples), text);
        warm.push(start.elapsed().as_secs_f64());
    }
    let short_only = env::var_os("TUCK_BENCH_SHORT").is_some();
    let (chunked, joined_time, joined_text) = if short_only {
        (None, None, String::new())
    } else {
        let start = Instant::now();
        for _ in 0..8 {
            assert_eq!(decode(&recognizer, &samples), text);
        }
        let chunked = start.elapsed().as_secs_f64();
        let joined = samples.repeat(8);
        let start = Instant::now();
        let joined_text = decode(&recognizer, &joined);
        (
            Some(chunked),
            Some(start.elapsed().as_secs_f64()),
            joined_text,
        )
    };
    println!(
        "{}",
        json!({"language":"rust", "native_linkage": if cfg!(feature="native-static") {"static"} else {"shared"}, "short_only":short_only, "sherpa_onnx":"1.13.8", "provider_requested":provider, "threads":threads, "sample_count":samples.len(), "audio_seconds":samples.len() as f64/16000.0, "load_seconds":load, "first_decode_seconds":first, "cold_load_decode_seconds":load+first, "warm_decode_seconds":warm, "eight_chunks_seconds":chunked, "joined_seconds":joined_time, "text":text, "joined_text":joined_text})
    );
}
