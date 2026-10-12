//! Password-protected backup of account sign-ins, so a new install can pick up the same
//! Gmail accounts without going through Google sign-in again.
//!
//! The file is JSON: a small header plus the account list encrypted with AES-256-GCM under
//! a key derived from the password with Argon2id. Mail itself is not included; it syncs again.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::auth;
use crate::commands::{register_account, with_db};
use crate::db;
use crate::model::{Account, SendAs};
use crate::secrets;
use crate::AppState;

const FORMAT: &str = "novabox-sign-ins";
const VERSION: u32 = 1;
const MIN_PASSWORD: usize = 8;
// OWASP's suggested Argon2id floor: 19 MiB, 2 passes.
const MEMORY_KIB: u32 = 19_456;
const ITERATIONS: u32 = 2;

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    kdf: Kdf,
    nonce: String,
    ciphertext: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kdf {
    algorithm: String,
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
    salt: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    created_at: i64,
    /// Short fingerprint of the Google client ID the tokens were issued to.
    client: String,
    accounts: Vec<BackupAccount>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupAccount {
    id: String,
    email: String,
    name: String,
    picture: Option<String>,
    color: String,
    signature: String,
    send_as: Vec<SendAs>,
    refresh_token: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    exported: Vec<String>,
    skipped: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    email: String,
    /// "added", "updated" or "failed".
    status: String,
    message: Option<String>,
}

fn client_fingerprint() -> String {
    Sha256::digest(auth::CLIENT_ID.as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn derive_key(password: &str, salt: &[u8], memory_kib: u32, iterations: u32, parallelism: u32) -> Result<[u8; 32], String> {
    let params = Params::new(memory_kib, iterations, parallelism, Some(32)).map_err(|e| e.to_string())?;
    let mut key = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| e.to_string())?;
    Ok(key)
}

fn encrypt(payload: &Payload, password: &str) -> Result<Envelope, String> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = derive_key(password, &salt, MEMORY_KIB, ITERATIONS, 1)?;
    let plain = serde_json::to_vec(payload).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce), plain.as_slice())
        .map_err(|_| "Could not encrypt the backup.".to_string())?;
    Ok(Envelope {
        format: FORMAT.into(),
        version: VERSION,
        kdf: Kdf {
            algorithm: "argon2id".into(),
            memory_kib: MEMORY_KIB,
            iterations: ITERATIONS,
            parallelism: 1,
            salt: STANDARD.encode(salt),
        },
        nonce: STANDARD.encode(nonce),
        ciphertext: STANDARD.encode(sealed),
    })
}

fn decrypt(envelope: &Envelope, password: &str) -> Result<Payload, String> {
    if envelope.format != FORMAT || envelope.kdf.algorithm != "argon2id" {
        return Err("This is not a Novabox sign-in backup.".into());
    }
    if envelope.version > VERSION {
        return Err("This backup was made by a newer version of Novabox. Update Novabox and try again.".into());
    }
    let kdf = &envelope.kdf;
    // Refuse absurd settings from a tampered file instead of hanging the app.
    if kdf.memory_kib > 1 << 20 || kdf.iterations > 16 || kdf.parallelism == 0 || kdf.parallelism > 16 {
        return Err("This backup file is damaged.".into());
    }
    let damaged = |_| "This backup file is damaged.".to_string();
    let salt = STANDARD.decode(&kdf.salt).map_err(damaged)?;
    let nonce = STANDARD.decode(&envelope.nonce).map_err(damaged)?;
    let sealed = STANDARD.decode(&envelope.ciphertext).map_err(damaged)?;
    if nonce.len() != 12 {
        return Err("This backup file is damaged.".into());
    }
    let key = derive_key(password, &salt, kdf.memory_kib, kdf.iterations, kdf.parallelism)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| e.to_string())?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce), sealed.as_slice())
        .map_err(|_| "Wrong password, or the backup file is damaged.".to_string())?;
    serde_json::from_slice(&plain).map_err(|_| "This backup file is damaged.".to_string())
}

#[tauri::command]
pub async fn backup_export(app: AppHandle, path: String, password: String) -> Result<ExportReport, String> {
    if password.chars().count() < MIN_PASSWORD {
        return Err(format!("Use a password of at least {MIN_PASSWORD} characters."));
    }
    let accounts = with_db(&app, |conn| db::list_accounts(conn))?;
    let mut exported = Vec::new();
    let mut skipped = Vec::new();
    let mut items = Vec::new();
    for account in accounts {
        let id = account.id.clone();
        let token = tauri::async_runtime::spawn_blocking(move || secrets::load(&id))
            .await
            .map_err(|e| e.to_string())?;
        match token {
            Ok(refresh_token) => {
                exported.push(account.email.clone());
                items.push(BackupAccount {
                    id: account.id,
                    email: account.email,
                    name: account.name,
                    picture: account.picture,
                    color: account.color,
                    signature: account.signature,
                    send_as: account.send_as,
                    refresh_token,
                });
            }
            Err(_) => skipped.push(account.email),
        }
    }
    if items.is_empty() {
        return Err("No signed-in accounts to export. Sign in to an account first.".into());
    }
    let payload = Payload { created_at: crate::util::now_secs(), client: client_fingerprint(), accounts: items };
    let envelope = tauri::async_runtime::spawn_blocking(move || encrypt(&payload, &password))
        .await
        .map_err(|e| e.to_string())??;
    let text = serde_json::to_string_pretty(&envelope).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("Could not save the backup: {e}"))?;
    Ok(ExportReport { exported, skipped })
}

#[tauri::command]
pub async fn backup_import(app: AppHandle, path: String, password: String) -> Result<Vec<ImportItem>, String> {
    if auth::CLIENT_ID.is_empty() {
        return Err(auth::MISSING_CLIENT.into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("Could not open the backup: {e}"))?;
    let envelope: Envelope = serde_json::from_str(&text).map_err(|_| "This is not a Novabox sign-in backup.".to_string())?;
    let payload = tauri::async_runtime::spawn_blocking(move || decrypt(&envelope, &password))
        .await
        .map_err(|e| e.to_string())??;
    if payload.client != client_fingerprint() {
        return Err("This backup was made with a different Google client ID, so its sign-ins can't be used with this copy of Novabox. Sign in with Google instead.".into());
    }
    let http = app.state::<AppState>().http.clone();
    let mut report = Vec::new();
    for item in payload.accounts {
        // Prove the sign-in still works before keeping it; Google may have revoked it.
        let session = match auth::refresh_access(&http, auth::CLIENT_ID, auth::CLIENT_SECRET, &item.refresh_token).await {
            Ok(session) => session,
            Err(err) => {
                // token_request prefixes Google's own refusals (invalid_grant and the like);
                // anything else is the network or a Google hiccup, and the backup is still good.
                let message = if err.starts_with("Google sign-in failed") {
                    format!("Google no longer accepts this sign-in ({err}). Sign in again with the + button.")
                } else {
                    format!("Couldn't check this sign-in with Google ({err}). Check your connection and import the backup again.")
                };
                report.push(ImportItem { email: item.email, status: "failed".into(), message: Some(message) });
                continue;
            }
        };
        let existed = with_db(&app, |conn| Ok(db::get_account(conn, &item.id).is_ok()))?;
        let id = item.id.clone();
        let refresh = session.refresh_token.clone();
        let saved = tauri::async_runtime::spawn_blocking(move || secrets::save(&id, &refresh))
            .await
            .map_err(|e| e.to_string())?;
        if let Err(err) = saved {
            report.push(ImportItem { email: item.email, status: "failed".into(), message: Some(err) });
            continue;
        }
        auth::store_token(&app, &item.id, &session);
        let account = Account {
            id: item.id,
            email: item.email.clone(),
            name: item.name,
            picture: item.picture,
            color: item.color,
            unread: 0,
            signature: item.signature,
            send_as: item.send_as,
            history_id: None,
            recent_page_token: None,
            backfill_token: None,
            backfill_done: false,
            initial_done: false,
        };
        match register_account(&app, account).await {
            Ok(_) => report.push(ImportItem {
                email: item.email,
                status: if existed { "updated".into() } else { "added".into() },
                message: None,
            }),
            Err(err) => report.push(ImportItem { email: item.email, status: "failed".into(), message: Some(err) }),
        }
    }
    crate::refresh_badge(&app);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Payload {
        Payload {
            created_at: 1,
            client: client_fingerprint(),
            accounts: vec![BackupAccount {
                id: "1".into(),
                email: "a@example.com".into(),
                name: "A".into(),
                picture: None,
                color: "#C9842A".into(),
                signature: String::new(),
                send_as: Vec::new(),
                refresh_token: "secret-token".into(),
            }],
        }
    }

    #[test]
    fn round_trips_with_the_right_password() {
        let envelope = encrypt(&sample(), "correct horse").unwrap();
        let text = serde_json::to_string(&envelope).unwrap();
        assert!(!text.contains("secret-token"));
        let back = decrypt(&serde_json::from_str(&text).unwrap(), "correct horse").unwrap();
        assert_eq!(back.accounts[0].refresh_token, "secret-token");
    }

    #[test]
    fn rejects_a_wrong_password() {
        let envelope = encrypt(&sample(), "correct horse").unwrap();
        let err = decrypt(&envelope, "wrong horse").err().unwrap();
        assert!(err.contains("Wrong password"));
    }

    #[test]
    fn rejects_tampered_ciphertext() {
        let mut envelope = encrypt(&sample(), "correct horse").unwrap();
        let mut bytes = STANDARD.decode(&envelope.ciphertext).unwrap();
        bytes[0] ^= 1;
        envelope.ciphertext = STANDARD.encode(bytes);
        assert!(decrypt(&envelope, "correct horse").is_err());
    }
}
