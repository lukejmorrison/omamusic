use crate::error::{Error, Result};
use crate::json_util::get_text;
use crate::paths::AppPaths;
use crate::protocol::{parse_line_default, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub fn request(
    paths: &AppPaths,
    socket: Option<PathBuf>,
    command: &str,
    payload: Value,
    start: bool,
    timeout: Duration,
) -> Result<Value> {
    let path = socket.unwrap_or_else(|| paths.socket_path());
    if !path.exists() {
        if !start {
            return Err(Error::invalid("backend is not running"));
        }
        start_service()?;
        wait_for_socket(&path, Duration::from_secs(8))?;
    }
    let mut stream =
        UnixStream::connect(&path).map_err(|_| Error::invalid("backend is not running"))?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut body = payload;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("v".into(), json!(PROTOCOL_VERSION));
        obj.insert("id".into(), json!(1));
        obj.insert("command".into(), json!(command));
    }
    let mut line = serde_json::to_vec(&body)?;
    line.push(b'\n');
    stream.write_all(&line)?;
    let mut reader = BufReader::new(stream);
    let mut text = String::new();
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            return Err(Error::invalid("timed out waiting for the backend"));
        }
        text.clear();
        match reader.read_line(&mut text) {
            Ok(0) => return Err(Error::invalid("backend closed the connection")),
            Ok(_) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(err) => return Err(err.into()),
        }
        let Some(message) = parse_line_default(&text) else {
            continue;
        };
        if message.get("type").and_then(Value::as_str) == Some("event") {
            continue;
        }
        if message.get("id") != Some(&json!(1)) && message.get("id") != Some(&json!(1.0)) {
            if message.get("type").and_then(Value::as_str) != Some("response") {
                continue;
            }
        }
        return Ok(message);
    }
}

fn start_service() -> Result<()> {
    let status = Command::new("systemctl")
        .args(["--user", "start", "omamusic.service"])
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err(Error::invalid(
            "could not start omamusic.service — run scripts/setup.sh first",
        )),
    }
}

fn wait_for_socket(path: &std::path::Path, timeout: Duration) -> Result<()> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(Error::invalid("backend did not create a socket"))
}

pub fn format_human(reply: &Value) -> String {
    if reply.get("ok") == Some(&json!(false)) {
        return reply
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Request failed")
            .to_string();
    }
    let result = reply.get("result").cloned().unwrap_or(json!({}));
    if let Some(track) = result.get("track") {
        if track.is_object() {
            let artist = get_text(track, "subtitle");
            let name = get_text(track, "name");
            let playing = result
                .get("playing")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mark = if playing { ">" } else { "||" };
            if artist.is_empty() {
                return format!("{mark} {name}");
            }
            return format!("{mark} {artist} — {name}");
        }
    }
    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".into())
}

pub fn parse_seek(text: &str) -> i64 {
    let text = text.trim();
    if let Ok(ms) = text.parse::<i64>() {
        if !text.contains(':') {
            return if ms < 1000 && !text.ends_with("ms") {
                ms * 1000
            } else {
                ms
            };
        }
    }
    let parts: Vec<i64> = text.split(':').filter_map(|p| p.parse().ok()).collect();
    let mut total = 0i64;
    for part in parts {
        total = total * 60 + part;
    }
    total * 1000
}

pub fn run_command(
    paths: &AppPaths,
    socket: Option<PathBuf>,
    start: bool,
    timeout: Duration,
    json_out: bool,
    human: bool,
    args: &[String],
) -> i32 {
    if args.is_empty() {
        eprintln!("omamusic: missing command. Try `omamusic --help`.");
        return 2;
    }
    let cmd = args[0].as_str();
    let rest = &args[1..];
    let catalog_timeout = Duration::from_secs(45.max(timeout.as_secs()));
    let (command, payload, wait) = match cmd {
        "status" | "health" => ("get_state", json!({}), timeout),
        "pause" => ("pause", json!({}), timeout),
        "next" => ("next", json!({}), timeout),
        "prev" | "previous" => ("previous", json!({}), timeout),
        "stop" => ("stop", json!({}), timeout),
        "toggle" => ("toggle", json!({}), timeout),
        "play" => {
            if rest.is_empty() {
                ("play", json!({}), timeout)
            } else {
                let query = rest.join(" ");
                match request(
                    paths,
                    socket.clone(),
                    "search",
                    json!({"query": query, "filter": "songs", "limit": 8}),
                    start,
                    catalog_timeout,
                ) {
                    Ok(reply) if reply.get("ok") == Some(&json!(true)) => {
                        let items = reply
                            .pointer("/result/items")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        let Some(first) = items
                            .into_iter()
                            .find(|item| !get_text(item, "videoId").is_empty())
                        else {
                            eprintln!("omamusic: no songs matched that search");
                            return 1;
                        };
                        (
                            "load",
                            json!({"video_id": get_text(&first, "videoId"), "name": get_text(&first, "name"), "subtitle": get_text(&first, "subtitle")}),
                            catalog_timeout,
                        )
                    }
                    Ok(reply) => {
                        print_reply(&reply, json_out, human);
                        return 1;
                    }
                    Err(err) => {
                        eprintln!("omamusic: {err}");
                        return exit_for(&err);
                    }
                }
            }
        }
        "play-id" => {
            if rest.is_empty() {
                eprintln!("omamusic: play-id needs a video id");
                return 2;
            }
            ("load", json!({"video_id": rest[0]}), catalog_timeout)
        }
        "volume" => {
            let volume: i64 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(80);
            ("set_volume", json!({"volume": volume}), timeout)
        }
        "seek" => {
            let ms = rest.first().map(|s| parse_seek(s)).unwrap_or(0);
            ("seek", json!({"position_ms": ms}), timeout)
        }
        "shuffle" => {
            let on = rest.first().map(|s| s != "off").unwrap_or(true);
            ("set_shuffle", json!({"shuffle": on}), timeout)
        }
        "repeat" => {
            let mode = match rest.first().map(|s| s.as_str()) {
                Some("all") | Some("context") => "context",
                Some("one") | Some("track") => "track",
                _ => "off",
            };
            ("set_repeat", json!({"mode": mode}), timeout)
        }
        "search" => {
            let query = rest.join(" ");
            (
                "search",
                json!({"query": query, "filter": "songs", "limit": 24}),
                catalog_timeout,
            )
        }
        "queue" => ("get_queue", json!({}), timeout),
        "browse" => {
            let view = rest.first().cloned().unwrap_or_else(|| "home".into());
            ("browse", json!({"view": view}), catalog_timeout)
        }
        "like" => ("like", json!({"liked": true}), timeout),
        "unlike" => ("like", json!({"liked": false}), timeout),
        "raw" => {
            if rest.is_empty() {
                eprintln!("omamusic: raw needs a command name");
                return 2;
            }
            let extra: Value = rest
                .get(1)
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(json!({}));
            let mut payload = extra;
            if let Some(obj) = payload.as_object_mut() {
                obj.insert("command".into(), json!(rest[0]));
            }
            return match request(paths, socket, &rest[0], payload, start, catalog_timeout) {
                Ok(reply) => {
                    print_reply(&reply, json_out, human);
                    if reply.get("ok") == Some(&json!(true)) {
                        0
                    } else {
                        1
                    }
                }
                Err(err) => {
                    eprintln!("omamusic: {err}");
                    exit_for(&err)
                }
            };
        }
        "open" => {
            let _ = Command::new("omarchy")
                .args(["shell", "-q", "wizwam.omamusic.player", "togglePlayer"])
                .status();
            return 0;
        }
        "mini" => {
            let _ = Command::new("omarchy")
                .args(["shell", "-q", "wizwam.omamusic.player", "toggleMiniPlayer"])
                .status();
            return 0;
        }
        other => {
            eprintln!("omamusic: unknown command {other}");
            return 2;
        }
    };
    match request(paths, socket, command, payload, start, wait) {
        Ok(reply) => {
            print_reply(&reply, json_out, human);
            if reply.get("ok") == Some(&json!(true)) {
                0
            } else {
                1
            }
        }
        Err(err) => {
            eprintln!("omamusic: {err}");
            exit_for(&err)
        }
    }
}

fn print_reply(reply: &Value, json_out: bool, human: bool) {
    let tty = atty();
    if json_out || (!human && !tty) {
        println!(
            "{}",
            serde_json::to_string(reply).unwrap_or_else(|_| "{}".into())
        );
    } else {
        println!("{}", format_human(reply));
    }
}

fn atty() -> bool {
    libc_isatty(1)
}

fn libc_isatty(fd: i32) -> bool {
    PathBuf::from(format!("/proc/self/fd/{fd}"))
        .canonicalize()
        .map(|p| p.starts_with("/dev/pts") || p.starts_with("/dev/tty"))
        .unwrap_or(false)
}

fn exit_for(err: &Error) -> i32 {
    match err {
        Error::Invalid(msg) if msg.contains("not running") => 3,
        Error::Invalid(_) if matches!(err.to_string().as_str(), s if s.contains("timed out")) => 1,
        _ => 1,
    }
}
