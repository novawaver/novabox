use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};
use url::Url;

use crate::secrets;
use crate::util::now_secs;
use crate::AppState;

pub const REDIRECT_URI: &str = "http://127.0.0.1:47821/oauth/callback";

/// The Google OAuth client for this build. build.rs reads these from `.env` (or the
/// environment) at compile time; a desktop client secret is not confidential by Google's design.
pub const CLIENT_ID: &str = match option_env!("NOVABOX_GOOGLE_CLIENT_ID") {
    Some(value) => value,
    None => "",
};
pub const CLIENT_SECRET: &str = match option_env!("NOVABOX_GOOGLE_CLIENT_SECRET") {
    Some(value) => value,
    None => "",
};
pub const MISSING_CLIENT: &str =
    "This copy of Novabox was built without a Google client ID. Set NOVABOX_GOOGLE_CLIENT_ID in .env and rebuild.";

pub struct CachedToken {
    pub access: String,
    pub expires_at: i64,
}

pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
}

pub struct OauthCode {
    pub code: String,
    pub verifier: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    sub: String,
    email: Option<String>,
    name: Option<String>,
    picture: Option<String>,
}

pub struct Profile {
    pub id: String,
    pub email: String,
    pub name: String,
    pub picture: Option<String>,
}

pub fn browser_sign_in(client_id: &str, cancel: &AtomicBool) -> Result<OauthCode, String> {
    if client_id.trim().is_empty() {
        return Err(MISSING_CLIENT.into());
    }
    let verifier = random_url_safe(64);
    let challenge = s256(&verifier);
    let state = random_url_safe(24);
    let listener = TcpListener::bind(("127.0.0.1", 47821)).map_err(|_| {
        "Port 47821 is already in use. Close whatever else is using it, then try again.".to_string()
    })?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut url = Url::parse("https://accounts.google.com/o/oauth2/v2/auth").map_err(|e| e.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", REDIRECT_URI)
            .append_pair("response_type", "code")
            .append_pair(
                "scope",
                // gmail.modify only: Novabox moves mail to Trash and never deletes it for good,
                // so it does not ask for full Gmail access (https://mail.google.com/).
                "openid email profile https://www.googleapis.com/auth/gmail.modify",
            )
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state)
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent select_account");
    }
    open::that(url.as_str()).map_err(|e| format!("Could not open the browser: {e}"))?;

    let start = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Sign-in was cancelled.".into());
        }
        if start.elapsed() > Duration::from_secs(180) {
            return Err("Sign-in timed out. Try again.".into());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                if let Some(code) = read_callback(&mut stream, &state)? {
                    return Ok(OauthCode { code, verifier });
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(err) => return Err(err.to_string()),
        }
    }
}

pub async fn exchange_code(
    http: &Client,
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
) -> Result<Session, String> {
    let mut form: Vec<(String, String)> = vec![
        ("client_id".into(), client_id.to_string()),
        ("code".into(), code.to_string()),
        ("code_verifier".into(), verifier.to_string()),
        ("grant_type".into(), "authorization_code".into()),
        ("redirect_uri".into(), REDIRECT_URI.into()),
    ];
    if !client_secret.is_empty() {
        form.push(("client_secret".into(), client_secret.to_string()));
    }
    token_request(http, &form).await
}

pub async fn refresh_access(
    http: &Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<Session, String> {
    let mut form: Vec<(String, String)> = vec![
        ("client_id".into(), client_id.to_string()),
        ("refresh_token".into(), refresh_token.to_string()),
        ("grant_type".into(), "refresh_token".into()),
    ];
    if !client_secret.is_empty() {
        form.push(("client_secret".into(), client_secret.to_string()));
    }
    let mut session = token_request(http, &form).await?;
    if session.refresh_token.is_empty() {
        session.refresh_token = refresh_token.to_string();
    }
    Ok(session)
}

pub async fn fetch_profile(http: &Client, access_token: &str) -> Result<Profile, String> {
    let response = http
        .get("https://www.googleapis.com/oauth2/v3/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Could not reach Google: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Could not read the Google profile: {status}"));
    }
    let info: UserInfo = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let email = info.email.unwrap_or_default();
    if email.is_empty() {
        return Err("Google did not share an email address for this account.".into());
    }
    Ok(Profile {
        id: info.sub,
        email: email.clone(),
        name: info.name.unwrap_or(email),
        picture: info.picture,
    })
}

pub async fn revoke(http: &Client, token: &str) -> Result<(), String> {
    let _ = http
        .post("https://oauth2.googleapis.com/revoke")
        .form(&[("token", token)])
        .send()
        .await;
    Ok(())
}

pub async fn ensure_access(app: &AppHandle, account_id: &str) -> Result<String, String> {
    let now = now_secs();
    {
        let state = app.state::<AppState>();
        let tokens = state.tokens.lock().map_err(|e| e.to_string())?;
        if let Some(token) = tokens.get(account_id) {
            if token.expires_at > now + 30 {
                return Ok(token.access.clone());
            }
        }
    }
    let id = account_id.to_string();
    let refresh = tauri::async_runtime::spawn_blocking(move || secrets::load(&id))
        .await
        .map_err(|e| e.to_string())??;
    let http = app.state::<AppState>().http.clone();
    let session = refresh_access(&http, CLIENT_ID, CLIENT_SECRET, &refresh).await?;
    if session.refresh_token != refresh {
        let id = account_id.to_string();
        let token = session.refresh_token.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || secrets::save(&id, &token)).await;
    }
    store_token(app, account_id, &session);
    Ok(session.access_token)
}

pub fn store_token(app: &AppHandle, account_id: &str, session: &Session) {
    if let Ok(mut tokens) = app.state::<AppState>().tokens.lock() {
        tokens.insert(
            account_id.to_string(),
            CachedToken {
                access: session.access_token.clone(),
                expires_at: session.expires_at,
            },
        );
    }
}

pub fn invalidate(app: &AppHandle, account_id: &str) {
    if let Ok(mut tokens) = app.state::<AppState>().tokens.lock() {
        tokens.remove(account_id);
    }
}

async fn token_request(http: &Client, form: &[(String, String)]) -> Result<Session, String> {
    let pairs: Vec<(&str, &str)> = form.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let response = http
        .post("https://oauth2.googleapis.com/token")
        .form(&pairs)
        .send()
        .await
        .map_err(|e| format!("Could not reach Google: {e}"))?;
    let text = response.text().await.unwrap_or_default();
    let parsed: TokenResponse = serde_json::from_str(&text).map_err(|_| {
        "Google returned an unexpected sign-in response.".to_string()
    })?;
    if let Some(error) = parsed.error {
        let detail = parsed.error_description.unwrap_or(error);
        return Err(format!("Google sign-in failed: {detail}"));
    }
    let access = parsed
        .access_token
        .ok_or("Google did not return an access token.")?;
    Ok(Session {
        access_token: access,
        refresh_token: parsed.refresh_token.unwrap_or_default(),
        expires_at: now_secs() + parsed.expires_in.unwrap_or(3600) - 60,
    })
}

fn read_callback(stream: &mut TcpStream, expected_state: &str) -> Result<Option<String>, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();
    let mut buf = [0u8; 8192];
    let size = stream.read(&mut buf).unwrap_or(0);
    let text = String::from_utf8_lossy(&buf[..size]);
    let path = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let fake = Url::parse(&format!("http://127.0.0.1{path}")).map_err(|e| e.to_string())?;
    let code = query_value(&fake, "code");
    let state = query_value(&fake, "state");
    let error = query_value(&fake, "error");
    if path.starts_with("/oauth/callback") {
        if let Some(error) = error {
            respond(stream, "Novabox sign-in was cancelled. You can close this window.");
            if error == "access_denied" {
                return Err("Google sign-in was cancelled.".into());
            }
            return Err(format!("Google sign-in failed: {error}"));
        }
        if state.as_deref() != Some(expected_state) {
            respond(stream, "Novabox could not verify that sign-in. You can close this window.");
            return Err("Sign-in could not be verified. Try again.".into());
        }
        if let Some(code) = code {
            respond(stream, "Novabox is connected. You can close this window.");
            return Ok(Some(code));
        }
    }
    respond(stream, "Waiting for Google. You can close this window after sign-in finishes.");
    Ok(None)
}

fn query_value(url: &Url, key: &str) -> Option<String> {
    url.query_pairs()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.into_owned())
}

fn respond(stream: &mut TcpStream, message: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Novabox</title></head><body style=\"font-family:Segoe UI,sans-serif;background:#f6f3ec;color:#1e1a15;display:grid;place-items:center;height:100vh;margin:0\"><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn random_url_safe(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
