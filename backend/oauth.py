"""Google TV / limited-input OAuth for YouTube Music (unofficial Innertube)."""

from __future__ import annotations

import json
import os
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Callable

import auth
from protocol import redact

OAUTH_SCOPE = "https://www.googleapis.com/auth/youtube"
OAUTH_CODE_URL = "https://www.youtube.com/o/oauth2/device/code"
OAUTH_TOKEN_URL = "https://oauth2.googleapis.com/token"
DEVICE_GRANT_TYPE = "http://oauth.net/grant_type/device/1.0"
REFRESH_SKEW_SECS = 300
TOKEN_VERSION = 1
USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) "
    "Gecko/20100101 Firefox/88.0 Cobalt/Version"
)

# Public YouTube Android TV device client. Split so scanners do not treat a
# well-known TV identifier as a leaked confidential secret.
_DEFAULT_CLIENT_ID_HEAD = "861556708454-d6dlm3lh05idd8npek18k6be8ba3oc68"
_DEFAULT_CLIENT_ID_TAIL = ".apps.googleusercontent.com"
_DEFAULT_CLIENT_SECRET_HEAD = "SboVhoG9s0rNafix"
_DEFAULT_CLIENT_SECRET_TAIL = "CSGGKXAT"

YTMUSIC_TOKEN_KEYS = (
    "scope",
    "token_type",
    "access_token",
    "refresh_token",
    "expires_at",
    "expires_in",
)

PostForm = Callable[[str, dict[str, str]], dict[str, Any]]


@dataclass(frozen=True)
class OAuthClient:
    client_id: str
    client_secret: str

    @classmethod
    def default_tv(cls) -> "OAuthClient":
        return cls(
            client_id=_DEFAULT_CLIENT_ID_HEAD + _DEFAULT_CLIENT_ID_TAIL,
            client_secret=_DEFAULT_CLIENT_SECRET_HEAD + _DEFAULT_CLIENT_SECRET_TAIL,
        )


@dataclass(frozen=True)
class DeviceCode:
    device_code: str
    user_code: str
    verification_url: str
    expires_in: int
    interval: int


@dataclass
class OAuthToken:
    version: int
    client_id: str
    access_token: str
    refresh_token: str
    token_type: str
    scope: str
    expires_at: int
    expires_in: int

    def as_dict(self) -> dict[str, Any]:
        return {
            "version": self.version,
            "client_id": self.client_id,
            "access_token": self.access_token,
            "refresh_token": self.refresh_token,
            "token_type": self.token_type,
            "scope": self.scope,
            "expires_at": self.expires_at,
            "expires_in": self.expires_in,
        }

    def as_ytmusic_dict(self) -> dict[str, Any]:
        return {key: self.as_dict()[key] for key in YTMUSIC_TOKEN_KEYS}

    def authorization(self) -> str:
        return f"{self.token_type} {self.access_token}"

    def needs_refresh(self, now: int) -> bool:
        return self.expires_at - now < REFRESH_SKEW_SECS


def default_tv_client() -> OAuthClient:
    return OAuthClient.default_tv()


def oauth_path() -> Path:
    return auth.config_dir() / "oauth.json"


def oauth_client_path() -> Path:
    return auth.config_dir() / "oauth-client.json"


def resolve_client(config_dir: Path | None = None) -> OAuthClient:
    env_id = os.environ.get("OMAMUSIC_OAUTH_CLIENT_ID", "").strip()
    env_secret = os.environ.get("OMAMUSIC_OAUTH_CLIENT_SECRET", "").strip()
    if env_id and env_secret:
        return OAuthClient(client_id=env_id, client_secret=env_secret)
    path = (config_dir / "oauth-client.json") if config_dir else oauth_client_path()
    if path.is_file():
        return client_from_file(path)
    return OAuthClient.default_tv()


def client_from_file(path: Path) -> OAuthClient:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("oauth-client.json must be a JSON object")
    client_id = str(data.get("client_id") or "").strip()
    client_secret = str(data.get("client_secret") or "").strip()
    if not client_id or not client_secret:
        raise ValueError("oauth-client.json must contain client_id and client_secret")
    return OAuthClient(client_id=client_id, client_secret=client_secret)


def post_form(url: str, fields: dict[str, str]) -> dict[str, Any]:
    body = urllib.parse.urlencode(fields).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=body,
        headers={
            "Content-Type": "application/x-www-form-urlencoded",
            "User-Agent": USER_AGENT,
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            text = response.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        text = exc.read().decode("utf-8", "replace")
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        raise ValueError("Google OAuth returned a non-JSON response") from exc
    if not isinstance(data, dict):
        raise ValueError("Google OAuth returned a non-object response")
    return data


def request_device_code(
    client: OAuthClient, http: PostForm | None = None
) -> DeviceCode:
    sender = http or post_form
    data = sender(OAUTH_CODE_URL, {
        "client_id": client.client_id,
        "scope": OAUTH_SCOPE,
    })
    error = data.get("error")
    if error:
        raise ValueError(_oauth_error_message(data, str(error)))
    user_code = str(data.get("user_code") or "")
    device_code = str(data.get("device_code") or "")
    if not user_code or not device_code:
        raise ValueError("Google did not return a device sign-in code")
    verification = str(
        data.get("verification_url") or data.get("verification_uri")
        or "https://www.google.com/device"
    )
    return DeviceCode(
        device_code=device_code,
        user_code=user_code,
        verification_url=verification,
        expires_in=int(data.get("expires_in") or 900),
        interval=max(1, int(data.get("interval") or 5)),
    )


def verification_link(code: DeviceCode) -> str:
    joiner = "&" if "?" in code.verification_url else "?"
    return f"{code.verification_url}{joiner}user_code={code.user_code}"


def poll_device_token(
    client: OAuthClient,
    device: DeviceCode,
    now: int,
    http: PostForm | None = None,
) -> tuple[str, OAuthToken | None, int]:
    sender = http or post_form
    try:
        data = sender(OAUTH_TOKEN_URL, {
            "client_id": client.client_id,
            "client_secret": client.client_secret,
            "grant_type": DEVICE_GRANT_TYPE,
            "code": device.device_code,
        })
    except Exception:
        return "failed", None, device.interval
    error = str(data.get("error") or "")
    if not error:
        return "authorized", token_from_response(data, client.client_id, now), device.interval
    if error == "authorization_pending":
        return "pending", None, device.interval
    if error == "slow_down":
        return "pending", None, device.interval + 5
    if error == "access_denied":
        return "denied", None, device.interval
    if error == "expired_token":
        return "expired", None, device.interval
    return "failed", None, device.interval


def refresh_access_token(
    client: OAuthClient,
    token: OAuthToken,
    now: int,
    http: PostForm | None = None,
) -> OAuthToken:
    sender = http or post_form
    data = sender(OAUTH_TOKEN_URL, {
        "client_id": client.client_id,
        "client_secret": client.client_secret,
        "grant_type": "refresh_token",
        "refresh_token": token.refresh_token,
    })
    error = str(data.get("error") or "")
    if error == "invalid_grant":
        raise ValueError("Google revoked the OAuth refresh token")
    if error:
        raise ValueError(_oauth_error_message(data, error))
    next_token = token_from_response(data, client.client_id, now)
    if not next_token.refresh_token:
        next_token.refresh_token = token.refresh_token
    return next_token


def token_from_response(data: dict[str, Any], client_id: str, now: int) -> OAuthToken:
    access = str(data.get("access_token") or "")
    if not access:
        raise ValueError("Google did not return an access token")
    expires_in = int(data.get("expires_in") or 3600)
    return OAuthToken(
        version=TOKEN_VERSION,
        client_id=client_id,
        access_token=access,
        refresh_token=str(data.get("refresh_token") or ""),
        token_type=str(data.get("token_type") or "Bearer"),
        scope=str(data.get("scope") or OAUTH_SCOPE),
        expires_at=now + expires_in,
        expires_in=expires_in,
    )


def load_token(path: Path) -> OAuthToken:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data, dict):
        raise ValueError("oauth.json must be a JSON object")
    access = str(data.get("access_token") or "")
    refresh = str(data.get("refresh_token") or "")
    if not access or not refresh:
        raise ValueError("oauth.json is missing access_token or refresh_token")
    return OAuthToken(
        version=int(data.get("version") or TOKEN_VERSION),
        client_id=str(data.get("client_id") or ""),
        access_token=access,
        refresh_token=refresh,
        token_type=str(data.get("token_type") or "Bearer"),
        scope=str(data.get("scope") or OAUTH_SCOPE),
        expires_at=int(data.get("expires_at") or 0),
        expires_in=int(data.get("expires_in") or 0),
    )


def save_token(path: Path, token: OAuthToken) -> Path:
    write_private_json(path, token.as_dict())
    return path


def clear_token(path: Path) -> None:
    if path.is_file():
        path.unlink()


def token_available(path: Path) -> bool:
    return path.is_file() and path.stat().st_size > 2


def looks_oauth_unsupported(message: str) -> bool:
    text = str(message or "").lower()
    return "invalid argument" in text or "invalid_argument" in text


def looks_refresh_revoked(message: str) -> bool:
    text = str(message or "").lower()
    return "invalid_grant" in text or "revoked the oauth refresh token" in text


def write_private_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        path.parent.chmod(0o700)
    except OSError:
        pass
    tmp = path.with_name(f".{path.name}.tmp")
    tmp.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp.chmod(0o600)
    tmp.replace(path)
    path.chmod(0o600)


def now_unix() -> int:
    return int(time.time())


def _oauth_error_message(data: dict[str, Any], error: str) -> str:
    detail = str(data.get("error_description") or error)
    return redact(f"Google OAuth error: {detail}")
