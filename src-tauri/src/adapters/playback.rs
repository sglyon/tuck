//! A scoped media protocol. Every response is at most one MiB; no entire recording
//! is placed in a webview IPC message or held in a JavaScript buffer.
use super::{archive::LocalArchive, audio};
use tauri::http::{Request, Response, StatusCode};
use tuck_core::{ports::Result, AppError};
pub fn source_url(meeting: &str, id: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("http://tuck-audio.localhost/{meeting}/{id}")
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("tuck-audio://localhost/{meeting}/{id}")
    }
}
fn invalid() -> AppError {
    AppError::invalid("Invalid audio range.")
}
pub fn byte_range(header: Option<&str>, length: u64) -> Result<(u64, u64)> {
    if length == 0 {
        return Err(invalid());
    }
    let Some(header) = header else {
        return Ok((0, (length - 1).min(audio::MAX_CHUNK - 1)));
    };
    if header.len() > 128 || header.contains(',') {
        return Err(invalid());
    }
    let range = header.strip_prefix("bytes=").ok_or_else(invalid)?;
    let (start, end) = range.split_once('-').ok_or_else(invalid)?;
    if start.is_empty() {
        if !end.bytes().all(|c| c.is_ascii_digit()) {
            return Err(invalid());
        }
        let suffix = end.parse::<u64>().map_err(|_| invalid())?;
        if suffix == 0 {
            return Err(invalid());
        }
        let start = length.saturating_sub(suffix);
        return Ok((
            start,
            (length - 1).min(start.saturating_add(audio::MAX_CHUNK - 1)),
        ));
    }
    if !start.bytes().all(|c| c.is_ascii_digit())
        || (!end.is_empty() && !end.bytes().all(|c| c.is_ascii_digit()))
    {
        return Err(invalid());
    }
    let start = start.parse::<u64>().map_err(|_| invalid())?;
    let end = if end.is_empty() {
        length - 1
    } else {
        end.parse::<u64>().map_err(|_| invalid())?.min(length - 1)
    };
    if start >= length || end < start {
        return Err(invalid());
    }
    Ok((start, end.min(start.saturating_add(audio::MAX_CHUNK - 1))))
}
fn allowed_origin(origin: &str) -> bool {
    matches!(
        origin,
        "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost"
    ) || (cfg!(debug_assertions)
        && matches!(origin, "http://127.0.0.1:1420" | "http://localhost:1420"))
}
fn empty(status: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header("Cache-Control", "no-store")
        .body(vec![])
        .unwrap()
}
pub fn response(archive: &LocalArchive, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    if !matches!(request.method().as_str(), "GET" | "HEAD") {
        return empty(StatusCode::METHOD_NOT_ALLOWED);
    }
    let origin = request
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    if origin.is_some_and(|v| !allowed_origin(v)) {
        return empty(StatusCode::FORBIDDEN);
    }
    let Ok(url) = url::Url::parse(&request.uri().to_string()) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(
            (url.scheme(), url.host_str()),
            ("tuck-audio", Some("localhost")) | ("http", Some("tuck-audio.localhost"))
        )
    {
        return empty(StatusCode::FORBIDDEN);
    }
    if request.uri().path() != url.path() || request.uri().path().contains('%') {
        return empty(StatusCode::NOT_FOUND);
    }
    let parts: Vec<_> = url.path().split('/').collect();
    if parts.len() != 3 || !parts[0].is_empty() {
        return empty(StatusCode::NOT_FOUND);
    }
    let mut file = match audio::open_owned(&archive.root, parts[1], parts[2]) {
        Ok(file) => file,
        Err(_) => return empty(StatusCode::NOT_FOUND),
    };
    let info = match audio::inspect(&mut file) {
        Ok(v) => v,
        Err(_) => return empty(StatusCode::UNSUPPORTED_MEDIA_TYPE),
    };
    let length = info.length();
    let header = match request.headers().get("range") {
        None => None,
        Some(v) => match v.to_str() {
            Ok(s) => Some(s),
            Err(_) => return empty(StatusCode::RANGE_NOT_SATISFIABLE),
        },
    };
    let (start, end) = match byte_range(header, length) {
        Ok(v) => v,
        Err(_) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header("Content-Range", format!("bytes */{length}"))
                .body(vec![])
                .unwrap()
        }
    };
    let head = request.method() == "HEAD";
    let bytes = if head {
        vec![]
    } else {
        match info.read_range(&mut file, start, end - start + 1) {
            Ok(v) => v,
            Err(_) => return empty(StatusCode::CONFLICT),
        }
    };
    let mut builder = Response::builder()
        .status(if head && header.is_none() {
            StatusCode::OK
        } else {
            StatusCode::PARTIAL_CONTENT
        })
        .header("Content-Type", "audio/wav")
        .header("Accept-Ranges", "bytes")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(
            "Content-Length",
            if head && header.is_none() {
                length
            } else {
                end - start + 1
            },
        )
        .header(
            "Access-Control-Expose-Headers",
            "Content-Range, Content-Length, Accept-Ranges",
        );
    if !(head && header.is_none()) {
        builder = builder.header("Content-Range", format!("bytes {start}-{end}/{length}"));
    }
    if let Some(origin) = origin {
        builder = builder
            .header("Access-Control-Allow-Origin", origin)
            .header("Vary", "Origin");
    }
    builder.body(bytes).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranges_are_bounded_and_reject_malformed_input() {
        assert_eq!(byte_range(Some("bytes=5-8"), 10).unwrap(), (5, 8));
        assert_eq!(byte_range(Some("bytes=5-"), 10).unwrap(), (5, 9));
        assert_eq!(byte_range(Some("bytes=-3"), 10).unwrap(), (7, 9));
        assert_eq!(byte_range(Some("bytes=0-999"), 10).unwrap(), (0, 9));
        assert_eq!(
            byte_range(None, u32::MAX as u64).unwrap(),
            (0, audio::MAX_CHUNK - 1)
        );
        for range in [
            "bytes=-0",
            "bytes=-+3",
            "bytes=10-",
            "bytes=8-5",
            "bytes=0-1,4-5",
            "bytes=18446744073709551616-",
            "bytes=0-18446744073709551616",
            "bytes=+1-5",
            "bytes=1 -5",
            "bytes=",
            "bytes=-",
            "items=0-1",
        ] {
            assert!(byte_range(Some(range), 10).is_err(), "{range}");
        }
        assert!(byte_range(None, 0).is_err());
    }
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use std::fs;
    #[test]
    fn synthetic_media_is_scoped_bounded_and_range_seekable() {
        let dir = tempfile::tempdir().unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let path = dir.path().join("audio").join(&id);
        fs::create_dir_all(&path).unwrap();
        audio::tests::fixture(&path.join("microphone.wav"), 8 * 1024 * 1024, true);
        let archive = LocalArchive::new(dir.path());
        let uri = format!("tuck-audio://localhost/{id}/microphone.wav");
        let request = Request::builder()
            .uri(&uri)
            .header("Range", "bytes=0-")
            .header("Origin", "tauri://localhost")
            .body(vec![])
            .unwrap();
        let reply = response(&archive, &request);
        assert_eq!(reply.status(), 206);
        assert_eq!(reply.body().len(), audio::MAX_CHUNK as usize);
        assert_eq!(&reply.body()[..4], b"RIFF");
        assert_eq!(
            reply.headers()["Content-Range"],
            format!("bytes 0-{}/{}", audio::MAX_CHUNK - 1, 8 * 1024 * 1024 + 44)
        );
        let reply = response(
            &archive,
            &Request::builder()
                .uri(&uri)
                .header("Range", "bytes=-20")
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(reply.status(), 206);
        assert_eq!(reply.body().len(), 20);
        let reply = response(
            &archive,
            &Request::builder()
                .method("HEAD")
                .uri(&uri)
                .body(vec![])
                .unwrap(),
        );
        assert_eq!(reply.status(), 200);
        assert!(reply.body().is_empty());
        assert_eq!(
            reply.headers()["Content-Length"],
            (8 * 1024 * 1024 + 44).to_string()
        );
        for uri in [
            format!("tuck-audio://evil.test/{id}/microphone.wav"),
            format!("tuck-audio://localhost/{id}/%2e%2e%2fsecret"),
            format!("tuck-audio://localhost/{id}/microphone.wav?path=/etc/passwd"),
        ] {
            assert!(
                !response(&archive, &Request::builder().uri(uri).body(vec![]).unwrap())
                    .status()
                    .is_success()
            );
        }
        assert_eq!(
            response(
                &archive,
                &Request::builder()
                    .uri(&uri)
                    .header("Origin", "https://evil.test")
                    .body(vec![])
                    .unwrap()
            )
            .status(),
            403
        );
    }
}
