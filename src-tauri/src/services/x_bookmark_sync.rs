//! v0.5.37 — X (Twitter) bookmark sync.
//!
//! End-to-end pipeline for connecting an X account, storing
//! OAuth tokens, and pulling the user's bookmarked tweets into
//! Recall as searchable memories.
//!
//! ## OAuth flow (PKCE, public client)
//!
//! 1. `start_oauth_flow` generates a code_verifier (random
//!    64-char string) + code_challenge (S256 hash, base64url),
//!    spawns a one-shot HTTP listener on `127.0.0.1:53682`, then
//!    opens the user's system browser to X's authorize URL.
//! 2. User signs into X (if not already) and clicks Authorize.
//! 3. X redirects to `http://127.0.0.1:53682/callback?code=...&state=...`.
//! 4. The loopback listener catches that one request, returns a
//!    "Success — you can close this tab" HTML page, then shuts
//!    down. The auth code is handed back to the caller.
//! 5. Caller exchanges the code for tokens at
//!    `https://api.twitter.com/2/oauth2/token` using PKCE
//!    (code_verifier in the body). No client_secret needed.
//! 6. Tokens persist via `XOAuthTokenRepository`. User row
//!    populated with `x_user_id` + `x_username` after a
//!    follow-up `GET /2/users/me` call.
//!
//! ## Bookmark sync
//!
//! `sync_bookmarks` paginates `GET /2/users/me/bookmarks`
//! (max_results=100), expanding author info so we can stamp the
//! `@username` on each memory. Each tweet's id is used as the
//! `external_id` on the memory row, and `find_by_external_source`
//! gates the create — re-running sync against the same bookmarks
//! is a no-op (idempotent).
//!
//! ## What's NOT here in v0.5.37
//!
//! * Auto-sync (manual "Sync now" button only — v0.5.38 adds
//!   periodic background sync gated on the token's expiry).
//! * Refresh token rotation handling (X doesn't rotate by
//!   default; if they ever do, we'd hit a refresh failure and
//!   the user re-connects).
//! * Token encryption (see migrations.rs comment).

use std::time::{Duration, SystemTime};

use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

use crate::db::repositories::SharedMemoryRepository;
use crate::errors::app_error::{AppError, AppResult};
use crate::models::{MemoryInput, MemorySourceType};

/// Public client ID registered for "Recall Desktop" on
/// developer.x.com. Public clients use PKCE so this is safe to
/// embed; only client_secret would need protection (we don't
/// have one — that's the whole point of "Public" app type).
const X_OAUTH_CLIENT_ID: &str = "Mlh2eVdiY2RkN2RuS1pTX0RpTGQ6MTpjaQ";

/// Pre-registered callback URL on the X dev portal. The loopback
/// HTTP listener binds to this exact port; X validates the
/// redirect_uri parameter against the registered list, so any
/// drift here would break the flow.
const X_OAUTH_CALLBACK_URL: &str = "http://127.0.0.1:53682/callback";

/// Loopback port. Hardcoded because X requires the redirect_uri
/// to exactly match a pre-registered URL.
const LOOPBACK_PORT: u16 = 53682;

const X_AUTHORIZE_URL: &str = "https://twitter.com/i/oauth2/authorize";
const X_TOKEN_URL: &str = "https://api.twitter.com/2/oauth2/token";
const X_USERS_ME_URL: &str = "https://api.twitter.com/2/users/me";
const X_BOOKMARKS_URL: &str = "https://api.twitter.com/2/users/{id}/bookmarks";

/// Source-app stamp on tweet memories. Lets the daily/weekly
/// recap composers route them to a proper section, the search
/// path filter by source, and the auto-tagger pre-seed
/// `twitter` as a topic label.
pub const TWITTER_SOURCE_APP: &str = "twitter";

/// Scopes requested. `offline.access` gets us a refresh token so
/// we can re-auth silently when the access token expires.
const X_SCOPES: &[&str] = &[
    "bookmark.read",
    "tweet.read",
    "users.read",
    "offline.access",
];

/// PKCE-flow shape. Returned by `start_oauth_flow` so the caller
/// can pass `code_verifier` back into `complete_oauth_flow` after
/// the user authorizes — without it the token exchange fails.
#[derive(Debug, Clone)]
pub struct PkceState {
    pub code_verifier: String,
    pub state: String,
    pub authorize_url: String,
}

/// Auth-code captured from the loopback callback. Hand to
/// `exchange_code_for_tokens` next.
#[derive(Debug, Clone)]
pub struct AuthCallback {
    pub code: String,
    pub state: String,
}

/// Stored OAuth token shape. Persisted via
/// `SqliteXOAuthTokenRepository` after a successful exchange.
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct XOAuthRow {
    pub id: String,
    pub x_user_id: String,
    pub x_username: Option<String>,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub scope: Option<String>,
    pub connected_at: String,
    pub last_synced_at: Option<String>,
    pub last_sync_count: i64,
}

/// Build the PKCE pair + authorize URL. Caller is responsible for
/// opening the URL in the user's system browser and then awaiting
/// `wait_for_callback` to catch the redirect.
pub fn start_oauth_flow() -> AppResult<PkceState> {
    // 64-char code_verifier built from two UUIDs (32 hex chars
    // each). Uniform random; the only requirement is 43-128
    // chars from the URL-safe alphabet, which hex satisfies.
    let code_verifier = format!(
        "{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let code_challenge = pkce_challenge(&code_verifier);
    let state = Uuid::new_v4().simple().to_string();

    let scope = X_SCOPES.join(" ");
    let authorize_url = format!(
        "{base}?response_type=code&client_id={client_id}&redirect_uri={redirect}&scope={scope}&state={state}&code_challenge={challenge}&code_challenge_method=S256",
        base = X_AUTHORIZE_URL,
        client_id = urlencoding::encode(X_OAUTH_CLIENT_ID),
        redirect = urlencoding::encode(X_OAUTH_CALLBACK_URL),
        scope = urlencoding::encode(&scope),
        state = urlencoding::encode(&state),
        challenge = urlencoding::encode(&code_challenge),
    );

    Ok(PkceState {
        code_verifier,
        state,
        authorize_url,
    })
}

/// SHA-256 + base64url-encoded code_challenge. The trailing `=`
/// padding gets stripped per RFC 7636.
fn pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let digest = hasher.finalize();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
}

/// Build an `application/x-www-form-urlencoded` body from key-value
/// pairs. We can't use `RequestBuilder::form` because that method
/// requires reqwest features we don't enable (we run with
/// default-features = false + rustls only). One small helper keeps
/// the dep budget tight without pulling in serde_urlencoded just
/// for the OAuth flow.
fn build_urlencoded_form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Spawn the loopback listener and wait for X's redirect. Times
/// out after 5 minutes — the user shouldn't take longer than
/// that to sign in and click Authorize, and parking forever on a
/// dead listener is worse UX than a clean error.
pub async fn wait_for_callback(expected_state: &str) -> AppResult<AuthCallback> {
    let listener = TcpListener::bind(("127.0.0.1", LOOPBACK_PORT))
        .await
        .map_err(|err| {
            AppError::Invalid(format!(
                "Could not bind {LOOPBACK_PORT} for OAuth callback: {err}. \
                 Is another Recall window or app holding the port?"
            ))
        })?;

    let accept_fut = async {
        loop {
            let (mut socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(err) => {
                    eprintln!("[recall][x-oauth] accept error: {err}");
                    continue;
                }
            };

            // Read the request — we only need the request line for
            // the query string. 4 KiB is plenty; X's redirect URL
            // tops out around 200 bytes.
            let mut buf = vec![0u8; 4096];
            let n = match socket.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => continue,
            };
            let request = String::from_utf8_lossy(&buf[..n]).to_string();

            let first_line = request.lines().next().unwrap_or("").to_string();
            // Pattern: `GET /callback?code=...&state=... HTTP/1.1`
            let path_with_query = first_line
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();

            let (code, state, error) = parse_callback_query(&path_with_query);

            // Reply to the browser regardless so the user sees
            // a clean "you can close this tab" page instead of
            // a connection error.
            let body = render_callback_html(error.as_deref(), code.is_some());
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;

            if let Some(err_msg) = error {
                return Err(AppError::Invalid(format!(
                    "X authorization failed: {err_msg}"
                )));
            }

            let Some(code) = code else {
                // Redirect with no code AND no error — could be a
                // probe request, ignore and keep listening.
                continue;
            };
            let state = state.unwrap_or_default();
            if state != expected_state {
                return Err(AppError::Invalid(
                    "OAuth state mismatch — possible CSRF, aborted.".into(),
                ));
            }
            return Ok(AuthCallback { code, state });
        }
    };

    tokio::select! {
        result = accept_fut => result,
        _ = tokio::time::sleep(Duration::from_secs(300)) => Err(AppError::Invalid(
            "X authorization timed out after 5 minutes. Click Connect again to retry.".into(),
        )),
    }
}

/// Parse `code` / `state` / `error` out of a path like
/// `/callback?code=...&state=...`. Returns `(code, state, error)`.
fn parse_callback_query(
    path_with_query: &str,
) -> (Option<String>, Option<String>, Option<String>) {
    let query = path_with_query.split('?').nth(1).unwrap_or("");
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let mut iter = pair.splitn(2, '=');
        let key = iter.next().unwrap_or("");
        let value = iter.next().unwrap_or("");
        let decoded = urlencoding::decode(value)
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| value.to_string());
        match key {
            "code" => code = Some(decoded),
            "state" => state = Some(decoded),
            "error" => error = Some(decoded),
            "error_description" => {
                // Append the description if we already saw an
                // error code, so the user gets the full reason.
                let prefix = error.take().unwrap_or_default();
                error = Some(if prefix.is_empty() {
                    decoded
                } else {
                    format!("{prefix}: {decoded}")
                });
            }
            _ => {}
        }
    }
    (code, state, error)
}

fn render_callback_html(error: Option<&str>, success: bool) -> String {
    let body = if let Some(err) = error {
        format!(
            r#"<h2 style="color:#d33">X authorization failed</h2><p>{err}</p><p>You can close this tab and try again from Recall.</p>"#,
            err = html_escape(err)
        )
    } else if success {
        "<h2>Connected to X ✓</h2><p>You can close this tab — Recall is finishing the handshake.</p>".to_string()
    } else {
        "<h2>Waiting…</h2><p>Recall didn't see an authorization code in this redirect. Try again from Settings.</p>".to_string()
    };
    format!(
        r#"<!doctype html><html><head><title>Recall · X authorization</title>
<style>body{{font-family:-apple-system,BlinkMacSystemFont,Segoe UI,sans-serif;max-width:520px;margin:80px auto;padding:0 20px;color:#222}}h2{{margin-bottom:14px}}p{{line-height:1.55;color:#555}}</style>
</head><body>{body}</body></html>"#
    )
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Token-exchange response from `POST /2/oauth2/token`. X returns
/// access_token + (optional) refresh_token + expires_in seconds.
#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

/// Exchange the auth code from the callback for an access token.
pub async fn exchange_code_for_tokens(
    code: &str,
    code_verifier: &str,
) -> AppResult<XOAuthRow> {
    let client = reqwest::Client::new();
    // We deliberately don't use `RequestBuilder::form` here — that
    // method requires a reqwest feature enabled by default-features
    // which we keep off (rustls only). Hand-build the
    // application/x-www-form-urlencoded body. urlencoding handles
    // the per-pair escape; same crate already pulled in for the
    // authorize URL builder.
    let form_body = build_urlencoded_form(&[
        ("code", code),
        ("grant_type", "authorization_code"),
        ("client_id", X_OAUTH_CLIENT_ID),
        ("redirect_uri", X_OAUTH_CALLBACK_URL),
        ("code_verifier", code_verifier),
    ]);
    let response = client
        .post(X_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(form_body)
        .send()
        .await
        .map_err(|err| AppError::Invalid(format!("X token request failed: {err}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Invalid(format!(
            "X token exchange returned {status}: {body}"
        )));
    }
    let token: TokenResponse = response
        .json()
        .await
        .map_err(|err| AppError::Invalid(format!("X token response parse failed: {err}")))?;

    let expires_at = token
        .expires_in
        .map(|secs| (Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339());

    // Look up the user's id + handle so the Settings UI can show
    // "Connected as @username" instead of an opaque token blob.
    let me = fetch_me(&token.access_token).await?;

    Ok(XOAuthRow {
        id: Uuid::new_v4().to_string(),
        x_user_id: me.id,
        x_username: Some(me.username),
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at,
        scope: token.scope,
        connected_at: Utc::now().to_rfc3339(),
        last_synced_at: None,
        last_sync_count: 0,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct UsersMeResponse {
    data: UserData,
}

#[derive(Debug, Clone, Deserialize)]
struct UserData {
    id: String,
    username: String,
}

async fn fetch_me(access_token: &str) -> AppResult<UserData> {
    let client = reqwest::Client::new();
    let response = client
        .get(X_USERS_ME_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|err| AppError::Invalid(format!("X /users/me failed: {err}")))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::Invalid(format!(
            "X /users/me returned {status}: {body}"
        )));
    }
    let parsed: UsersMeResponse = response.json().await.map_err(|err| {
        AppError::Invalid(format!("X /users/me parse failed: {err}"))
    })?;
    Ok(parsed.data)
}

/// Bookmark response shape. The `expansions=author_id` parameter
/// gives us the author's user object alongside each tweet so we
/// can stamp `@handle` on the memory without a per-tweet user
/// lookup.
#[derive(Debug, Clone, Deserialize)]
struct BookmarksResponse {
    #[serde(default)]
    data: Vec<TweetData>,
    #[serde(default)]
    includes: Option<Includes>,
    #[serde(default)]
    meta: Option<BookmarksMeta>,
}

#[derive(Debug, Clone, Deserialize)]
struct TweetData {
    id: String,
    text: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    author_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct Includes {
    #[serde(default)]
    users: Vec<UserInclude>,
}

#[derive(Debug, Clone, Deserialize)]
struct UserInclude {
    id: String,
    username: String,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct BookmarksMeta {
    #[serde(default)]
    next_token: Option<String>,
}

/// Result counts from a sync run. Surfaced to the frontend so the
/// UI can show "Synced 3 new tweets, 7 already saved" feedback.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BookmarkSyncResult {
    pub fetched: u32,
    pub created: u32,
    pub already_saved: u32,
}

/// Pull every bookmark for the connected user and persist new
/// ones as memories. Idempotent — re-running this against the
/// same bookmarks dedupes by `external_id = tweet_id`.
pub async fn sync_bookmarks(
    token: &XOAuthRow,
    memory_repo: &SharedMemoryRepository,
) -> AppResult<BookmarkSyncResult> {
    let mut result = BookmarkSyncResult::default();
    let mut next_token: Option<String> = None;

    let client = reqwest::Client::new();
    loop {
        let url_template = X_BOOKMARKS_URL.replace("{id}", &token.x_user_id);
        let mut url = url::Url::parse(&url_template)
            .map_err(|err| AppError::Invalid(format!("invalid bookmarks URL: {err}")))?;
        url.query_pairs_mut()
            .append_pair("max_results", "100")
            .append_pair("expansions", "author_id")
            .append_pair("tweet.fields", "created_at,author_id")
            .append_pair("user.fields", "username,name");
        if let Some(t) = &next_token {
            url.query_pairs_mut().append_pair("pagination_token", t);
        }

        let response = client
            .get(url)
            .bearer_auth(&token.access_token)
            .send()
            .await
            .map_err(|err| AppError::Invalid(format!("X bookmarks fetch failed: {err}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Invalid(format!(
                "X bookmarks returned {status}: {body}"
            )));
        }
        let page: BookmarksResponse = response
            .json()
            .await
            .map_err(|err| AppError::Invalid(format!("X bookmarks parse failed: {err}")))?;

        let users_by_id: std::collections::HashMap<String, &UserInclude> = page
            .includes
            .as_ref()
            .map(|inc| {
                inc.users
                    .iter()
                    .map(|u| (u.id.clone(), u))
                    .collect()
            })
            .unwrap_or_default();

        for tweet in &page.data {
            result.fetched += 1;

            // Dedup: if a memory with this tweet_id already
            // exists for our source_app, skip the create.
            if memory_repo
                .find_by_external_source(TWITTER_SOURCE_APP, &tweet.id)
                .await?
                .is_some()
            {
                result.already_saved += 1;
                continue;
            }

            let author = tweet
                .author_id
                .as_deref()
                .and_then(|id| users_by_id.get(id).copied());
            let author_handle = author
                .map(|u| format!("@{}", u.username))
                .unwrap_or_else(|| "@unknown".to_string());
            let author_name = author
                .and_then(|u| u.name.as_deref())
                .unwrap_or("");

            let canonical_url = if let Some(u) = author {
                format!("https://twitter.com/{}/status/{}", u.username, tweet.id)
            } else {
                format!("https://twitter.com/i/status/{}", tweet.id)
            };

            let title = build_tweet_title(&tweet.text, author_name, &author_handle);
            let content = build_tweet_content(&tweet.text, &author_handle, author_name);
            let created_at = tweet
                .created_at
                .clone()
                .unwrap_or_else(|| Utc::now().to_rfc3339());

            memory_repo
                .create(MemoryInput {
                    source_type: Some(MemorySourceType::Manual),
                    title: Some(title),
                    content,
                    note: None,
                    project_id: None,
                    url: Some(canonical_url),
                    external_id: Some(tweet.id.clone()),
                    folder_path: None,
                    source_app: Some(TWITTER_SOURCE_APP.to_string()),
                    source_window: Some(author_handle),
                    created_at: Some(created_at.clone()),
                    updated_at: Some(created_at),
                })
                .await?;
            result.created += 1;
        }

        next_token = page.meta.and_then(|m| m.next_token);
        if next_token.is_none() {
            break;
        }
        // Tiny pause between pages to be polite under the per-app
        // rate limit (180 req / 15 min on free tier — we'd never
        // come close, but no reason to hammer).
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    Ok(result)
}

/// Convenience: timestamp helper. Used by callers that want to
/// know when a token will expire without having to parse the row.
#[allow(dead_code)]
pub fn parse_expires_at(token: &XOAuthRow) -> Option<SystemTime> {
    let raw = token.expires_at.as_deref()?;
    let parsed = DateTime::parse_from_rfc3339(raw).ok()?;
    Some(parsed.into())
}

fn build_tweet_title(text: &str, author_name: &str, author_handle: &str) -> String {
    let label = if author_name.is_empty() {
        author_handle.to_string()
    } else {
        format!("{} ({})", author_name, author_handle)
    };
    let preview: String = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    if preview.is_empty() {
        format!("Tweet by {label}")
    } else {
        format!("{label} · {preview}")
    }
}

fn build_tweet_content(text: &str, author_handle: &str, author_name: &str) -> String {
    let header = if author_name.is_empty() {
        author_handle.to_string()
    } else {
        format!("{author_name} ({author_handle})")
    };
    format!("{header}\n\n{text}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_rfc7636_example() {
        // Spec example from RFC 7636 Appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let expected = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert_eq!(pkce_challenge(verifier), expected);
    }

    #[test]
    fn parse_callback_query_extracts_code_and_state() {
        let (code, state, error) = parse_callback_query("/callback?code=abc&state=xyz");
        assert_eq!(code.as_deref(), Some("abc"));
        assert_eq!(state.as_deref(), Some("xyz"));
        assert!(error.is_none());
    }

    #[test]
    fn parse_callback_query_extracts_error() {
        let (code, _state, error) = parse_callback_query(
            "/callback?error=access_denied&error_description=user+denied",
        );
        assert!(code.is_none());
        assert_eq!(error.as_deref(), Some("access_denied: user denied"));
    }

    #[test]
    fn build_tweet_title_uses_author_name_and_preview() {
        let title = build_tweet_title("Just shipped Recall v0.5.37 — bookmark sync from X.\n\nMore later.", "Siddhant", "@siddh");
        assert!(title.starts_with("Siddhant (@siddh) · "));
        assert!(title.contains("Just shipped"));
    }
}
