//! Explicit local evaluation of saved tracks. No library DB, devices or AI provider.
use serde_json::json;
use std::{fs::OpenOptions, io::Write, path::PathBuf, sync::Arc, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args=std::env::args().skip(1).collect::<Vec<_>>();
    if args.len()!=3 {return Err("Usage: tuck-audio-eval MODEL_DATA_DIR SAVED_AUDIO_DIR NEW_OUTPUT_JSON".into());}
    let models=tuck::adapters::models::ModelManager::new(&PathBuf::from(&args[0]),Arc::new(|_,_|{}));
    let root=PathBuf::from(&args[1]);
    let paths=["system","microphone","import"].into_iter().filter_map(|source|{
        let path=root.join(format!("{source}.wav"));path.is_file().then_some((source.into(),path))
    }).collect::<Vec<_>>();
    if paths.is_empty(){return Err("No saved WAV tracks found.".into());}
    let mut options=OpenOptions::new();options.write(true).create_new(true);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o600);}
    // Reserve a fresh output before inference; never overwrite an earlier trial.
    let mut output=options.open(&args[2])?;
    let started=Instant::now();
    let (duration,segments)=tuck::adapters::refinement::refine(&models,&paths,"evaluation",2).map_err(|e|e.message)?;
    let elapsed=started.elapsed().as_secs_f64();
    serde_json::to_writer(&mut output,&json!({"duration":duration,"elapsedSeconds":elapsed,"segments":segments}))?;
    output.write_all(b"\n")?;output.sync_all()?;
    println!("Refined {:.3} seconds of audio in {:.2} seconds; result saved locally.",duration,elapsed);
    Ok(())
}
