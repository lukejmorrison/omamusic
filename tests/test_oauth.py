#!/usr/bin/env python3
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

from oauth import (  # noqa: E402
    DEVICE_GRANT_TYPE,
    OAUTH_CODE_URL,
    OAUTH_SCOPE,
    OAUTH_TOKEN_URL,
    REFRESH_SKEW_SECS,
    DeviceCode,
    OAuthClient,
    OAuthToken,
    load_token,
    looks_oauth_unsupported,
    looks_refresh_revoked,
    poll_device_token,
    refresh_access_token,
    request_device_code,
    save_token,
    token_from_response,
    verification_link,
)


FIXTURE = Path(__file__).resolve().parent / "fixtures" / "oauth.json"


class ScriptedHttp:
    def __init__(self, responses: list[dict]):
        self.responses = list(responses)
        self.calls: list[tuple[str, dict[str, str]]] = []

    def __call__(self, url: str, fields: dict[str, str]) -> dict:
        self.calls.append((url, fields))
        if not self.responses:
            raise ValueError("no scripted OAuth response")
        return self.responses.pop(0)


def sample_device() -> DeviceCode:
    return DeviceCode(
        device_code="dev-code",
        user_code="ABCD-EFGH",
        verification_url="https://www.google.com/device",
        expires_in=900,
        interval=5,
    )


class OAuthTests(unittest.TestCase):
    def test_request_device_code_parses_interval_and_url(self):
        http = ScriptedHttp([{
            "device_code": "dev",
            "user_code": "WXYZ-1234",
            "verification_url": "https://www.google.com/device",
            "expires_in": 600,
            "interval": 7,
        }])
        code = request_device_code(OAuthClient.default_tv(), http=http)
        self.assertEqual(code.user_code, "WXYZ-1234")
        self.assertEqual(code.interval, 7)
        self.assertEqual(
            verification_link(code),
            "https://www.google.com/device?user_code=WXYZ-1234",
        )
        self.assertEqual(http.calls[0][0], OAUTH_CODE_URL)
        self.assertEqual(http.calls[0][1]["scope"], OAUTH_SCOPE)

    def test_poll_pending_then_authorized_computes_expires_at(self):
        http = ScriptedHttp([
            {"error": "authorization_pending"},
            {
                "access_token": "tok",
                "refresh_token": "ref",
                "token_type": "Bearer",
                "scope": OAUTH_SCOPE,
                "expires_in": 3600,
            },
        ])
        client = OAuthClient.default_tv()
        device = sample_device()
        status, token, interval = poll_device_token(client, device, 1000, http=http)
        self.assertEqual(status, "pending")
        self.assertIsNone(token)
        self.assertEqual(interval, 5)
        status, token, _ = poll_device_token(client, device, 1000, http=http)
        self.assertEqual(status, "authorized")
        assert token is not None
        self.assertEqual(token.access_token, "tok")
        self.assertEqual(token.expires_at, 4600)
        self.assertEqual(http.calls[1][1]["grant_type"], DEVICE_GRANT_TYPE)

    def test_poll_slow_down_adds_five_seconds(self):
        http = ScriptedHttp([{"error": "slow_down"}])
        status, token, interval = poll_device_token(
            OAuthClient.default_tv(), sample_device(), 0, http=http
        )
        self.assertEqual(status, "pending")
        self.assertIsNone(token)
        self.assertEqual(interval, 10)

    def test_poll_denied_and_expired(self):
        for error, expected in (("access_denied", "denied"), ("expired_token", "expired")):
            status, token, _ = poll_device_token(
                OAuthClient.default_tv(),
                sample_device(),
                0,
                http=ScriptedHttp([{"error": error}]),
            )
            self.assertEqual(status, expected)
            self.assertIsNone(token)

    def test_save_token_is_private_and_omits_client_secret(self):
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / "oauth.json"
            token = token_from_response(
                {
                    "access_token": "access",
                    "refresh_token": "refresh",
                    "token_type": "Bearer",
                    "scope": OAUTH_SCOPE,
                    "expires_in": 3600,
                },
                "client.apps.googleusercontent.com",
                9000,
            )
            save_token(dest, token)
            text = dest.read_text(encoding="utf-8")
            self.assertNotIn("client_secret", text)
            self.assertNotIn(OAuthClient.default_tv().client_secret, text)
            self.assertEqual(dest.stat().st_mode & 0o777, 0o600)
            loaded = load_token(dest)
            self.assertEqual(loaded.refresh_token, "refresh")
            self.assertEqual(loaded.as_ytmusic_dict()["access_token"], "access")

    def test_refresh_preserves_refresh_token_when_omitted(self):
        http = ScriptedHttp([{
            "access_token": "next",
            "token_type": "Bearer",
            "expires_in": 1800,
        }])
        token = OAuthToken(
            version=1,
            client_id="client",
            access_token="old",
            refresh_token="refresh",
            token_type="Bearer",
            scope=OAUTH_SCOPE,
            expires_at=10,
            expires_in=3600,
        )
        next_token = refresh_access_token(OAuthClient.default_tv(), token, 50, http=http)
        self.assertEqual(next_token.access_token, "next")
        self.assertEqual(next_token.refresh_token, "refresh")
        self.assertEqual(next_token.expires_at, 1850)
        self.assertEqual(http.calls[0][0], OAUTH_TOKEN_URL)

    def test_refresh_invalid_grant_is_revoked(self):
        with self.assertRaises(ValueError) as caught:
            refresh_access_token(
                OAuthClient.default_tv(),
                OAuthToken(
                    version=1,
                    client_id="client",
                    access_token="old",
                    refresh_token="refresh",
                    token_type="Bearer",
                    scope=OAUTH_SCOPE,
                    expires_at=10,
                    expires_in=3600,
                ),
                50,
                http=ScriptedHttp([{"error": "invalid_grant"}]),
            )
        self.assertTrue(looks_refresh_revoked(str(caught.exception)))

    def test_expiry_window_uses_five_minute_skew(self):
        token = OAuthToken(
            version=1,
            client_id="client",
            access_token="a",
            refresh_token="r",
            token_type="Bearer",
            scope=OAUTH_SCOPE,
            expires_at=1000,
            expires_in=3600,
        )
        self.assertTrue(token.needs_refresh(1000 - REFRESH_SKEW_SECS + 1))
        self.assertFalse(token.needs_refresh(1000 - REFRESH_SKEW_SECS - 1))

    def test_shared_fixture_round_trip(self):
        loaded = load_token(FIXTURE)
        self.assertEqual(loaded.access_token, "ya29.test-access")
        self.assertEqual(loaded.scope, OAUTH_SCOPE)
        with tempfile.TemporaryDirectory() as tmp:
            dest = Path(tmp) / "oauth.json"
            save_token(dest, loaded)
            again = load_token(dest)
            self.assertEqual(again.as_dict(), loaded.as_dict())

    def test_looks_oauth_unsupported(self):
        self.assertTrue(looks_oauth_unsupported(
            "Server returned HTTP 400: Request contains an invalid argument."
        ))
        self.assertFalse(looks_oauth_unsupported("401 unauthorized"))


if __name__ == "__main__":
    unittest.main()
