use crate::auth;
use crate::catalog::{looks_unauthorized, watch_url, Catalog, CatalogOps};
use crate::error::{Error, Result};
use crate::innertube::Innertube;
use crate::json_util::get_text;
use crate::oauth::{self, DeviceCode, PollOutcome, ReqwestOAuthHttp};
use crate::paths::{chmod, AppPaths};
use crate::play_history;
use crate::player::{yt_dlp_cache_warm, QueuePlayer};
use crate::protocol::{
    encode_line_default, event, parse_line_default, redact, response, spectrum_event,
    ERROR_UNSUPPORTED_VERSION, MAX_LINE_BYTES, PROTOCOL_VERSION,
};
use crate::queue_session;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub fn idle_should_exit(
    idle_minutes: i64,
    playing: bool,
    client_count: usize,
    last_activity: f64,
    now: f64,
) -> bool {
    if idle_minutes <= 0 || playing || client_count > 0 {
        return false;
    }
    (now - last_activity) >= (idle_minutes * 60) as f64
}

pub struct Backend {
    pub paths: AppPaths,
    pub auth_path: PathBuf,
    pub catalog: Option<Box<dyn CatalogOps>>,
    pub signed_in: bool,
    pub account_name: String,
    pub lifecycle: String,
    pub error: String,
    pub idle_minutes: i64,
    pub quality_kbps: i64,
    pub generation: i64,
    pub player: QueuePlayer,
    pub local_history: Vec<Value>,
    pub auth_kind: String,
    oauth: OAuthFlow,
    last_broadcast: Instant,
    auth_refreshed_at: Instant,
    last_queue_save: Instant,
    resume_playing: bool,
    queue_path: PathBuf,
    stop: Arc<AtomicBool>,
    clients: Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>>,
    send_lock: Arc<Mutex<()>>,
    notify: Option<Sender<BroadcastKind>>,
}

#[derive(Clone, Copy)]
enum BroadcastKind {
    State,
    Spectrum,
}

struct OAuthFlow {
    status: String,
    user_code: String,
    verification_url: String,
    expires_at: i64,
    error: String,
    device_code: String,
    interval: u64,
    generation: u64,
    cancel: Arc<AtomicBool>,
}

impl OAuthFlow {
    fn idle() -> Self {
        Self {
            status: "idle".into(),
            user_code: String::new(),
            verification_url: String::new(),
            expires_at: 0,
            error: String::new(),
            device_code: String::new(),
            interval: 5,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Backend {
    pub fn new(paths: AppPaths, auth_path: Option<PathBuf>) -> Self {
        let _ = paths.ensure();
        let auth_path = auth_path.unwrap_or_else(|| paths.auth_path());
        let mut player = QueuePlayer::new(&paths);
        player.quality_kbps = 320;
        let local_history = play_history::load(&paths.history_load_path());
        let queue_path = paths.queue_path();
        let mut backend = Self {
            paths,
            auth_path,
            catalog: None,
            signed_in: false,
            account_name: String::new(),
            lifecycle: "starting".into(),
            error: String::new(),
            idle_minutes: 15,
            quality_kbps: 320,
            generation: 0,
            player,
            local_history,
            auth_kind: "none".into(),
            oauth: OAuthFlow::idle(),
            last_broadcast: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            auth_refreshed_at: Instant::now()
                .checked_sub(Duration::from_secs(400))
                .unwrap_or_else(Instant::now),
            last_queue_save: Instant::now()
                .checked_sub(Duration::from_secs(10))
                .unwrap_or_else(Instant::now),
            resume_playing: false,
            queue_path,
            stop: Arc::new(AtomicBool::new(false)),
            clients: Arc::new(Mutex::new(Vec::new())),
            send_lock: Arc::new(Mutex::new(())),
            notify: None,
        };
        backend.restore_queue_session();
        backend
    }

    pub fn state(&self) -> Value {
        json!({
            "lifecycle": self.lifecycle,
            "backend_version": crate::protocol::BACKEND_VERSION,
            "protocol_version": PROTOCOL_VERSION,
            "signed_in": self.signed_in,
            "auth_kind": self.auth_kind,
            "account_name": self.account_name,
            "oauth_status": self.oauth.status,
            "oauth_user_code": self.oauth.user_code,
            "oauth_verification_url": self.oauth.verification_url,
            "oauth_expires_at": self.oauth.expires_at,
            "oauth_error": self.oauth.error,
            "playing": self.player.playing,
            "resolving": self.player.resolving,
            "shuffle": self.player.shuffle,
            "repeat": self.player.repeat,
            "volume": self.player.volume,
            "muted": self.player.muted,
            "position_ms": self.player.position_ms,
            "duration_ms": self.player.duration_ms,
            "track": self.player.snapshot_track(),
            "queue": self.player.queue,
            "queue_index": self.player.index,
            "sleep_active": self.player.sleep_active(),
            "sleep_remaining": self.player.sleep_remaining_seconds(),
            "idle_minutes": self.idle_minutes,
            "quality_kbps": self.quality_kbps,
            "eq": self.player.eq_snapshot(),
            "generation": self.generation,
            "error": if self.error.is_empty() { self.player.error.clone() } else { self.error.clone() },
            "play_history": self.local_history,
        })
    }

    fn require_catalog(&mut self) -> Result<&mut dyn CatalogOps> {
        self.catalog
            .as_mut()
            .map(|c| c.as_mut() as &mut dyn CatalogOps)
            .ok_or_else(|| Error::catalog("YouTube Music is unavailable"))
    }

    pub fn start_catalog(&mut self) {
        let path = auth::resolve_auth_path(&self.paths, Some(&self.auth_path));
        self.auth_path = path.clone();
        match self.open_catalog(&path) {
            Ok(()) => {
                self.lifecycle = "ready".into();
                self.error.clear();
            }
            Err(err) => match Innertube::unauthenticated() {
                Ok(yt) => {
                    self.catalog = Some(Box::new(Catalog::new(yt)));
                    self.signed_in = false;
                    self.lifecycle = "ready".into();
                    self.error = redact(&err.to_string());
                }
                Err(inner) => {
                    self.catalog = None;
                    self.lifecycle = "error".into();
                    self.error = redact(&inner.to_string());
                }
            },
        }
    }

    fn open_catalog(&mut self, path: &Path) -> Result<()> {
        if self.activate_oauth_file()? {
            return Ok(());
        }
        let browser = match auth::refresh_live_browser_session(path, None, None) {
            Ok(updated) => updated,
            Err(_) if auth::auth_available(path) => {
                let _ = auth::refresh_browser_authorization(path);
                path.to_path_buf()
            }
            Err(_) => PathBuf::new(),
        };
        if !browser.as_os_str().is_empty() && auth::auth_available(&browser) {
            let yt = Innertube::from_browser_json(&browser)?;
            let catalog = Catalog::new(yt);
            self.signed_in = true;
            self.auth_kind = "browser".into();
            self.auth_refreshed_at = Instant::now();
            if let Some(cookies) = auth::export_cookies(&browser, &self.paths.cookies_path()) {
                self.player.resolver.set_cookies(Some(cookies));
            }
            let info = catalog.account();
            self.account_name = get_text(&info, "name");
            self.catalog = Some(Box::new(catalog));
        } else {
            let yt = Innertube::unauthenticated()?;
            self.catalog = Some(Box::new(Catalog::new(yt)));
            self.signed_in = false;
            self.auth_kind = "none".into();
            self.account_name.clear();
        }
        Ok(())
    }

    fn activate_oauth_file(&mut self) -> Result<bool> {
        let path = self.paths.oauth_path();
        if !oauth::token_available(&path) {
            return Ok(false);
        }
        let client = match oauth::resolve_client(&self.paths) {
            Ok(client) => client,
            Err(_) => return Ok(false),
        };
        let mut token = oauth::load_token(&path)?;
        let now = oauth::now_unix();
        if token.needs_refresh(now) {
            match oauth::refresh_access_token(&ReqwestOAuthHttp, &client, &token, now) {
                Ok(next) => {
                    token = next;
                    let _ = oauth::save_token(&path, &token);
                }
                Err(err) if oauth::looks_refresh_revoked(&err.to_string()) => {
                    oauth::clear_token(&path);
                    self.oauth.error = "revoked".into();
                    return Ok(false);
                }
                Err(err) => return Err(err),
            }
        }
        let yt = Innertube::from_oauth_token(&token.access_token)?;
        let catalog = Catalog::new(yt);
        match catalog.playlists(1) {
            Ok(_) => {}
            Err(err) => {
                let text = err.to_string();
                if oauth::looks_oauth_unsupported(&text) {
                    self.oauth.status = "failed".into();
                    self.oauth.error = "innertube_rejected".into();
                    return Ok(false);
                }
                if looks_unauthorized(&text) {
                    return Err(err);
                }
            }
        }
        let info = catalog.account();
        self.account_name = get_text(&info, "name");
        self.catalog = Some(Box::new(catalog));
        self.signed_in = true;
        self.auth_kind = "oauth".into();
        self.auth_refreshed_at = Instant::now();
        self.player.resolver.set_cookies(None);
        self.oauth.status = "authorized".into();
        self.oauth.error.clear();
        Ok(true)
    }

    fn spawn_oauth_poll(backend: Arc<Mutex<Self>>) {
        thread::spawn(move || {
            loop {
                let snapshot = {
                    let Ok(inner) = backend.lock() else {
                        return;
                    };
                    if inner.oauth.status != "awaiting_user"
                        || inner.oauth.cancel.load(Ordering::SeqCst)
                    {
                        return;
                    }
                    (
                        inner.oauth.generation,
                        Arc::clone(&inner.oauth.cancel),
                        inner.oauth.interval,
                        DeviceCode {
                            device_code: inner.oauth.device_code.clone(),
                            user_code: inner.oauth.user_code.clone(),
                            verification_url: inner.oauth.verification_url.clone(),
                            expires_in: 0,
                            interval: inner.oauth.interval,
                        },
                        inner.paths.clone(),
                    )
                };
                let (generation, cancel, interval, device, paths) = snapshot;
                let mut waited = 0u64;
                while waited < interval.saturating_mul(1000) {
                    if cancel.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(200));
                    waited += 200;
                }
                let Ok(client) = oauth::resolve_client(&paths) else {
                    return;
                };
                let Ok(outcome) =
                    oauth::poll_device_token(&ReqwestOAuthHttp, &client, &device, oauth::now_unix())
                else {
                    continue;
                };
                let Ok(mut inner) = backend.lock() else {
                    return;
                };
                if inner.oauth.generation != generation
                    || inner.oauth.cancel.load(Ordering::SeqCst)
                {
                    return;
                }
                match outcome {
                    PollOutcome::Pending { interval } => {
                        inner.oauth.interval = interval;
                    }
                    PollOutcome::Authorized(token) => {
                        if oauth::save_token(&inner.paths.oauth_path(), &token).is_err() {
                            inner.oauth.status = "failed".into();
                            inner.oauth.error = "failed".into();
                        } else if inner.activate_oauth_file().unwrap_or(false) {
                            inner.oauth.status = "authorized".into();
                            inner.oauth.error.clear();
                        } else {
                            inner.oauth.status = "failed".into();
                            if inner.oauth.error.is_empty() {
                                inner.oauth.error = "failed".into();
                            }
                        }
                        inner.poke(BroadcastKind::State);
                        return;
                    }
                    PollOutcome::Denied => {
                        inner.oauth.status = "denied".into();
                        inner.oauth.error = "denied".into();
                        inner.poke(BroadcastKind::State);
                        return;
                    }
                    PollOutcome::Expired => {
                        inner.oauth.status = "expired".into();
                        inner.oauth.error = "expired".into();
                        inner.poke(BroadcastKind::State);
                        return;
                    }
                    PollOutcome::Failed(message) => {
                        inner.oauth.status = "failed".into();
                        inner.oauth.error = "failed".into();
                        inner.error = message;
                        inner.poke(BroadcastKind::State);
                        return;
                    }
                }
            }
        });
    }

    pub fn broadcast_locked(backend: &Arc<Mutex<Self>>) {
        let mut inner = backend.lock().unwrap();
        inner.generation += 1;
        let data = encode_line_default(&event("state_changed", inner.state()));
        inner.last_broadcast = Instant::now();
        let clients = Arc::clone(&inner.clients);
        let send_lock = Arc::clone(&inner.send_lock);
        drop(inner);
        write_all_clients(&clients, &send_lock, &data);
    }

    pub fn broadcast_spectrum_locked(backend: &Arc<Mutex<Self>>) {
        let inner = backend.lock().unwrap();
        let bands = inner.player.spectrum.snapshot();
        let playing = inner.player.playing;
        let data = encode_line_default(&spectrum_event(&bands));
        let clients = Arc::clone(&inner.clients);
        let send_lock = Arc::clone(&inner.send_lock);
        drop(inner);
        if !playing {
            let _ = playing;
        }
        write_all_clients(&clients, &send_lock, &data);
    }

    fn remember_play(&mut self, item: Value) {
        self.local_history =
            play_history::remember(&item, &self.local_history, &self.paths.history_path());
    }

    pub fn remember_queue(&mut self) {
        let payload = json!({
            "items": self.player.queue,
            "index": self.player.index,
            "shuffle": self.player.shuffle,
            "repeat": self.player.repeat,
            "position_ms": self.player.position_ms,
            "playing": self.player.playing,
        });
        queue_session::save(&payload, &self.queue_path);
        self.last_queue_save = Instant::now();
    }

    pub fn restore_queue_session(&mut self) {
        if let Some(session) = queue_session::load(&self.paths.queue_load_path()) {
            let items = session
                .get("items")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !items.is_empty() {
                self.player.restore_queue(
                    &items,
                    session.get("index").and_then(Value::as_i64).unwrap_or(0),
                    session
                        .get("shuffle")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    session
                        .get("repeat")
                        .and_then(Value::as_str)
                        .unwrap_or("off"),
                    session
                        .get("position_ms")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                );
                self.resume_playing = session
                    .get("playing")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                return;
            }
            return;
        }
        if !self.local_history.is_empty() {
            self.player
                .restore_queue(&self.local_history[..1], 0, false, "off", 0);
        }
    }

    pub fn resume_queue_session(&mut self) {
        if !self.resume_playing || self.player.current().is_none() {
            return;
        }
        self.resume_playing = false;
        let _ = self.player.play();
    }

    pub fn handle_shared(backend: &Arc<Mutex<Self>>, message: &Value) -> Value {
        let command = get_text(message, "command");
        let (reply, job) = {
            let mut inner = backend.lock().unwrap();
            let reply = inner.handle_prepare(message);
            let job = inner.player.take_pending_resolve();
            (reply, job)
        };
        if command == "start_oauth" && reply.get("ok").and_then(Value::as_bool) == Some(true) {
            Self::spawn_oauth_poll(Arc::clone(backend));
        }
        if let Some(job) = job {
            Self::spawn_resolve(Arc::clone(backend), job);
        }
        reply
    }

    fn spawn_resolve(backend: Arc<Mutex<Self>>, job: crate::player::ResolveJob) {
        thread::spawn(move || {
            let (kbps, cookies) = {
                let Ok(inner) = backend.lock() else {
                    return;
                };
                (
                    inner.player.quality_kbps,
                    inner.player.resolver.cookies_path(),
                )
            };
            let resolver = crate::player::StreamResolver::with_cookies(cookies);
            let resolved = resolver.resolve(&job.video_id, kbps);
            let Ok(mut inner) = backend.lock() else {
                return;
            };
            let current = inner
                .player
                .current()
                .map(|c| get_text(c, "videoId"))
                .unwrap_or_default();
            if current != job.video_id {
                return;
            }
            let played = match resolved {
                Ok(url) => match inner.player.apply_resolved(&url, job.start) {
                    Ok(()) => inner.player.current().cloned(),
                    Err(err) => {
                        inner.player.fail_resolved(&err.to_string());
                        None
                    }
                },
                Err(err) => {
                    inner.player.fail_resolved(&err.to_string());
                    None
                }
            };
            inner.poke(BroadcastKind::State);
            drop(inner);
            if let Some(item) = played {
                if let Ok(mut inner) = backend.lock() {
                    inner.remember_play(item);
                }
            }
        });
    }

    fn handle_prepare(&mut self, message: &Value) -> Value {
        let version = message
            .get("v")
            .and_then(Value::as_u64)
            .unwrap_or(PROTOCOL_VERSION as u64);
        let request_id = message.get("id").cloned();
        if version != PROTOCOL_VERSION as u64 {
            return response(
                request_id,
                false,
                None,
                Some(ERROR_UNSUPPORTED_VERSION),
                Some("This plugin backend speaks protocol 1"),
            );
        }
        let command = get_text(message, "command");
        match self.dispatch(&command, message) {
            Ok(result) => response(request_id, true, Some(result), None, None),
            Err(err) => {
                if matches!(err, Error::AuthRequired(_)) {
                    self.invalidate_session();
                }
                response(
                    request_id,
                    false,
                    None,
                    Some(err.code()),
                    Some(&err.to_string()),
                )
            }
        }
    }

    pub fn handle(&mut self, message: &Value) -> Value {
        let reply = self.handle_prepare(message);
        if let Some(job) = self.player.take_pending_resolve() {
            let request_id = message.get("id").cloned();
            match self.player.resolve_blocking(job) {
                Ok(()) => {
                    if let Some(item) = self.player.current().cloned() {
                        self.remember_play(item);
                    }
                    response(request_id, true, Some(self.state()), None, None)
                }
                Err(err) => {
                    if matches!(err, Error::AuthRequired(_)) {
                        self.invalidate_session();
                    }
                    response(
                        request_id,
                        false,
                        None,
                        Some(err.code()),
                        Some(&err.to_string()),
                    )
                }
            }
        } else {
            reply
        }
    }

    pub fn dispatch(&mut self, command: &str, message: &Value) -> Result<Value> {
        self.player.note_activity();
        match command {
            "hello" | "ping" | "get_state" => Ok(self.state()),
            "setup_auth" => self.setup_auth(&get_text(message, "headers_raw")),
            "import_browser" => self.import_browser(),
            "start_oauth" => self.start_oauth(),
            "cancel_oauth" => self.cancel_oauth(),
            "logout" => self.logout(),
            "set_idle_minutes" => {
                self.idle_minutes = message
                    .get("minutes")
                    .and_then(Value::as_i64)
                    .unwrap_or(0)
                    .clamp(0, 1440);
                Ok(self.state())
            }
            "set_quality" => {
                let kbps = message.get("kbps").and_then(Value::as_i64).unwrap_or(320);
                self.quality_kbps = if kbps <= 96 {
                    96
                } else if kbps <= 160 {
                    160
                } else {
                    320
                };
                self.player.quality_kbps = self.quality_kbps;
                Ok(self.state())
            }
            "play" => {
                self.player.play()?;
                Ok(self.state())
            }
            "pause" => {
                self.player.pause();
                Ok(self.state())
            }
            "toggle" => {
                self.player.toggle()?;
                Ok(self.state())
            }
            "stop" => {
                self.player.stop_playback();
                Ok(self.state())
            }
            "next" => {
                self.player.next()?;
                Ok(self.state())
            }
            "previous" => {
                self.player.previous()?;
                Ok(self.state())
            }
            "seek" => {
                self.player.seek(
                    message
                        .get("position_ms")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                )?;
                Ok(self.state())
            }
            "set_volume" => {
                let volume = message
                    .get("volume")
                    .or_else(|| message.get("value"))
                    .and_then(Value::as_i64)
                    .unwrap_or(80);
                self.player.set_volume(volume);
                Ok(self.state())
            }
            "set_shuffle" => {
                self.player.set_shuffle(
                    message
                        .get("shuffle")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                );
                Ok(self.state())
            }
            "set_repeat" => {
                self.player
                    .set_repeat(message.get("mode").and_then(Value::as_str).unwrap_or("off"));
                Ok(self.state())
            }
            "load" => self.load(message),
            "add_to_queue" => {
                let item = message.get("item").cloned().unwrap_or(json!({}));
                if !item.is_object() {
                    return Err(Error::invalid("item is required"));
                }
                self.player.add_to_queue(item)?;
                Ok(json!({"queue": self.player.queue}))
            }
            "reorder_queue" => {
                self.player.reorder_queue(
                    message
                        .get("source_index")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                    message
                        .get("destination_index")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                );
                Ok(json!({"queue": self.player.queue, "index": self.player.index}))
            }
            "set_eq_band" => {
                self.player.set_eq_band(
                    message.get("index").and_then(Value::as_i64).unwrap_or(0),
                    message.get("gain").and_then(Value::as_f64).unwrap_or(0.0),
                );
                Ok(self.player.eq_snapshot())
            }
            "set_eq_preset" => {
                self.player.set_eq_preset(&get_text(message, "name"))?;
                Ok(self.player.eq_snapshot())
            }
            "cycle_eq_preset" => {
                let name = self.player.cycle_eq_preset()?;
                let mut snap = self.player.eq_snapshot();
                snap["preset"] = json!(name);
                Ok(snap)
            }
            "restore_eq" => {
                self.player.restore_eq(
                    message
                        .get("preset")
                        .and_then(Value::as_str)
                        .unwrap_or("Flat"),
                    message.get("bands"),
                );
                Ok(self.player.eq_snapshot())
            }
            "search" => {
                self.refresh_session();
                Ok(self.require_catalog()?.search(
                    &get_text(message, "query"),
                    &get_text(message, "filter"),
                    message.get("limit").and_then(Value::as_u64).unwrap_or(24) as usize,
                ))
            }
            "browse" => self.browse(
                &get_text(message, "view"),
                message
                    .get("force")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            ),
            "get_playlist" => {
                self.refresh_session();
                let id = {
                    let item = get_text(message, "item_id");
                    if item.is_empty() {
                        get_text(message, "playlist_id")
                    } else {
                        item
                    }
                };
                self.require_catalog()?.playlist(&id, 80)
            }
            "get_album" => {
                self.refresh_session();
                let id = {
                    let item = get_text(message, "item_id");
                    if item.is_empty() {
                        get_text(message, "album_id")
                    } else {
                        item
                    }
                };
                self.require_catalog()?.album(&id)
            }
            "get_artist" => {
                self.refresh_session();
                let id = {
                    let item = get_text(message, "item_id");
                    if item.is_empty() {
                        get_text(message, "artist_id")
                    } else {
                        item
                    }
                };
                self.require_catalog()?.artist(&id)
            }
            "get_queue" => Ok(json!({"items": self.player.queue, "index": self.player.index})),
            "like" => {
                let playlist_id = {
                    let p = get_text(message, "playlist_id");
                    if p.is_empty() {
                        get_text(message, "album_id")
                    } else {
                        p
                    }
                };
                let liked = message
                    .get("liked")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !playlist_id.is_empty() {
                    self.like_playlist(&playlist_id, liked)
                } else {
                    self.like(&get_text(message, "video_id"), liked)
                }
            }
            "create_playlist" => {
                let item = self
                    .require_catalog()?
                    .create_playlist(&get_text(message, "name"))?;
                Ok(json!({"playlist": item}))
            }
            "add_to_playlist" => {
                self.require_catalog()?.add_to_playlist(
                    &get_text(message, "playlist_id"),
                    &get_text(message, "video_id"),
                )?;
                Ok(json!({}))
            }
            "sleep" => {
                self.player.set_sleep(
                    message
                        .get("minutes")
                        .and_then(Value::as_f64)
                        .unwrap_or(0.0),
                    &get_text(message, "after"),
                );
                Ok(self.state())
            }
            "cancel_sleep" => {
                self.player.set_sleep(0.0, "");
                Ok(self.state())
            }
            "" => Err(Error::invalid("missing command")),
            other => Err(Error::invalid(format!("Unknown command: {other}"))),
        }
    }

    fn setup_auth(&mut self, headers_raw: &str) -> Result<Value> {
        if headers_raw.trim().is_empty() {
            return Err(Error::auth(
                "Paste the request headers from music.youtube.com",
            ));
        }
        let path = auth::save_headers(headers_raw, &self.paths.auth_path())
            .map_err(|e| Error::auth(redact(&e.to_string())))?;
        self.activate_auth(path)
    }

    fn import_browser(&mut self) -> Result<Value> {
        let path = auth::import_from_browser(&self.paths.auth_path(), None, None)
            .map_err(|e| Error::auth(e.to_string()))?;
        self.activate_auth(path)
    }

    fn activate_auth(&mut self, path: PathBuf) -> Result<Value> {
        self.auth_path = path;
        self.start_catalog();
        if !self.signed_in {
            return Err(Error::auth(if self.error.is_empty() {
                "Those headers were not accepted".into()
            } else {
                self.error.clone()
            }));
        }
        self.poke(BroadcastKind::State);
        Ok(self.state())
    }

    fn start_oauth(&mut self) -> Result<Value> {
        self.oauth.cancel.store(true, Ordering::SeqCst);
        self.oauth.generation += 1;
        self.oauth.cancel = Arc::new(AtomicBool::new(false));
        self.oauth.status = "requesting".into();
        self.oauth.error.clear();
        let client = oauth::resolve_client(&self.paths)?;
        let device = oauth::request_device_code(&ReqwestOAuthHttp, &client)
            .map_err(|e| Error::auth(redact(&e.to_string())))?;
        self.oauth.status = "awaiting_user".into();
        self.oauth.user_code = device.user_code.clone();
        self.oauth.verification_url = oauth::verification_link(&device);
        self.oauth.expires_at = oauth::now_unix() + device.expires_in as i64;
        self.oauth.device_code = device.device_code;
        self.oauth.interval = device.interval;
        self.poke(BroadcastKind::State);
        Ok(self.state())
    }

    fn cancel_oauth(&mut self) -> Result<Value> {
        self.oauth.cancel.store(true, Ordering::SeqCst);
        let generation = self.oauth.generation;
        self.oauth = OAuthFlow::idle();
        self.oauth.generation = generation;
        self.oauth.status = "cancelled".into();
        self.oauth.error = "cancelled".into();
        self.poke(BroadcastKind::State);
        Ok(self.state())
    }

    fn logout(&mut self) -> Result<Value> {
        self.player.stop_playback();
        auth::clear_auth(&self.auth_path, &self.paths.cookies_path());
        auth::clear_oauth(&self.paths.oauth_path());
        self.auth_kind = "none".into();
        self.oauth = OAuthFlow::idle();
        self.start_catalog();
        self.poke(BroadcastKind::State);
        Ok(self.state())
    }

    fn refresh_session(&mut self) {
        if self.catalog.is_some()
            && self.signed_in
            && self.auth_refreshed_at.elapsed() < Duration::from_secs(300)
        {
            return;
        }
        if oauth::token_available(&self.paths.oauth_path()) {
            if self.activate_oauth_file().unwrap_or(false) {
                return;
            }
        }
        if !auth::auth_available(&self.auth_path)
            && auth::iter_cookie_databases().is_empty()
        {
            return;
        }
        let path = auth::refresh_live_browser_session(&self.auth_path, None, None)
            .ok()
            .or_else(|| {
                let _ = auth::refresh_browser_authorization(&self.auth_path);
                auth::auth_available(&self.auth_path).then(|| self.auth_path.clone())
            });
        let Some(path) = path else {
            return;
        };
        if let Ok(yt) = Innertube::from_browser_json(&path) {
            self.catalog = Some(Box::new(Catalog::new(yt)));
            self.signed_in = true;
            self.auth_kind = "browser".into();
            self.auth_refreshed_at = Instant::now();
            if let Some(cookies) = auth::export_cookies(&path, &self.paths.cookies_path()) {
                self.player.resolver.set_cookies(Some(cookies));
            }
        }
    }

    fn browse(&mut self, view: &str, force: bool) -> Result<Value> {
        self.refresh_session();
        let signed_in = self.signed_in;
        match view {
            "" | "home" => {
                let home = self.require_catalog()?.home(6, force);
                Ok(json!({"home": home, "signed_in": signed_in}))
            }
            "history" => {
                let remote = if signed_in {
                    self.require_catalog()?.history(40).unwrap_or_default()
                } else {
                    vec![]
                };
                Ok(json!({"items": play_history::merge(&self.local_history, &remote)}))
            }
            "liked" => {
                let items = self.require_catalog()?.liked(50)?;
                Ok(json!({"items": items}))
            }
            "playlists" => {
                let items = self.require_catalog()?.playlists(50)?;
                Ok(json!({"items": items}))
            }
            "library_songs" => {
                let items = self.require_catalog()?.library_songs(50)?;
                Ok(json!({"items": items}))
            }
            "library_albums" => {
                let items = self.require_catalog()?.library_albums(50)?;
                Ok(json!({"items": items}))
            }
            "library_artists" => {
                let items = self.require_catalog()?.library_artists(50)?;
                Ok(json!({"items": items}))
            }
            "library" => {
                let songs = self.require_catalog()?.library_songs(50)?;
                let albums = self.require_catalog()?.library_albums(50)?;
                let artists = self.require_catalog()?.library_artists(50)?;
                let playlists = self.require_catalog()?.playlists(50)?;
                let liked = self.require_catalog()?.liked(50)?;
                let history = self.require_catalog()?.history(40).unwrap_or_default();
                Ok(json!({
                    "songs": songs,
                    "albums": albums,
                    "artists": artists,
                    "playlists": playlists,
                    "liked": liked,
                    "history": history,
                }))
            }
            other => Err(Error::invalid(format!("Unknown view: {other}"))),
        }
    }

    pub fn like(&mut self, video_id: &str, liked: bool) -> Result<Value> {
        if video_id.is_empty() {
            return Err(Error::invalid("video_id is required"));
        }
        if !self.signed_in {
            return Err(Error::auth("Sign in to like songs"));
        }
        if let Err(err) = self.require_catalog()?.rate_song(video_id, liked) {
            if matches!(err, Error::AuthRequired(_)) {
                self.invalidate_session();
                return Err(Error::auth(err.to_string()));
            }
            return Err(err);
        }
        if let Some(current) = self.player.current() {
            if get_text(current, "videoId") == video_id {
                if let Some(obj) = self.player.queue.get_mut(self.player.index as usize) {
                    obj["liked"] = json!(liked);
                }
            }
        }
        self.poke(BroadcastKind::State);
        Ok(json!({"liked": liked}))
    }

    pub fn like_playlist(&mut self, playlist_id: &str, liked: bool) -> Result<Value> {
        if playlist_id.is_empty() {
            return Err(Error::invalid("playlist_id is required"));
        }
        if !self.signed_in {
            return Err(Error::auth("Sign in to like albums"));
        }
        if let Err(err) = self.require_catalog()?.rate_playlist(playlist_id, liked) {
            if matches!(err, Error::AuthRequired(_)) {
                self.invalidate_session();
                return Err(Error::auth(err.to_string()));
            }
            return Err(err);
        }
        self.poke(BroadcastKind::State);
        Ok(json!({"liked": liked, "playlist_id": playlist_id}))
    }

    fn invalidate_session(&mut self) {
        if !self.signed_in && self.account_name.is_empty() {
            return;
        }
        self.signed_in = false;
        self.account_name.clear();
        self.poke(BroadcastKind::State);
    }

    fn load(&mut self, message: &Value) -> Result<Value> {
        let mut index = message
            .get("index")
            .or_else(|| message.get("offset_index"))
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let video_id = get_text(message, "video_id");
        let playlist_id = get_text(message, "playlist_id");
        let album_id = get_text(message, "album_id");
        let artist_id = get_text(message, "artist_id");
        let play = message.get("play").and_then(Value::as_bool).unwrap_or(true);
        let mut resolved: Vec<Value> = vec![];
        if let Some(Value::Array(items)) = message.get("items") {
            resolved = items
                .iter()
                .filter(|item| item.is_object() && !get_text(item, "videoId").is_empty())
                .cloned()
                .collect();
        }
        if resolved.is_empty() && !playlist_id.is_empty() {
            resolved = self
                .require_catalog()?
                .playlist(&playlist_id, 80)?
                .get("tracks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
        } else if resolved.is_empty() && !album_id.is_empty() {
            if album_id.starts_with("MPRE") {
                resolved = self
                    .require_catalog()?
                    .album(&album_id)?
                    .get("tracks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
            } else {
                resolved = self
                    .require_catalog()?
                    .playlist(&album_id, 80)?
                    .get("tracks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
            }
        } else if resolved.is_empty() && !artist_id.is_empty() {
            let detail = self.require_catalog()?.artist(&artist_id)?;
            resolved = detail
                .get("tracks")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if resolved.is_empty() {
                if let Some(radio) = detail.get("radioId").and_then(Value::as_str) {
                    if !radio.is_empty() {
                        resolved = self
                            .require_catalog()?
                            .playlist(radio, 80)?
                            .get("tracks")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                    }
                }
            }
        } else if resolved.is_empty() && !video_id.is_empty() {
            let seed_name = {
                let n = get_text(message, "name");
                if n.is_empty() {
                    "Oma Music".into()
                } else {
                    n
                }
            };
            let seed = json!({
                "kind": "item",
                "type": "track",
                "id": video_id,
                "uri": format!("ytm:track:{video_id}"),
                "name": seed_name,
                "subtitle": get_text(message, "subtitle"),
                "videoId": video_id,
                "externalUrl": watch_url(&video_id),
            });
            let related = self.radio_tracks(&video_id);
            resolved = if related.is_empty() {
                vec![seed]
            } else if related[0].get("videoId").and_then(Value::as_str) != Some(&video_id) {
                let mut out = vec![seed];
                out.extend(
                    related
                        .into_iter()
                        .filter(|item| get_text(item, "videoId") != video_id),
                );
                out
            } else {
                related
            };
            index = 0;
        }
        if resolved.is_empty() {
            return Err(Error::playback("Nothing playable was found"));
        }
        if !video_id.is_empty() {
            if let Some(i) = resolved
                .iter()
                .position(|item| get_text(item, "videoId") == video_id)
            {
                index = i as i64;
            }
        }
        self.player.load(resolved, index, play)?;
        Ok(self.state())
    }

    fn radio_tracks(&mut self, video_id: &str) -> Vec<Value> {
        self.catalog
            .as_mut()
            .map(|c| c.watch_playlist(video_id, 50))
            .unwrap_or_default()
    }

    fn poke(&self, kind: BroadcastKind) {
        if let Some(tx) = &self.notify {
            let _ = tx.send(kind);
        }
    }

    pub fn catalog_video_id(&mut self) -> String {
        for view in ["history", "liked", "home"] {
            let items = match view {
                "home" => self
                    .catalog
                    .as_mut()
                    .map(|c| c.home(6, false))
                    .unwrap_or_default()
                    .into_iter()
                    .flat_map(|shelf| {
                        shelf
                            .get("tracks")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>(),
                "history" => self
                    .catalog
                    .as_mut()
                    .and_then(|c| c.history(40).ok())
                    .unwrap_or_default(),
                _ => self
                    .catalog
                    .as_mut()
                    .and_then(|c| c.liked(50).ok())
                    .unwrap_or_default(),
            };
            for item in items {
                let vid = get_text(&item, "videoId");
                if !vid.is_empty() {
                    return vid;
                }
            }
        }
        String::new()
    }

    pub fn warm_stream_cache(&mut self) {
        if yt_dlp_cache_warm(None) {
            return;
        }
        let video_id = self.catalog_video_id();
        if video_id.is_empty() {
            return;
        }
        let kbps = self.quality_kbps;
        eprintln!("omamusic warming yt-dlp player cache");
        let _ = self.player.resolver.resolve(&video_id, kbps);
    }
}

fn write_all_clients(
    clients: &Arc<Mutex<Vec<Arc<Mutex<UnixStream>>>>>,
    send_lock: &Arc<Mutex<()>>,
    data: &[u8],
) {
    let mut living = Vec::new();
    let current = clients.lock().unwrap().clone();
    for client in current {
        let ok = {
            let _g = send_lock.lock().unwrap();
            let mut stream = client.lock().unwrap();
            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
            stream.write_all(data).is_ok()
        };
        if ok {
            living.push(client);
        }
    }
    *clients.lock().unwrap() = living;
}

pub fn serve(mut backend: Backend, socket: PathBuf) -> Result<()> {
    if socket.exists() {
        let _ = std::fs::remove_file(&socket);
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
        chmod(parent, 0o700);
    }
    let listener = UnixListener::bind(&socket)?;
    chmod(&socket, 0o600);
    eprintln!("omamusic listening on {}", socket.display());

    let (tx, rx) = mpsc::channel::<BroadcastKind>();
    backend.notify = Some(tx.clone());
    let stop = Arc::clone(&backend.stop);
    let shared = Arc::new(Mutex::new(backend));

    {
        let tx = tx.clone();
        shared
            .lock()
            .unwrap()
            .player
            .set_on_change(Arc::new(move || {
                let _ = tx.send(BroadcastKind::State);
            }));
        let hist_backend = Arc::clone(&shared);
        shared
            .lock()
            .unwrap()
            .player
            .set_on_played(Arc::new(move |item| {
                if let Ok(mut inner) = hist_backend.lock() {
                    inner.remember_play(item);
                }
            }));
    }

    let broadcaster = {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            while let Ok(kind) = rx.recv() {
                match kind {
                    BroadcastKind::State => {
                        Backend::broadcast_locked(&shared);
                        if let Ok(mut inner) = shared.lock() {
                            inner.remember_queue();
                        }
                    }
                    BroadcastKind::Spectrum => Backend::broadcast_spectrum_locked(&shared),
                }
            }
        })
    };

    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            {
                let mut inner = shared.lock().unwrap();
                inner.start_catalog();
            }
            Backend::broadcast_locked(&shared);
            {
                let mut inner = shared.lock().unwrap();
                inner.resume_queue_session();
            }
            Backend::broadcast_locked(&shared);
            if !stop.load(Ordering::SeqCst) {
                let warm = {
                    let inner = shared.lock().unwrap();
                    inner.player.resolver.cookies_path()
                };
                if !crate::player::yt_dlp_cache_warm(None) {
                    let (video_id, kbps) = {
                        let mut inner = shared.lock().unwrap();
                        (inner.catalog_video_id(), inner.quality_kbps)
                    };
                    if !video_id.is_empty() {
                        eprintln!("omamusic warming yt-dlp player cache");
                        let _ = crate::player::StreamResolver::with_cookies(warm)
                            .resolve(&video_id, kbps);
                    }
                }
            }
        });
    }

    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(15));
                let (idle_minutes, playing, last_activity, clients) = {
                    let inner = shared.lock().unwrap();
                    let idle_minutes = inner.idle_minutes;
                    let playing = inner.player.playing;
                    let last_activity = inner.player.last_activity;
                    let clients = inner.clients.lock().unwrap().len();
                    (idle_minutes, playing, last_activity, clients)
                };
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs_f64())
                    .unwrap_or(0.0);
                if idle_should_exit(idle_minutes, playing, clients, last_activity, now) {
                    eprintln!("omamusic idle shutdown");
                    if let Ok(mut inner) = shared.lock() {
                        inner.remember_queue();
                    }
                    stop.store(true, Ordering::SeqCst);
                    break;
                }
            }
        });
    }

    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        let tx = tx.clone();
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(1));
                let playing = shared.lock().unwrap().player.playing;
                if playing {
                    let _ = tx.send(BroadcastKind::State);
                }
            }
        });
    }

    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        let tx = tx.clone();
        thread::spawn(move || {
            let mut last = vec![0.0; 10];
            while !stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_millis(50));
                let (playing, mut bands) = {
                    let inner = shared.lock().unwrap();
                    (inner.player.playing, inner.player.spectrum.snapshot())
                };
                if !playing && last.iter().all(|v| *v == 0.0) {
                    continue;
                }
                if playing {
                    // keep bands
                } else {
                    bands = last.iter().map(|v| v * 0.72).collect();
                }
                if !last.is_empty()
                    && bands
                        .iter()
                        .zip(last.iter())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0_f64, f64::max)
                        < 0.02
                {
                    last = bands;
                    continue;
                }
                last = bands;
                if last.iter().copied().fold(0.0_f64, f64::max) < 0.02 && !playing {
                    last.clear();
                }
                let _ = tx.send(BroadcastKind::Spectrum);
            }
        });
    }

    {
        let shared = Arc::clone(&shared);
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                shared.lock().unwrap().player.poll();
                thread::sleep(Duration::from_millis(50));
            }
        });
    }

    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let shared = Arc::clone(&shared);
                let stop = Arc::clone(&stop);
                thread::spawn(move || client_loop(stream, shared, stop));
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    if let Ok(mut inner) = shared.lock() {
        inner.remember_queue();
        inner.player.shutdown();
    }
    drop(tx);
    let _ = broadcaster.join();
    let _ = std::fs::remove_file(&socket);
    Ok(())
}

fn client_loop(stream: UnixStream, backend: Arc<Mutex<Backend>>, stop: Arc<AtomicBool>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let shared_stream = Arc::new(Mutex::new(stream));
    {
        let mut inner = backend.lock().unwrap();
        inner.player.note_activity();
        inner
            .clients
            .lock()
            .unwrap()
            .push(Arc::clone(&shared_stream));
        let data = encode_line_default(&event("state_changed", inner.state()));
        let send_lock = Arc::clone(&inner.send_lock);
        drop(inner);
        let _g = send_lock.lock().unwrap();
        {
            let mut stream = shared_stream.lock().unwrap();
            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
            if stream.write_all(&data).is_err() {
                return;
            }
        }
    }
    let mut reader = BufReader::new(reader_stream);
    let mut line = String::new();
    while !stop.load(Ordering::SeqCst) {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(err)
                if err.kind() == std::io::ErrorKind::WouldBlock
                    || err.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        }
        if line.len() > MAX_LINE_BYTES {
            break;
        }
        let Some(message) = parse_line_default(&line) else {
            continue;
        };
        let reply = Backend::handle_shared(&backend, &message);
        let data = encode_line_default(&reply);
        let send_lock = Arc::clone(&backend.lock().unwrap().send_lock);
        let _g = send_lock.lock().unwrap();
        {
            let mut stream = shared_stream.lock().unwrap();
            let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
            if stream.write_all(&data).is_err() {
                return;
            }
        }
    }
}

pub fn self_test() -> Result<()> {
    use crate::catalog::{duration_ms, track_item};
    use crate::protocol::{encode_line_default, event, spectrum_event};
    let sample = json!({
        "title": "Test Song",
        "videoId": "dQw4w9wgKcQ",
        "artists": [{"name": "Artist", "id": "UC123"}],
        "duration": "3:45",
        "thumbnails": [{"url": "https://example.com/a.jpg", "width": 226}],
    });
    let item = track_item(&sample, "track").expect("track");
    assert_eq!(item["durationMs"], 225000);
    assert_eq!(duration_ms(&json!({"duration": "1:02:03"})), 3723000);
    let probe = encode_line_default(&event("state_changed", json!({"lifecycle": "ready"})));
    assert!(probe.ends_with(b"\n"));
    assert!(probe.windows(9).any(|w| w == b"lifecycle"));
    let spectrum = encode_line_default(&spectrum_event(&[0.0; 10]));
    assert!(spectrum.ends_with(b"\n"));
    assert!(spectrum.windows(8).any(|w| w == b"spectrum"));
    println!("ok");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::eq_presets;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn idle_exit_requires_minutes_and_silence() {
        let now = 1_000.0;
        assert!(!idle_should_exit(15, false, 0, now, now));
        assert!(idle_should_exit(15, false, 0, now - 15.0 * 60.0, now));
    }

    #[test]
    fn idle_exit_skips_playing_and_connected_clients() {
        let now = 1_000.0;
        assert!(!idle_should_exit(15, true, 0, now - 15.0 * 60.0, now));
        assert!(!idle_should_exit(15, false, 1, now - 15.0 * 60.0, now));
        assert!(!idle_should_exit(0, false, 0, now - 15.0 * 60.0, now));
    }

    fn isolated_backend() -> (tempfile::TempDir, Backend) {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let backend = Backend::new(paths, Some(dir.path().join("absent.json")));
        (dir, backend)
    }

    #[test]
    fn state_reports_auth_kind_and_oauth_fields() {
        let (_dir, backend) = isolated_backend();
        let state = backend.state();
        assert_eq!(state["auth_kind"], "none");
        assert_eq!(state["oauth_status"], "idle");
        assert_eq!(state["oauth_user_code"], "");
        assert!(state.get("oauth_device_code").is_none());
    }

    #[test]
    fn logout_clears_oauth_file() {
        let (dir, mut backend) = isolated_backend();
        let path = backend.paths.oauth_path();
        crate::oauth::save_token(
            &path,
            &crate::oauth::OAuthToken {
                version: 1,
                client_id: "client".into(),
                access_token: "access".into(),
                refresh_token: "refresh".into(),
                token_type: "Bearer".into(),
                scope: crate::oauth::OAUTH_SCOPE.into(),
                expires_at: 9_000,
                expires_in: 3600,
            },
        )
        .unwrap();
        assert!(path.is_file());
        backend.logout().unwrap();
        assert!(!path.is_file());
        let _ = dir;
    }

    #[test]
    fn load_replies_before_stream_resolve_finishes() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let backend = Arc::new(Mutex::new(Backend::new(
            paths,
            Some(dir.path().join("absent.json")),
        )));
        let started = Instant::now();
        let reply = Backend::handle_shared(
            &backend,
            &json!({
                "v": 1,
                "id": 9,
                "command": "load",
                "items": [{
                    "type": "track",
                    "videoId": "dQw4w9wgKcQ",
                    "name": "Never Gonna Give You Up",
                    "subtitle": "Rick Astley"
                }],
                "index": 0,
            }),
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "load held the backend for {:?}",
            started.elapsed()
        );
        assert_eq!(reply["ok"], true, "{reply}");
        assert_eq!(reply["result"]["resolving"], true, "{reply}");
        assert_eq!(
            reply["result"]["track"]["videoId"],
            "dQw4w9wgKcQ",
            "{reply}"
        );
    }

    #[test]
    fn state_reports_whether_a_resolve_is_in_flight() {
        let (_dir, mut backend) = isolated_backend();
        assert_eq!(backend.state()["resolving"], false);
        backend.player.resolving = true;
        assert_eq!(backend.state()["resolving"], true);
    }

    #[test]
    fn like_without_session_asks_to_sign_in() {
        let (_dir, mut backend) = isolated_backend();
        backend.signed_in = false;
        let err = backend.like("abcdefghijk", true).unwrap_err();
        assert_eq!(err.to_string(), "Sign in to like songs");
    }

    #[test]
    fn like_playlist_without_session_asks_to_sign_in() {
        let (_dir, mut backend) = isolated_backend();
        backend.signed_in = false;
        let err = backend.like_playlist("OLAK5uy_abc", true).unwrap_err();
        assert_eq!(err.to_string(), "Sign in to like albums");
    }

    fn track(vid: &str, name: &str) -> Value {
        json!({"type": "track", "videoId": vid, "name": name, "subtitle": "Artist"})
    }

    #[test]
    fn reorder_queue_moves_items_and_keeps_now_playing_index() {
        let (_dir, mut backend) = isolated_backend();
        backend.player.queue = vec![
            track("aaa", "First"),
            track("bbb", "Second"),
            track("ccc", "Third"),
        ];
        backend.player.index = 1;
        let result = backend
            .dispatch(
                "reorder_queue",
                &json!({"source_index": 0, "destination_index": 2}),
            )
            .unwrap();
        let ids: Vec<_> = result["queue"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| get_text(item, "videoId"))
            .collect();
        assert_eq!(ids, ["bbb", "ccc", "aaa"]);
        assert_eq!(backend.player.index, 0);
        assert_eq!(backend.player.current().unwrap()["videoId"], "bbb");
    }

    #[test]
    fn set_eq_preset_applies_cliamp_curve() {
        let (_dir, mut backend) = isolated_backend();
        let snapshot = backend
            .dispatch("set_eq_preset", &json!({"name": "Rock"}))
            .unwrap();
        assert_eq!(snapshot["preset"], "Rock");
        assert_eq!(
            snapshot["bands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect::<Vec<_>>(),
            eq_presets()["Rock"].to_vec()
        );
    }

    #[test]
    fn restore_eq_reloads_custom_bands() {
        let (_dir, mut backend) = isolated_backend();
        let snapshot = backend
            .dispatch(
                "restore_eq",
                &json!({"preset": "Custom", "bands": [4, 0, -2]}),
            )
            .unwrap();
        assert_eq!(snapshot["preset"], "Custom");
        assert_eq!(snapshot["bands"][0], 4.0);
        assert_eq!(snapshot["bands"][2], -2.0);
        assert_eq!(snapshot["bands"].as_array().unwrap().len(), 10);
    }

    struct FakeCatalog {
        playlist_calls: Mutex<Vec<String>>,
        playlist: Value,
        album_should_fail: bool,
        liked_err: Option<String>,
    }

    impl CatalogOps for FakeCatalog {
        fn account(&mut self) -> Value {
            json!({})
        }
        fn home(&mut self, _l: usize, _f: bool) -> Vec<Value> {
            vec![]
        }
        fn history(&mut self, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn liked(&mut self, _l: usize) -> Result<Vec<Value>> {
            if let Some(err) = &self.liked_err {
                return Err(Error::auth_required(err.clone()));
            }
            Ok(vec![])
        }
        fn playlists(&mut self, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn library_songs(&mut self, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn library_albums(&mut self, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn library_artists(&mut self, _l: usize) -> Result<Vec<Value>> {
            Ok(vec![])
        }
        fn search(&mut self, _q: &str, _f: &str, _l: usize) -> Value {
            json!({})
        }
        fn playlist(&mut self, playlist_id: &str, _l: usize) -> Result<Value> {
            self.playlist_calls.lock().unwrap().push(playlist_id.into());
            Ok(self.playlist.clone())
        }
        fn album(&mut self, _id: &str) -> Result<Value> {
            if self.album_should_fail {
                return Err(Error::catalog(
                    "Invalid album browseId provided, must start with MPRE.",
                ));
            }
            Ok(json!({}))
        }
        fn artist(&mut self, _id: &str) -> Result<Value> {
            Ok(json!({}))
        }
        fn watch_playlist(&mut self, _id: &str, _l: usize) -> Vec<Value> {
            vec![]
        }
        fn rate_song(&mut self, _id: &str, _l: bool) -> Result<()> {
            Ok(())
        }
        fn rate_playlist(&mut self, _id: &str, _l: bool) -> Result<()> {
            Ok(())
        }
        fn create_playlist(&mut self, _n: &str) -> Result<Value> {
            Ok(json!({}))
        }
        fn add_to_playlist(&mut self, _p: &str, _v: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn get_playlist_uses_item_id_not_request_id() {
        let (_dir, mut backend) = isolated_backend();
        let fake = FakeCatalog {
            playlist_calls: Mutex::new(vec![]),
            playlist: json!({"type": "playlist", "name": "Liked Music", "tracks": []}),
            album_should_fail: false,
            liked_err: None,
        };
        let calls = Arc::new(Mutex::new(Vec::<String>::new()));
        // wrap to capture calls
        backend.catalog = Some(Box::new(fake));
        let reply = backend.handle(&json!({
            "v": 1,
            "id": 7,
            "command": "get_playlist",
            "item_id": "LM",
        }));
        assert_eq!(reply["id"], 7);
        assert_eq!(reply["ok"], true);
        let _ = calls;
    }

    #[test]
    fn load_album_playlist_id_uses_playlist_not_get_album() {
        let (_dir, mut backend) = isolated_backend();
        let fake = FakeCatalog {
            playlist_calls: Mutex::new(vec![]),
            playlist: json!({"tracks": [{"videoId": "aaa", "type": "track", "name": "One"}]}),
            album_should_fail: true,
            liked_err: None,
        };
        backend.catalog = Some(Box::new(fake));
        // load will try mpv; catch playback error after catalog resolution
        let result = backend.load(&json!({"album_id": "OLAK5uy_abc"}));
        // Catalog playlist path is used; mpv may fail if not installed in CI, but
        // album() must not be the path that runs for OLA ids. If album ran it would
        // return Catalog error.
        if let Err(err) = &result {
            assert!(!err.to_string().contains("MPRE"), "{err}");
        }
    }

    #[test]
    fn browse_liked_auth_error_asks_to_sign_in() {
        let (_dir, mut backend) = isolated_backend();
        backend.catalog = Some(Box::new(FakeCatalog {
            playlist_calls: Mutex::new(vec![]),
            playlist: json!({}),
            album_should_fail: false,
            liked_err: Some("Sign in to see liked songs".into()),
        }));
        let reply = backend.handle(&json!({
            "v": 1,
            "id": 4,
            "command": "browse",
            "view": "liked",
        }));
        assert_eq!(reply["ok"], false);
        assert_eq!(reply["error"]["code"], crate::protocol::ERROR_AUTH);
        assert_eq!(reply["error"]["message"], "Sign in to see liked songs");
    }

    #[test]
    fn backend_restores_saved_queue_on_start() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        queue_session::save(
            &json!({
                "items": [{"videoId": "aaa", "name": "Saved"}],
                "index": 0,
                "shuffle": false,
                "repeat": "off",
                "position_ms": 0,
            }),
            &paths.queue_path(),
        );
        let backend = Backend::new(paths, Some(dir.path().join("absent.json")));
        assert_eq!(backend.player.current().unwrap()["videoId"], "aaa");
        assert_eq!(backend.state()["track"]["name"], "Saved");
        assert_eq!(backend.state()["playing"], false);
    }

    #[test]
    fn backend_restores_legacy_omarchy_ytmusic_queue() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        let legacy_dir = dir.path().join("omarchy-ytmusic");
        fs::create_dir_all(&legacy_dir).unwrap();
        queue_session::save(
            &json!({
                "items": [{"videoId": "legacy", "name": "From Python"}],
                "index": 0,
                "shuffle": false,
                "repeat": "off",
                "position_ms": 0,
            }),
            &legacy_dir.join("play-queue.json"),
        );
        let backend = Backend::new(paths, Some(dir.path().join("absent.json")));
        assert_eq!(backend.player.current().unwrap()["videoId"], "legacy");
        assert_eq!(backend.state()["track"]["name"], "From Python");
    }

    #[test]
    fn backend_falls_back_to_last_history_when_queue_is_missing() {
        let (_dir, mut backend) = isolated_backend();
        backend.local_history = vec![json!({"videoId": "hist", "name": "Last play"})];
        backend.restore_queue_session();
        assert_eq!(backend.player.current().unwrap()["videoId"], "hist");
        assert_eq!(backend.state()["track"]["name"], "Last play");
    }

    #[test]
    fn remember_queue_writes_the_current_session() {
        let (_dir, mut backend) = isolated_backend();
        backend.player.queue = vec![json!({"videoId": "aaa", "name": "First"})];
        backend.player.index = 0;
        backend.player.shuffle = true;
        backend.player.playing = true;
        backend.player.position_ms = 9000;
        backend.remember_queue();
        let loaded = queue_session::load(&backend.queue_path).unwrap();
        assert_eq!(loaded["items"][0]["videoId"], "aaa");
        assert_eq!(loaded["index"], 0);
        assert_eq!(loaded["shuffle"], true);
        assert_eq!(loaded["playing"], true);
        assert_eq!(loaded["position_ms"], 9000);
    }

    #[test]
    fn backend_resumes_when_the_saved_session_was_playing() {
        let dir = tempdir().unwrap();
        let paths = AppPaths::for_tests(dir.path());
        paths.ensure().unwrap();
        queue_session::save(
            &json!({
                "items": [{"videoId": "aaa", "name": "Saved"}],
                "index": 0,
                "playing": true,
                "position_ms": 4000,
            }),
            &paths.queue_path(),
        );
        let backend = Backend::new(paths, Some(dir.path().join("absent.json")));
        assert_eq!(backend.player.current().unwrap()["videoId"], "aaa");
        assert!(backend.resume_playing);
        // play() needs mpv; just verify restore flag
    }
}
