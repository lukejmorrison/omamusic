use serde_json::{json, Value};
use std::sync::Mutex;

pub const PROTOCOL_VERSION: u32 = 1;
pub const BACKEND_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MAX_LINE_BYTES: usize = 256 * 1024;

pub const ERROR_UNSUPPORTED_VERSION: &str = "unsupported_version";
pub const ERROR_UNKNOWN_COMMAND: &str = "unknown_command";
pub const ERROR_INVALID_REQUEST: &str = "invalid_request";
pub const ERROR_AUTH: &str = "auth_failed";
pub const ERROR_UNAVAILABLE: &str = "unavailable";
pub const ERROR_PLAYBACK: &str = "playback_failed";
pub const ERROR_CATALOG: &str = "catalog_failed";

pub fn dumps(payload: &Value) -> String {
    serde_json::to_string(payload).unwrap_or_else(|_| "{}".into())
}

pub fn line_size(text: &str) -> usize {
    text.len()
}

pub fn parse_line(line: &str, max_bytes: usize) -> Option<Value> {
    let text = line.trim();
    if text.is_empty() || line_size(text) > max_bytes {
        return None;
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Some(Value::Object(map)),
        _ => None,
    }
}

pub fn parse_line_default(line: &str) -> Option<Value> {
    parse_line(line, MAX_LINE_BYTES)
}

fn stripped_payload(payload: &Value) -> Value {
    let mut next = payload.clone();
    match next.get("type").and_then(Value::as_str) {
        Some("event") => {
            if let Some(state) = next.get_mut("state").and_then(Value::as_object_mut) {
                state.insert("play_history".into(), json!([]));
                let track = state.get("track").cloned();
                let queue = match track {
                    Some(Value::Object(map)) => json!([Value::Object(map)]),
                    _ => json!([]),
                };
                state.insert("queue".into(), queue);
            }
        }
        Some("response") => {
            if let Some(result) = next.get_mut("result").and_then(Value::as_object_mut) {
                for key in ["home", "items", "sections", "play_history", "queue"] {
                    if result.contains_key(key) {
                        result.insert(key.into(), json!([]));
                    }
                }
            }
        }
        _ => {}
    }
    next
}

pub fn encode_line(payload: &Value, max_bytes: usize) -> Vec<u8> {
    let mut data = format!("{}\n", dumps(payload)).into_bytes();
    if data.len() <= max_bytes {
        return data;
    }
    let slim = stripped_payload(payload);
    data = format!("{}\n", dumps(&slim)).into_bytes();
    if data.len() <= max_bytes {
        return data;
    }
    let fallback = response(
        payload.get("id").cloned(),
        false,
        None,
        Some(ERROR_UNAVAILABLE),
        Some("Response too large"),
    );
    format!("{}\n", dumps(&fallback)).into_bytes()
}

pub fn encode_line_default(payload: &Value) -> Vec<u8> {
    encode_line(payload, MAX_LINE_BYTES)
}

pub fn response(
    request_id: Option<Value>,
    ok: bool,
    result: Option<Value>,
    code: Option<&str>,
    message: Option<&str>,
) -> Value {
    let mut payload = json!({
        "type": "response",
        "v": PROTOCOL_VERSION,
        "id": request_id.unwrap_or(Value::Null),
        "ok": ok,
    });
    if ok {
        payload["result"] = result.unwrap_or_else(|| json!({}));
    } else {
        payload["error"] = json!({
            "code": code.unwrap_or(ERROR_INVALID_REQUEST),
            "message": redact(message.unwrap_or("Request failed")),
        });
    }
    payload
}

pub fn event(name: &str, state: Value) -> Value {
    json!({
        "type": "event",
        "v": PROTOCOL_VERSION,
        "event": name,
        "state": state,
    })
}

pub fn spectrum_event(bands: &[f64]) -> Value {
    let clamped: Vec<f64> = bands
        .iter()
        .map(|value| {
            let clamped = value.clamp(0.0, 1.0);
            (clamped * 1000.0).round() / 1000.0
        })
        .collect();
    json!({
        "type": "event",
        "v": PROTOCOL_VERSION,
        "event": "spectrum",
        "bands": clamped,
    })
}

pub fn redact(value: &str) -> String {
    let mut text = value.to_string();
    let replacements = [
        ("authorization", true),
        ("cookie", true),
        ("sapisid", true),
        ("access_token", false),
        ("refresh_token", false),
    ];
    let lower = text.to_ascii_lowercase();
    for (token, headerish) in replacements {
        if lower.contains(token) {
            text = redact_token(&text, token, headerish);
        }
    }
    text
}

fn redact_token(text: &str, token: &str, headerish: bool) -> String {
    let pattern = if headerish {
        format!(r"(?i)({token}\s*[:=]\s*)([^\s;]+)")
    } else {
        format!(r"(?i)({token}=)[^&\s]+")
    };
    if let Ok(re) = regex::Regex::new(&pattern) {
        if headerish {
            re.replace_all(text, "${1}<redacted>").into_owned()
        } else {
            re.replace_all(text, "${1}<redacted>").into_owned()
        }
    } else {
        text.to_string()
    }
}

pub fn write_locked(
    lock: &Mutex<()>,
    writer: &mut impl std::io::Write,
    data: &[u8],
) -> std::io::Result<()> {
    let _guard = lock.lock().unwrap();
    writer.write_all(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn parse_line_roundtrip() {
        let parsed = parse_line_default(r#"{"command":"ping","id":1,"v":1}"#).unwrap();
        assert_eq!(parsed["command"], "ping");
        assert!(parse_line_default("").is_none());
        assert!(parse_line_default("not-json").is_none());
    }

    #[test]
    fn response_ok_and_error() {
        let ok = response(
            Some(json!(7)),
            true,
            Some(json!({"lifecycle": "ready"})),
            None,
            None,
        );
        assert_eq!(ok["ok"], true);
        assert_eq!(ok["id"], 7);
        assert_eq!(ok["result"]["lifecycle"], "ready");
        let err = response(
            Some(json!(8)),
            false,
            None,
            Some(ERROR_UNSUPPORTED_VERSION),
            Some("nope"),
        );
        assert_eq!(err["ok"], false);
        assert_eq!(err["error"]["code"], ERROR_UNSUPPORTED_VERSION);
    }

    #[test]
    fn parse_line_rejects_oversized_frames() {
        let huge = format!(
            r#"{{"command":"ping","pad":"{}"}}"#,
            "x".repeat(MAX_LINE_BYTES + 8)
        );
        assert!(parse_line_default(&huge).is_none());
    }

    #[test]
    fn encode_line_stays_under_the_ceiling() {
        let queue: Vec<Value> = (0..80).map(|_| json!({"name": "x".repeat(8000)})).collect();
        let history: Vec<Value> = (0..80).map(|_| json!({"name": "y".repeat(8000)})).collect();
        let payload = json!({
            "type": "event",
            "state": {
                "track": {"name": "Song"},
                "queue": queue,
                "play_history": history,
            }
        });
        let encoded = encode_line(&payload, 4096);
        assert!(encoded.len() <= 4096);
        assert!(encoded.ends_with(b"\n"));
    }

    #[test]
    fn spectrum_event_shape() {
        let payload = spectrum_event(&[0.5; 10]);
        assert_eq!(payload["event"], "spectrum");
        assert_eq!(payload["bands"].as_array().unwrap().len(), 10);
        assert!(payload["bands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b == 0.5));
    }

    #[test]
    fn spectrum_event_clamps_and_rounds() {
        let mut bands = vec![-0.4, 0.1234, 2.0];
        bands.extend(std::iter::repeat(0.0).take(7));
        let payload = spectrum_event(&bands);
        assert_eq!(payload["type"], "event");
        assert_eq!(payload["event"], "spectrum");
        assert_eq!(payload["v"], PROTOCOL_VERSION);
        assert_eq!(payload["bands"][0], 0.0);
        assert_eq!(payload["bands"][1], 0.123);
        assert_eq!(payload["bands"][2], 1.0);
        let line = encode_line_default(&payload);
        assert!(line.ends_with(b"\n"));
        assert!(line.len() < 512);
    }

    #[test]
    fn redact_cookie_and_authorization() {
        let text = redact("cookie: SID=supersecret authorization: Bearer abc");
        assert!(!text.contains("supersecret"));
        assert!(!text.contains("Bearer abc"));
        assert!(text.contains("<redacted>"));
    }

    struct Buf(Arc<Mutex<Vec<u8>>>);
    impl Write for Buf {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn write_locked_keeps_ndjson_lines_intact() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let lock = Arc::new(Mutex::new(()));
        let frames = [
            format!(r#"{{"type":"response","id":1,"pad":"{}"}}"#, "A".repeat(80)).into_bytes(),
            br#"{"type":"event","event":"spectrum","bands":[0.1,0.2]}"#.to_vec(),
        ];
        let mut lines = frames
            .into_iter()
            .map(|mut f| {
                f.push(b'\n');
                f
            })
            .collect::<Vec<_>>();
        let workers: Vec<_> = lines
            .drain(..)
            .map(|frame| {
                let store = Arc::clone(&store);
                let lock = Arc::clone(&lock);
                thread::spawn(move || {
                    let mut buf = Buf(store);
                    write_locked(&lock, &mut buf, &frame).unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let raw = store.lock().unwrap().clone();
        let text = String::from_utf8(raw).unwrap();
        let parsed: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(parsed.len(), 2);
        let mut kinds: Vec<_> = parsed
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect();
        kinds.sort();
        assert_eq!(kinds, ["event", "response"]);
    }
}
