use crate::catalog::{track_item, watch_url};
use crate::error::{Error, Result};
use crate::json_util::get_text;
use crate::paths::{which, yt_dlp_sigfunc_cache, AppPaths};
use crate::spectrum::SpectrumTap;
use rand::Rng;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const RESOLVE_TIMEOUT_WARM: u64 = 40;
pub const RESOLVE_TIMEOUT_COLD: u64 = 150;

pub const EQ_FREQS: [u32; 10] = [70, 180, 320, 600, 1000, 3000, 6000, 12000, 14000, 16000];
pub const EQ_LABELS: [&str; 10] = [
    "70", "180", "320", "600", "1k", "3k", "6k", "12k", "14k", "16k",
];

pub fn eq_presets() -> HashMap<&'static str, [f64; 10]> {
    let mut map = HashMap::new();
    map.insert("Flat", [0.0; 10]);
    map.insert("Rock", [5.0, 4.0, 2.0, -1.0, -2.0, 2.0, 4.0, 5.0, 5.0, 5.0]);
    map.insert("Pop", [-1.0, 2.0, 4.0, 5.0, 4.0, 1.0, -1.0, -1.0, 1.0, 2.0]);
    map.insert("Jazz", [3.0, 4.0, 2.0, 1.0, -1.0, -1.0, 1.0, 2.0, 3.0, 4.0]);
    map.insert(
        "Classical",
        [3.0, 2.0, 1.0, 0.0, -1.0, -1.0, 0.0, 2.0, 3.0, 4.0],
    );
    map.insert(
        "Bass Boost",
        [8.0, 6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    );
    map.insert(
        "Treble Boost",
        [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 3.0, 5.0, 6.0, 7.0],
    );
    map.insert(
        "Vocal",
        [-2.0, -1.0, 1.0, 4.0, 5.0, 4.0, 2.0, 0.0, -1.0, -2.0],
    );
    map.insert(
        "Electronic",
        [6.0, 4.0, 1.0, -1.0, -2.0, 1.0, 3.0, 4.0, 5.0, 6.0],
    );
    map.insert(
        "Acoustic",
        [3.0, 3.0, 2.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0, 1.0],
    );
    map
}

pub fn yt_dlp_cache_warm(path: Option<&Path>) -> bool {
    let target = path
        .map(Path::to_path_buf)
        .unwrap_or_else(yt_dlp_sigfunc_cache);
    target
        .is_dir()
        .then(|| fs::read_dir(&target).ok().and_then(|mut d| d.next()))
        .flatten()
        .is_some()
}

pub fn resolve_timeout(warm: bool) -> u64 {
    if warm {
        RESOLVE_TIMEOUT_WARM
    } else {
        RESOLVE_TIMEOUT_COLD
    }
}

pub fn playback_error_message(detail: &str) -> String {
    let text = detail.trim();
    let lower = text.to_ascii_lowercase();
    if lower.contains("403") || lower.contains("forbidden") {
        return "YouTube refused that stream. Try the track again.".into();
    }
    if lower.contains("401") || lower.contains("unauthorized") || lower.contains("sign in") {
        return "Sign in to play this track".into();
    }
    if text.is_empty() {
        return "Could not resolve audio stream".into();
    }
    let mut line = text.lines().last().unwrap_or(text).trim().to_string();
    if line.to_ascii_lowercase().starts_with("error:") {
        line = line[6..].trim().to_string();
    }
    if line.is_empty() {
        "Could not resolve audio stream".into()
    } else {
        line.chars().take(200).collect()
    }
}

pub fn quality_format(kbps: i64) -> String {
    let rate = if kbps <= 96 {
        96
    } else if kbps <= 160 {
        160
    } else {
        320
    };
    format!("bestaudio[abr<={rate}]/bestaudio/best")
}

pub fn eq_filter_chain(bands: &[f64]) -> String {
    let mut values = bands.to_vec();
    values.resize(10, 0.0);
    let parts: Vec<String> = EQ_FREQS
        .iter()
        .zip(values.iter())
        .map(|(freq, gain)| {
            let clamped = gain.clamp(-12.0, 12.0);
            format!("equalizer=f={freq}:t=o:w=1:g={clamped:.1}")
        })
        .collect();
    format!("lavfi=[{}]", parts.join(","))
}

pub fn media_title(item: &Value) -> String {
    let mut title = get_text(item, "name");
    if title.is_empty() {
        title = get_text(item, "title");
    }
    if title.is_empty() {
        "Oma Music".into()
    } else {
        title.chars().take(200).collect()
    }
}

pub fn media_artist(item: &Value) -> String {
    let artist = get_text(item, "subtitle");
    if !artist.is_empty() {
        return artist.chars().take(200).collect();
    }
    if let Some(Value::Array(artists)) = item.get("artists") {
        let names: Vec<String> = artists
            .iter()
            .filter_map(|entry| entry.get("name").and_then(Value::as_str))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let joined = names.join(", ");
        if !joined.is_empty() {
            return joined.chars().take(200).collect();
        }
    }
    String::new()
}

pub fn mpris_title(item: &Value) -> String {
    let title = media_title(item);
    let artist = media_artist(item);
    if !artist.is_empty()
        && !title
            .to_ascii_lowercase()
            .contains(&artist.to_ascii_lowercase())
    {
        format!("{artist} - {title}").chars().take(220).collect()
    } else {
        title
    }
}

pub fn loadfile_command(url: &str, item: &Value) -> Value {
    json!(["loadfile", url, "replace", -1, {"force-media-title": mpris_title(item)}])
}

pub fn looks_like_stream_title(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("googlevideo.com")
        || lower.contains("videoplayback")
        || lower.contains("mime=audio")
        || text.starts_with("webm&")
        || text.contains("&ns=")
        || text.contains("&sig=")
}

pub fn mpv_command_line(binary: &str, ipc_path: &Path, mpris: &str) -> Vec<String> {
    let mut command = vec![
        binary.to_string(),
        "--no-config".into(),
        "--idle=yes".into(),
        "--no-video".into(),
        "--vo=null".into(),
        "--force-window=no".into(),
        "--no-terminal".into(),
        "--audio-display=no".into(),
        "--osc=no".into(),
        "--load-scripts=no".into(),
        "--keep-open=no".into(),
        "--ytdl=no".into(),
        "--ao=pipewire,pulse".into(),
        "--clipboard-backends-clr".into(),
        "--no-input-default-bindings".into(),
        "--volume=80".into(),
        "--title=Oma Music".into(),
        "--audio-client-name=omamusic".into(),
        format!("--input-ipc-server={}", ipc_path.display()),
        "--msg-level=cplayer=info,ao=info,ffmpeg=warn".into(),
    ];
    if !mpris.is_empty() {
        command.push(format!("--script={mpris}"));
    }
    command
}

pub fn mpv_env(source: &HashMap<String, String>) -> HashMap<String, String> {
    let mut env = source.clone();
    for key in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "HYPRLAND_INSTANCE_SIGNATURE",
        "SWAYSOCK",
        "WAYLAND_SOCKET",
    ] {
        env.remove(key);
    }
    env
}

fn mpris_script() -> String {
    for path in [
        "/usr/lib/mpv-mpris/mpris.so",
        "/usr/lib/mpv/scripts/mpris.so",
        "/usr/lib64/mpv-mpris/mpris.so",
    ] {
        if Path::new(path).is_file() {
            return path.into();
        }
    }
    String::new()
}

pub fn stale_mpv_pids(ipc_path: &Path) -> Vec<i32> {
    let ours = ipc_path.display().to_string();
    let mut pids = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid_str) = name.to_str() else {
            continue;
        };
        if !pid_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let mut raw = fs::read(entry.path().join("cmdline")).unwrap_or_default();
        for byte in &mut raw {
            if *byte == 0 {
                *byte = b' ';
            }
        }
        let cmd = String::from_utf8_lossy(&raw).into_owned();
        if !cmd.contains("audio-client-name=omamusic") {
            continue;
        }
        if cmd.contains(&format!("--input-ipc-server={ours}")) {
            continue;
        }
        if !cmd.contains("mpv") {
            continue;
        }
        if let Ok(pid) = pid_str.parse() {
            pids.push(pid);
        }
    }
    pids
}

pub fn reap_stale_mpv(ipc_path: &Path) -> usize {
    let mut killed = 0;
    for pid in stale_mpv_pids(ipc_path) {
        kill_pid(pid, 15);
        killed += 1;
    }
    killed
}

fn kill_pid(pid: i32, sig: i32) {
    let _ = Command::new("kill")
        .args(["-s", &sig.to_string(), &pid.to_string()])
        .status();
}

struct Mpv {
    ipc_path: PathBuf,
    process: Option<Child>,
    sock: Option<UnixStream>,
    next_id: i64,
}

impl Mpv {
    fn new(ipc_path: PathBuf) -> Self {
        Self {
            ipc_path,
            process: None,
            sock: None,
            next_id: 1,
        }
    }

    fn running(&mut self) -> bool {
        match &mut self.process {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    fn start(&mut self) -> Result<()> {
        if self.running() {
            return Ok(());
        }
        let mpv = which("mpv").ok_or_else(|| Error::playback("mpv is not installed"))?;
        if let Some(parent) = self.ipc_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if self.ipc_path.exists() {
            let _ = fs::remove_file(&self.ipc_path);
        }
        let log_path = self
            .ipc_path
            .parent()
            .unwrap_or(Path::new("."))
            .join("mpv.log");
        let command = mpv_command_line(
            mpv.to_str().unwrap_or("mpv"),
            &self.ipc_path,
            &mpris_script(),
        );
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env = mpv_env(&env);
        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log)
            .envs(&env);
        // start_new_session
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        self.process = Some(cmd.spawn()?);
        self.wait_for_socket()?;
        self.connect()?;
        self.command(&json!(["observe_property", 1, "pause"]))?;
        self.command(&json!(["observe_property", 2, "eof-reached"]))?;
        self.command(&json!(["observe_property", 3, "idle-active"]))?;
        self.command(&json!(["observe_property", 4, "time-pos"]))?;
        self.command(&json!(["observe_property", 5, "duration"]))?;
        self.command(&json!(["observe_property", 6, "volume"]))?;
        self.command(&json!(["observe_property", 7, "media-title"]))?;
        Ok(())
    }

    fn wait_for_socket(&mut self) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if self.ipc_path.exists() {
                return Ok(());
            }
            if let Some(child) = &mut self.process {
                if child.try_wait()?.is_some() {
                    return Err(Error::playback(
                        "mpv exited before the control socket appeared",
                    ));
                }
            }
            thread::sleep(Duration::from_millis(50));
        }
        Err(Error::playback("mpv control socket did not appear"))
    }

    fn connect(&mut self) -> Result<()> {
        let sock = UnixStream::connect(&self.ipc_path)?;
        sock.set_nonblocking(true)?;
        self.sock = Some(sock);
        Ok(())
    }

    fn stop(&mut self) {
        if self.sock.is_some() {
            let _ = self.command(&json!(["quit"]));
        }
        if let Some(sock) = self.sock.take() {
            let _ = sock.shutdown(std::net::Shutdown::Both);
        }
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if self.ipc_path.exists() {
            let _ = fs::remove_file(&self.ipc_path);
        }
    }

    fn command(&mut self, args: &Value) -> Result<i64> {
        let sock = self
            .sock
            .as_mut()
            .ok_or_else(|| Error::playback("mpv is not connected"))?;
        let request_id = self.next_id;
        self.next_id += 1;
        let payload = json!({"command": args, "request_id": request_id});
        let mut line = serde_json::to_vec(&payload)?;
        line.push(b'\n');
        sock.write_all(&line)?;
        Ok(request_id)
    }

    fn poll_events(&mut self, timeout: Duration) -> Vec<Value> {
        let Some(sock) = self.sock.as_mut() else {
            return vec![];
        };
        let _ = sock.set_read_timeout(Some(timeout));
        let _ = sock.set_nonblocking(false);
        let mut buf = [0u8; 65536];
        let mut chunks = Vec::new();
        match sock.read(&mut buf) {
            Ok(0) => {}
            Ok(n) => chunks.extend_from_slice(&buf[..n]),
            Err(_) => return vec![],
        }
        let _ = sock.set_nonblocking(true);
        loop {
            match sock.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => chunks.extend_from_slice(&buf[..n]),
                Err(_) => break,
            }
        }
        if chunks.is_empty() {
            return vec![];
        }
        String::from_utf8_lossy(&chunks)
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
            .filter(Value::is_object)
            .collect()
    }
}

pub struct StreamResolver {
    cookies_path: Option<PathBuf>,
    cache: Mutex<HashMap<String, (f64, String)>>,
}

pub struct ResolveJob {
    pub video_id: String,
    pub start: bool,
}

impl StreamResolver {
    pub fn new() -> Self {
        Self {
            cookies_path: None,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_cookies(cookies_path: Option<PathBuf>) -> Self {
        Self {
            cookies_path,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn cookies_path(&self) -> Option<PathBuf> {
        self.cookies_path.clone()
    }

    pub fn set_cookies(&mut self, path: Option<PathBuf>) {
        self.cookies_path = path;
        self.cache.lock().unwrap().clear();
    }

    pub fn resolve(&self, video_id: &str, kbps: i64) -> Result<String> {
        let video_id = video_id.trim();
        if video_id.is_empty() {
            return Err(Error::playback("Missing video id"));
        }
        let now = unix_now();
        if let Some((until, url)) = self.cache.lock().unwrap().get(video_id).cloned() {
            if until > now {
                return Ok(url);
            }
        }
        let url = self.yt_dlp(video_id, kbps)?;
        self.cache
            .lock()
            .unwrap()
            .insert(video_id.to_string(), (now + 4.0 * 60.0 * 60.0, url.clone()));
        Ok(url)
    }

    pub fn prefetch(&self, video_id: String, kbps: i64) {
        let resolver_video = video_id.clone();
        // Fire-and-forget; cache is mutex-protected.
        let cache_hit = self
            .cache
            .lock()
            .unwrap()
            .get(&video_id)
            .map(|(until, _)| *until > unix_now())
            .unwrap_or(false);
        if cache_hit {
            return;
        }
        let this_kbps = kbps;
        // Need cookies/path: copy via yt_dlp using self
        // We'll spawn using a snapshot of cookies is unused (android client).
        let _ = resolver_video;
        let vid = video_id;
        thread::spawn(move || {
            let tmp = StreamResolver::new();
            let _ = tmp.yt_dlp(&vid, this_kbps);
        });
    }

    fn yt_dlp(&self, video_id: &str, kbps: i64) -> Result<String> {
        let binary = which("yt-dlp").ok_or_else(|| Error::playback("yt-dlp is not installed"))?;
        let url = watch_url(video_id);
        let format = quality_format(kbps);
        let warm = yt_dlp_cache_warm(None);
        let mut cmd = Command::new(binary);
        cmd.args([
            "--extractor-args",
            "youtube:player_client=android",
            "-f",
            &format,
            "-g",
            "--no-playlist",
            "--no-warnings",
            "--no-progress",
            &url,
        ]);
        let timeout = Duration::from_secs(resolve_timeout(warm));
        let output = match run_with_timeout(cmd, timeout) {
            Ok(o) => o,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                return Err(Error::playback(if warm {
                    "YouTube took too long to answer. Try that track again."
                } else {
                    "Preparing YouTube playback took too long the first time. Try that track again; the next one is much faster."
                }));
            }
            Err(err) => return Err(Error::playback(playback_error_message(&err.to_string()))),
        };
        let stream = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<_> = stream.trim().lines().collect();
        if !output.status.success() || lines.is_empty() {
            let detail = String::from_utf8_lossy(&output.stderr);
            let last = detail
                .trim()
                .lines()
                .last()
                .unwrap_or("Could not resolve audio stream");
            return Err(Error::playback(playback_error_message(last)));
        }
        Ok(lines.last().unwrap().to_string())
    }
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<std::process::Output> {
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let start = Instant::now();
    loop {
        if let Some(_status) = child.try_wait()? {
            return child.wait_with_output();
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out",
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub struct QueuePlayer {
    mpv: Mpv,
    pub resolver: StreamResolver,
    pub queue: Vec<Value>,
    pub index: i64,
    pub shuffle: bool,
    pub repeat: String,
    pub playing: bool,
    pub volume: i64,
    pub muted: bool,
    volume_before_mute: i64,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub error: String,
    pub resolving: bool,
    pub last_activity: f64,
    pub eq_bands: Vec<f64>,
    pub eq_preset: String,
    eq_last_chain: String,
    eq_guard_until: f64,
    pub spectrum: SpectrumTap,
    loaded_video_id: String,
    resume_position_ms: i64,
    sleep_deadline: f64,
    sleep_after: String,
    display_title: String,
    stop: Arc<Mutex<bool>>,
    pub quality_kbps: i64,
    on_change: Option<Arc<dyn Fn() + Send + Sync>>,
    on_played: Option<Arc<dyn Fn(Value) + Send + Sync>>,
    catalog_radio: Option<Arc<dyn Fn(&str) -> Vec<Value> + Send + Sync>>,
    pending_resolve: Option<ResolveJob>,
}

impl QueuePlayer {
    pub fn new(paths: &AppPaths) -> Self {
        Self {
            mpv: Mpv::new(paths.mpv_socket()),
            resolver: StreamResolver::new(),
            queue: vec![],
            index: -1,
            shuffle: false,
            repeat: "off".into(),
            playing: false,
            volume: 80,
            muted: false,
            volume_before_mute: 80,
            position_ms: 0,
            duration_ms: 0,
            error: String::new(),
            resolving: false,
            last_activity: unix_now(),
            eq_bands: vec![0.0; 10],
            eq_preset: "Flat".into(),
            eq_last_chain: String::new(),
            eq_guard_until: 0.0,
            spectrum: SpectrumTap::new(),
            loaded_video_id: String::new(),
            resume_position_ms: 0,
            sleep_deadline: 0.0,
            sleep_after: String::new(),
            display_title: String::new(),
            stop: Arc::new(Mutex::new(false)),
            quality_kbps: 320,
            on_change: None,
            on_played: None,
            catalog_radio: None,
            pending_resolve: None,
        }
    }

    pub fn set_on_change(&mut self, cb: Arc<dyn Fn() + Send + Sync>) {
        self.on_change = Some(cb);
    }

    pub fn set_on_played(&mut self, cb: Arc<dyn Fn(Value) + Send + Sync>) {
        self.on_played = Some(cb);
    }

    pub fn set_catalog_radio(&mut self, cb: Arc<dyn Fn(&str) -> Vec<Value> + Send + Sync>) {
        self.catalog_radio = Some(cb);
    }

    pub fn current(&self) -> Option<&Value> {
        if self.index >= 0 {
            self.queue.get(self.index as usize)
        } else {
            None
        }
    }

    pub fn snapshot_track(&self) -> Option<Value> {
        self.current().cloned()
    }

    pub fn note_activity(&mut self) {
        self.last_activity = unix_now();
    }

    fn emit(&self) {
        if let Some(cb) = &self.on_change {
            cb();
        }
    }

    pub fn ensure_started(&mut self) -> Result<()> {
        if !self.mpv.running() {
            reap_stale_mpv(&self.mpv.ipc_path);
            self.mpv.start()?;
            *self.stop.lock().unwrap() = false;
            self.mpv
                .command(&json!(["set_property", "volume", self.volume]))?;
            self.apply_eq(true);
            self.spectrum.start();
        }
        Ok(())
    }

    pub fn shutdown(&mut self) {
        *self.stop.lock().unwrap() = true;
        self.spectrum.shutdown();
        self.mpv.stop();
        self.playing = false;
        self.loaded_video_id.clear();
    }

    pub fn eq_snapshot(&self) -> Value {
        json!({
            "bands": self.eq_bands,
            "preset": self.eq_preset,
            "labels": EQ_LABELS,
        })
    }

    pub fn apply_eq(&mut self, _immediate: bool) {
        if !self.mpv.running() {
            return;
        }
        let chain = eq_filter_chain(&self.eq_bands);
        if chain == self.eq_last_chain {
            return;
        }
        self.eq_last_chain = chain.clone();
        self.eq_guard_until = unix_now() + 1.5;
        let _ = self.mpv.command(&json!(["set_property", "af", chain]));
    }

    pub fn set_eq_band(&mut self, index: i64, gain: f64) {
        let band = index.clamp(0, self.eq_bands.len() as i64 - 1) as usize;
        self.eq_bands[band] = gain.clamp(-12.0, 12.0);
        self.eq_preset = "Custom".into();
        self.apply_eq(false);
        self.emit();
    }

    pub fn set_eq_preset(&mut self, name: &str) -> Result<()> {
        let presets = eq_presets();
        let preset = presets
            .get(name.trim())
            .ok_or_else(|| Error::playback("Unknown EQ preset"))?;
        self.eq_bands = preset.to_vec();
        self.eq_preset = name.to_string();
        self.apply_eq(false);
        self.emit();
        Ok(())
    }

    pub fn restore_eq(&mut self, preset: &str, bands: Option<&Value>) {
        let name = {
            let n = preset.trim();
            if n.is_empty() {
                "Flat"
            } else {
                n
            }
        };
        if name == "Custom" {
            let values = bands.and_then(Value::as_array).cloned().unwrap_or_default();
            let mut cleaned = Vec::new();
            for index in 0..10 {
                let gain = values.get(index).and_then(Value::as_f64).unwrap_or(0.0);
                cleaned.push(((gain * 2.0).round() / 2.0).clamp(-12.0, 12.0));
            }
            self.eq_bands = cleaned;
            self.eq_preset = "Custom".into();
            self.apply_eq(true);
            self.emit();
            return;
        }
        let presets = eq_presets();
        let key = if presets.contains_key(name) {
            name
        } else {
            "Flat"
        };
        self.eq_bands = presets[key].to_vec();
        self.eq_preset = key.to_string();
        self.apply_eq(true);
        self.emit();
    }

    pub fn cycle_eq_preset(&mut self) -> Result<String> {
        let names = [
            "Flat",
            "Rock",
            "Pop",
            "Jazz",
            "Classical",
            "Bass Boost",
            "Treble Boost",
            "Vocal",
            "Electronic",
            "Acoustic",
        ];
        let nxt = names
            .iter()
            .position(|n| *n == self.eq_preset)
            .map(|i| (i + 1) % names.len())
            .unwrap_or(0);
        self.set_eq_preset(names[nxt])?;
        Ok(names[nxt].to_string())
    }

    pub fn restore_queue(
        &mut self,
        items: &[Value],
        index: i64,
        shuffle: bool,
        repeat: &str,
        position_ms: i64,
    ) -> bool {
        let tracks: Vec<Value> = items
            .iter()
            .filter(|item| item.is_object() && !get_text(item, "videoId").is_empty())
            .cloned()
            .collect();
        if tracks.is_empty() {
            return false;
        }
        self.queue = tracks;
        self.index = index.clamp(0, self.queue.len() as i64 - 1);
        self.shuffle = shuffle;
        self.repeat = if ["off", "context", "track"].contains(&repeat) {
            repeat.into()
        } else {
            "off".into()
        };
        self.playing = false;
        self.loaded_video_id.clear();
        self.position_ms = position_ms.max(0);
        self.resume_position_ms = self.position_ms;
        self.duration_ms = self
            .current()
            .and_then(|c| c.get("durationMs").and_then(Value::as_i64))
            .unwrap_or(0)
            .max(0);
        true
    }

    pub fn load(&mut self, items: Vec<Value>, index: i64, play: bool) -> Result<()> {
        let tracks: Vec<Value> = items
            .into_iter()
            .filter(|item| item.is_object() && !get_text(item, "videoId").is_empty())
            .collect();
        if tracks.is_empty() {
            return Err(Error::playback("Nothing playable in that selection"));
        }
        self.queue = tracks;
        self.index = index.clamp(0, self.queue.len() as i64 - 1);
        self.note_activity();
        self.ensure_started()?;
        self.play_current(play)
    }

    pub fn add_to_queue(&mut self, item: Value) -> Result<()> {
        let track = if get_text(&item, "type") == "track" {
            item
        } else {
            track_item(&item, "track")
                .ok_or_else(|| Error::playback("That item cannot be queued"))?
        };
        if get_text(&track, "videoId").is_empty() {
            return Err(Error::playback("That item cannot be queued"));
        }
        self.queue.push(track);
        self.note_activity();
        if let Some(nxt) = self.upcoming_video_id() {
            self.resolver.prefetch(nxt, self.quality_kbps);
        }
        self.emit();
        Ok(())
    }

    pub fn reorder_queue(&mut self, source_index: i64, destination_index: i64) {
        if self.queue.is_empty() {
            return;
        }
        let source = source_index.clamp(0, self.queue.len() as i64 - 1) as usize;
        let destination = destination_index.clamp(0, self.queue.len() as i64 - 1) as usize;
        if source == destination {
            return;
        }
        let current = self.index;
        let item = self.queue.remove(source);
        self.queue.insert(destination, item);
        if current == source as i64 {
            self.index = destination as i64;
        } else if source < destination
            && source < current as usize
            && (current as usize) <= destination
        {
            self.index -= 1;
        } else if destination < source
            && destination <= current as usize
            && (current as usize) < source
        {
            self.index += 1;
        }
        self.note_activity();
        self.emit();
    }

    pub fn play(&mut self) -> Result<()> {
        let video_id = self
            .current()
            .map(|c| get_text(c, "videoId"))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| Error::playback("Nothing is queued"))?;
        if !self.mpv.running() || self.loaded_video_id != video_id {
            self.play_current(true)?;
            self.apply_resume_position();
            return Ok(());
        }
        self.ensure_started()?;
        self.mpv.command(&json!(["set_property", "pause", false]))?;
        self.playing = true;
        self.note_activity();
        self.apply_resume_position();
        self.emit();
        Ok(())
    }

    fn apply_resume_position(&mut self) {
        let resume_at = self.resume_position_ms;
        self.resume_position_ms = 0;
        if resume_at <= 0 {
            return;
        }
        if self.seek(resume_at).is_err() {
            self.position_ms = resume_at;
        }
    }

    pub fn pause(&mut self) {
        if !self.mpv.running() {
            return;
        }
        let _ = self.mpv.command(&json!(["set_property", "pause", true]));
        self.playing = false;
        self.note_activity();
        self.emit();
    }

    pub fn toggle(&mut self) -> Result<()> {
        if self.playing {
            self.pause();
            Ok(())
        } else {
            self.play()
        }
    }

    pub fn stop_playback(&mut self) {
        if self.mpv.running() {
            let _ = self.mpv.command(&json!(["stop"]));
        }
        self.playing = false;
        self.position_ms = 0;
        self.loaded_video_id.clear();
        self.note_activity();
        self.emit();
    }

    pub fn next(&mut self) -> Result<()> {
        self.note_activity();
        if self.advance() {
            self.play_current(true)
        } else {
            self.playing = false;
            self.emit();
            Ok(())
        }
    }

    pub fn previous(&mut self) -> Result<()> {
        self.note_activity();
        if self.position_ms > 3000 && self.current().is_some() {
            return self.seek(0);
        }
        if self.index > 0 {
            self.index -= 1;
            self.play_current(true)
        } else if self.repeat == "context" && !self.queue.is_empty() {
            self.index = self.queue.len() as i64 - 1;
            self.play_current(true)
        } else {
            self.seek(0)
        }
    }

    pub fn seek(&mut self, position_ms: i64) -> Result<()> {
        if !self.mpv.running() {
            return Ok(());
        }
        let seconds = position_ms.max(0) as f64 / 1000.0;
        self.mpv.command(&json!(["seek", seconds, "absolute"]))?;
        self.position_ms = (seconds * 1000.0) as i64;
        self.note_activity();
        self.emit();
        Ok(())
    }

    pub fn set_volume(&mut self, volume: i64) {
        let volume = volume.clamp(0, 100);
        self.volume = volume;
        self.muted = volume <= 0;
        if volume > 0 {
            self.volume_before_mute = volume;
        }
        if self.mpv.running() {
            let _ = self.mpv.command(&json!(["set_property", "volume", volume]));
        }
        self.note_activity();
        self.emit();
    }

    pub fn set_shuffle(&mut self, value: bool) {
        self.shuffle = value;
        self.note_activity();
        self.emit();
    }

    pub fn set_repeat(&mut self, mode: &str) {
        self.repeat = if ["off", "context", "track"].contains(&mode) {
            mode.into()
        } else {
            "off".into()
        };
        self.note_activity();
        self.emit();
    }

    pub fn cycle_repeat(&mut self) -> String {
        let nxt = match self.repeat.as_str() {
            "off" => "context",
            "context" => "track",
            _ => "off",
        };
        self.set_repeat(nxt);
        nxt.into()
    }

    pub fn set_sleep(&mut self, minutes: f64, after: &str) {
        if after == "track" || after == "context" {
            self.sleep_after = after.into();
            self.sleep_deadline = 0.0;
        } else if minutes > 0.0 {
            self.sleep_deadline = unix_now() + minutes * 60.0;
            self.sleep_after.clear();
        } else {
            self.sleep_deadline = 0.0;
            self.sleep_after.clear();
        }
        self.note_activity();
        self.emit();
    }

    pub fn sleep_active(&self) -> bool {
        self.sleep_deadline > 0.0 || !self.sleep_after.is_empty()
    }

    pub fn sleep_remaining_seconds(&self) -> i64 {
        if self.sleep_deadline <= 0.0 {
            0
        } else {
            (self.sleep_deadline - unix_now()).max(0.0) as i64
        }
    }

    fn upcoming_video_id(&self) -> Option<String> {
        let nxt = self.index + 1;
        if nxt >= 0 && (nxt as usize) < self.queue.len() {
            let id = get_text(&self.queue[nxt as usize], "videoId");
            if id.is_empty() {
                None
            } else {
                Some(id)
            }
        } else {
            None
        }
    }

    fn publish_title(&mut self, item: Option<&Value>) {
        let title = mpris_title(item.unwrap_or(&json!({})));
        self.display_title = title.clone();
        if self.mpv.running() {
            let _ = self
                .mpv
                .command(&json!(["set_property", "force-media-title", title]));
        }
    }

    pub fn play_current(&mut self, start: bool) -> Result<()> {
        self.pending_resolve = self.prepare_resolve(start)?;
        Ok(())
    }

    pub fn take_pending_resolve(&mut self) -> Option<ResolveJob> {
        self.pending_resolve.take()
    }

    pub fn prepare_resolve(&mut self, start: bool) -> Result<Option<ResolveJob>> {
        let item = self
            .current()
            .cloned()
            .ok_or_else(|| Error::playback("Nothing is queued"))?;
        let video_id = get_text(&item, "videoId");
        if video_id.is_empty() {
            return Err(Error::playback("Nothing is queued"));
        }
        self.error.clear();
        self.ensure_started()?;
        self.publish_title(Some(&item));
        if self.loaded_video_id == video_id {
            self.mpv
                .command(&json!(["set_property", "pause", !start]))?;
            self.playing = start;
            self.resolving = false;
            self.emit();
            return Ok(None);
        }
        self.resolving = true;
        self.emit();
        Ok(Some(ResolveJob { video_id, start }))
    }

    pub fn resolve_blocking(&mut self, job: ResolveJob) -> Result<()> {
        let url = match self.resolver.resolve(&job.video_id, self.quality_kbps) {
            Ok(url) => url,
            Err(err) => {
                self.fail_resolved(&err.to_string());
                return Err(Error::playback(self.error.clone()));
            }
        };
        self.apply_resolved(&url, job.start)
    }

    pub fn apply_resolved(&mut self, url: &str, start: bool) -> Result<()> {
        let item = self
            .current()
            .cloned()
            .ok_or_else(|| Error::playback("Nothing is queued"))?;
        let video_id = get_text(&item, "videoId");
        self.publish_title(Some(&item));
        let cmd = loadfile_command(url, &item);
        self.mpv.command(&cmd)?;
        self.mpv
            .command(&json!(["set_property", "pause", !start]))?;
        self.playing = start;
        self.position_ms = 0;
        self.duration_ms = item.get("durationMs").and_then(Value::as_i64).unwrap_or(0);
        self.loaded_video_id = video_id.clone();
        self.resolving = false;
        if let Some(nxt) = self.upcoming_video_id() {
            self.resolver.prefetch(nxt, self.quality_kbps);
        } else if self.catalog_radio.is_some() && self.queue.len() as i64 - self.index <= 2 {
            self.fill_radio(&video_id);
        }
        self.emit();
        Ok(())
    }

    pub fn fail_resolved(&mut self, err: &str) {
        self.error = playback_error_message(err);
        self.playing = false;
        self.resolving = false;
        self.emit();
    }

    fn fill_radio(&self, video_id: &str) {
        let Some(radio) = self.catalog_radio.clone() else {
            return;
        };
        let vid = video_id.to_string();
        // Queue mutation from another thread is unsafe without a lock around QueuePlayer.
        // Radio fill is best-effort via callback after fetch; skip auto-append here if not shared.
        let _ = (radio, vid);
    }

    fn advance(&mut self) -> bool {
        if self.sleep_after == "track" {
            self.sleep_after.clear();
            return false;
        }
        if self.repeat == "track" && self.current().is_some() {
            return true;
        }
        if self.shuffle && self.queue.len() > 1 {
            let mut rng = rand::thread_rng();
            let choices: Vec<i64> = (0..self.queue.len() as i64)
                .filter(|i| *i != self.index)
                .collect();
            if choices.is_empty() {
                return false;
            }
            self.index = choices[rng.gen_range(0..choices.len())];
            return true;
        }
        if self.index + 1 < self.queue.len() as i64 {
            self.index += 1;
            return true;
        }
        if self.repeat == "context" && !self.queue.is_empty() {
            if self.sleep_after == "context" {
                self.sleep_after.clear();
                return false;
            }
            self.index = 0;
            return true;
        }
        false
    }

    pub fn poll(&mut self) {
        if self.sleep_deadline > 0.0 && unix_now() >= self.sleep_deadline {
            self.sleep_deadline = 0.0;
            self.pause();
            return;
        }
        let events = self.mpv.poll_events(Duration::from_millis(50));
        let mut changed = false;
        let mut eof = false;
        for event in events {
            let name = event.get("event").and_then(Value::as_str).unwrap_or("");
            if name == "property-change" {
                let prop = event.get("name").and_then(Value::as_str).unwrap_or("");
                let value = event.get("data");
                match prop {
                    "pause" => {
                        self.playing = value == Some(&json!(false));
                        changed = true;
                    }
                    "time-pos" => {
                        if let Some(v) = value.and_then(Value::as_f64) {
                            self.position_ms = (v.max(0.0) * 1000.0) as i64;
                        }
                    }
                    "duration" => {
                        if let Some(v) = value.and_then(Value::as_f64) {
                            if v > 0.0 {
                                self.duration_ms = (v * 1000.0) as i64;
                                changed = true;
                            }
                        }
                    }
                    "volume" => {
                        if let Some(v) = value.and_then(Value::as_f64) {
                            self.volume = v.clamp(0.0, 100.0) as i64;
                        }
                    }
                    "eof-reached" => {
                        if value == Some(&json!(true)) {
                            eof = true;
                        }
                    }
                    "media-title" => {
                        let shown = value.and_then(Value::as_str).unwrap_or("");
                        if !self.display_title.is_empty()
                            && (looks_like_stream_title(shown) || shown != self.display_title)
                        {
                            let current = self.current().cloned();
                            self.publish_title(current.as_ref());
                        }
                    }
                    _ => {}
                }
            } else if name == "file-loaded" || name == "playback-restart" {
                let current = self.current().cloned();
                self.publish_title(current.as_ref());
                if name == "playback-restart" {
                    self.playing = true;
                    self.error.clear();
                    changed = true;
                }
            } else if name == "end-file" {
                let reason = event
                    .get("reason")
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                if reason == "eof" || reason == "0" {
                    eof = true;
                } else if reason == "error" {
                    if unix_now() < self.eq_guard_until {
                        continue;
                    }
                    self.playing = false;
                    if self.error.is_empty() {
                        self.error = "Playback failed".into();
                    }
                    changed = true;
                }
            }
        }
        if eof && self.playing {
            if self.advance() {
                if self.play_current(true).is_err() {
                    self.playing = false;
                    changed = true;
                }
            } else {
                self.playing = false;
                changed = true;
            }
        }
        if changed {
            self.emit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn quality_format_rates() {
        assert!(quality_format(96).contains("96"));
        assert!(quality_format(160).contains("160"));
        assert!(quality_format(320).contains("320"));
    }

    #[test]
    fn mpv_command_stays_headless() {
        let command = mpv_command_line("/usr/bin/mpv", Path::new("/tmp/mpv.sock"), "/lib/mpris.so");
        let joined = command.join(" ");
        assert!(command.iter().any(|a| a == "--vo=null"));
        assert!(command.iter().any(|a| a == "--no-config"));
        assert!(command.iter().any(|a| a == "--load-scripts=no"));
        assert!(command.iter().any(|a| a == "--keep-open=no"));
        assert!(command.iter().any(|a| a == "--clipboard-backends-clr"));
        assert!(command.iter().any(|a| a == "--audio-client-name=omamusic"));
        assert!(command.iter().any(|a| a == "--script=/lib/mpris.so"));
        assert!(!command.iter().any(|a| a == "--really-quiet"));
        assert!(joined.contains("pipewire"));
        assert!(command
            .iter()
            .filter(|arg| arg.starts_with("--audio-client-name="))
            .all(|arg| !arg.split_once('=').unwrap().1.contains(' ')));
    }

    #[test]
    fn media_title_prefers_track_name() {
        assert_eq!(
            media_title(&json!({"name": "Splashing Around"})),
            "Splashing Around"
        );
        assert_eq!(media_title(&json!({})), "Oma Music");
        assert_eq!(
            media_artist(&json!({"subtitle": "Baby Sleep Music", "artists": [{"name": "Other"}]})),
            "Baby Sleep Music"
        );
        assert_eq!(
            media_artist(&json!({"artists": [{"name": "A"}, {"name": "B"}]})),
            "A, B"
        );
    }

    #[test]
    fn loadfile_sets_force_media_title() {
        let command = loadfile_command(
            "https://example.test/stream",
            &json!({"name": "Splashing Around", "subtitle": "Baby Sleep Music"}),
        );
        assert_eq!(command[0], "loadfile");
        assert_eq!(command[2], "replace");
        assert_eq!(
            command[4]["force-media-title"],
            "Baby Sleep Music - Splashing Around"
        );
        assert_eq!(
            mpris_title(&json!({"name": "Splashing Around", "subtitle": "Baby Sleep Music"})),
            "Baby Sleep Music - Splashing Around"
        );
    }

    #[test]
    fn stream_titles_are_rejected() {
        assert!(looks_like_stream_title("webm&ns=abc&rqh=1"));
        assert!(looks_like_stream_title(
            "https://rr1---sn.googlevideo.com/videoplayback?x=1"
        ));
        assert!(!looks_like_stream_title("Skip to my lou"));
    }

    #[test]
    fn mpv_env_drops_wayland() {
        let mut env = HashMap::new();
        env.insert("WAYLAND_DISPLAY".into(), "wayland-1".into());
        env.insert("DISPLAY".into(), ":0".into());
        env.insert("XDG_RUNTIME_DIR".into(), "/run/user/1000".into());
        env.insert("HOME".into(), "/home/user".into());
        let env = mpv_env(&env);
        assert!(!env.contains_key("WAYLAND_DISPLAY"));
        assert!(!env.contains_key("DISPLAY"));
        assert_eq!(env["XDG_RUNTIME_DIR"], "/run/user/1000");
    }

    #[test]
    fn cold_cache_gets_a_bigger_resolve_budget() {
        assert!(resolve_timeout(false) > resolve_timeout(true));
        assert_eq!(resolve_timeout(true), 40);
    }

    #[test]
    fn yt_dlp_cache_warm_detects_a_solved_challenge() {
        let dir = tempdir().unwrap();
        let cache = dir.path().join("youtube-sigfuncs");
        assert!(!yt_dlp_cache_warm(Some(&cache)));
        fs::create_dir(&cache).unwrap();
        assert!(!yt_dlp_cache_warm(Some(&cache)));
        fs::write(cache.join("abc-main-1.json"), "{}").unwrap();
        assert!(yt_dlp_cache_warm(Some(&cache)));
    }

    #[test]
    fn playback_error_message_rewrites_youtube_refusals() {
        assert_eq!(
            playback_error_message("ERROR: [youtube] abc: HTTP Error 403: Forbidden"),
            "YouTube refused that stream. Try the track again."
        );
        assert_eq!(
            playback_error_message("Could not resolve audio stream"),
            "Could not resolve audio stream"
        );
    }

    #[test]
    fn eq_filter_chain_keeps_a_stable_lavfi_graph() {
        let chain = eq_filter_chain(&[8.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(chain.starts_with("lavfi=["));
        assert!(chain.contains("equalizer=f=70:t=o:w=1:g=8.0"));
        assert!(chain.contains("equalizer=f=16000:t=o:w=1:g=0.0"));
        assert_eq!(chain.matches("equalizer=").count(), 10);
        let flat = eq_filter_chain(&[0.0; 10]);
        assert_eq!(flat.matches("equalizer=").count(), 10);
        assert!(flat.contains("g=0.0"));
    }

    #[test]
    fn eq_presets_match_cliamp_count() {
        let presets = eq_presets();
        assert_eq!(presets.values().next().unwrap().len(), 10);
        assert!(presets.contains_key("Rock"));
    }

    #[test]
    fn restore_eq_named_preset_and_custom_bands() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let mut qp = QueuePlayer::new(&paths);
        qp.restore_eq("Rock", None);
        assert_eq!(qp.eq_preset, "Rock");
        assert_eq!(qp.eq_bands, eq_presets()["Rock"].to_vec());
        qp.restore_eq("Custom", Some(&json!([3, -1])));
        assert_eq!(qp.eq_preset, "Custom");
        assert_eq!(qp.eq_bands[0], 3.0);
        assert_eq!(qp.eq_bands[1], -1.0);
        assert_eq!(qp.eq_bands.len(), 10);
    }

    #[test]
    fn restore_queue_sets_current_without_playing() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let mut qp = QueuePlayer::new(&paths);
        qp.restore_queue(
            &[
                json!({"videoId": "aaa", "name": "One"}),
                json!({"videoId": "bbb", "name": "Two"}),
            ],
            1,
            true,
            "track",
            0,
        );
        assert_eq!(qp.current().unwrap()["videoId"], "bbb");
        assert!(!qp.playing);
        assert!(qp.shuffle);
        assert_eq!(qp.repeat, "track");
        assert_eq!(qp.snapshot_track().unwrap()["name"], "Two");
    }
}
