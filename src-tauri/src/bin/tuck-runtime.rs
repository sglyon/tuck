//! Diagnostic JSONL host for native adapter verification without a webview.
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn send(output: &Mutex<io::Stdout>, value: Value) {
    if let Ok(mut output) = output.lock() {
        if serde_json::to_writer(&mut *output, &value).is_ok() {
            let _ = writeln!(output);
            let _ = output.flush();
        }
    }
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let data = args
        .windows(2)
        .find(|v| v[0] == "--data-dir")
        .map(|v| PathBuf::from(&v[1]))
        .unwrap_or_else(|| {
            eprintln!("Usage: tuck-runtime --data-dir PATH");
            std::process::exit(2)
        });
    let resources = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_default();
    let output = Arc::new(Mutex::new(io::stdout()));
    let event_output = output.clone();
    let service = match tuck::runtime::compose(
        &data,
        &resources,
        Arc::new(move |event, payload| {
            send(&event_output, json!({"event":event,"payload":payload}))
        }),
    ) {
        Ok(v) => v,
        Err(e) => {
            send(&output, json!({"error":e}));
            std::process::exit(1)
        }
    };
    let mut input = io::stdin().lock();
    loop {
        let mut line = Vec::new();
        let count = match input.by_ref().take(1_048_578).read_until(b'\n', &mut line) {
            Ok(v) => v,
            Err(_) => break,
        };
        if count == 0 {
            break;
        }
        if line.len() > 1_048_576 {
            if !line.ends_with(b"\n") {
                let _ = input.skip_until(b'\n');
            }
            send(
                &output,
                json!({"id":null,"error":{"code":"request_too_large","message":"This command is too large."}}),
            );
            continue;
        }
        let request = serde_json::from_slice::<Value>(&line);
        let shutdown = request.as_ref().is_ok_and(|v| v["method"] == "shutdown");
        let result = match request {
            Ok(request)
                if request["id"].as_str().is_some_and(|v| v.len() <= 128)
                    && request["method"].as_str().is_some_and(|v| v.len() <= 80)
                    && request.get("params").is_none_or(Value::is_object) =>
            {
                let id = &request["id"];
                match service.handle(
                    request["method"].as_str().unwrap(),
                    request.get("params").cloned().unwrap_or_else(|| json!({})),
                ) {
                    Ok(result) => json!({"id":id,"result":result}),
                    Err(error) => json!({"id":id,"error":error}),
                }
            }
            _ => {
                json!({"id":null,"error":{"code":"invalid_request","message":"Invalid command or arguments."}})
            }
        };
        send(&output, result);
        if shutdown {
            break;
        }
    }
    let _ = service.handle("shutdown", json!({}));
}
