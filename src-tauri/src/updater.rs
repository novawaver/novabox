//! Finding and installing newer Novabox releases.
//!
//! The feed is a `latest.json` published with every release by `scripts/build-installer.ps1`
//! or the release workflow. Its URL is `plugins.updater.endpoints` in tauri.conf.json; a build
//! can point somewhere else with `NOVABOX_UPDATE_URL` in `.env` (see build.rs). A downloaded
//! installer must verify against the public key in that config before it runs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

/// Build-time override of the feed URL, from `.env`.
const UPDATE_URL: Option<&str> = option_env!("NOVABOX_UPDATE_URL");

/// Sent to the window when a background check finds a release. Payload: [`UpdateInfo`].
pub const EVENT_AVAILABLE: &str = "update://available";
/// Sent while an update downloads and installs. Payload: [`UpdateProgress`].
pub const EVENT_PROGRESS: &str = "update://progress";

/// The first check waits this long after start-up, so it never competes with the first mail sync.
const FIRST_CHECK: Duration = Duration::from_secs(45);
/// Later checks, while Novabox stays open (it usually lives in the tray for days).
const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Default)]
pub struct UpdateState {
    /// The release the last check found, so Install does not have to fetch the feed again.
    pending: Mutex<Option<Update>>,
    installing: AtomicBool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release notes from the feed, when it has any.
    pub notes: Option<String>,
    /// Release time as a Unix timestamp in seconds, when the feed has one.
    pub date_unix: Option<i64>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    /// `downloading` while the installer streams in, then `installing` while it runs.
    pub phase: &'static str,
    pub downloaded: u64,
    /// Installer size, when the server sent one.
    pub total: Option<u64>,
}

fn info_of(update: &Update) -> UpdateInfo {
    UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone().filter(|notes| !notes.trim().is_empty()),
        date_unix: update.date.map(|date| date.unix_timestamp()),
    }
}

fn friendly(err: UpdaterError) -> String {
    match err {
        UpdaterError::ReleaseNotFound => {
            "No update feed was found. The first release may not be published yet.".into()
        }
        UpdaterError::Reqwest(err) if err.is_connect() || err.is_timeout() => {
            "Could not reach the update server. Check your connection and try again.".into()
        }
        UpdaterError::Minisign(_) => {
            "The downloaded installer failed its signature check and was not run.".into()
        }
        other => format!("Update failed: {other}"),
    }
}

/// Asks the feed for a release newer than this build.
async fn fetch(app: &AppHandle) -> Result<Option<Update>, String> {
    let mut builder = app.updater_builder();
    if let Some(url) = UPDATE_URL.map(str::trim).filter(|url| !url.is_empty()) {
        let url = url
            .parse()
            .map_err(|err| format!("NOVABOX_UPDATE_URL is not a valid URL: {err}"))?;
        builder = builder.endpoints(vec![url]).map_err(friendly)?;
    }
    let updater = builder.build().map_err(friendly)?;
    updater.check().await.map_err(friendly)
}

fn remember(app: &AppHandle, update: Option<Update>) {
    if let Ok(mut pending) = app.state::<UpdateState>().pending.lock() {
        *pending = update;
    }
}

/// Looks for a newer release. `None` means this build is current.
#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = fetch(&app).await?;
    let info = update.as_ref().map(info_of);
    remember(&app, update);
    Ok(info)
}

/// Downloads and installs the release found by the last check (or checks first), then
/// restarts Novabox. Progress arrives as [`EVENT_PROGRESS`]. On Windows the installer runs
/// in passive mode (a progress bar, no questions) and starts the new version when it is done.
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UpdateState>();
    if state.installing.swap(true, Ordering::SeqCst) {
        return Err("An update is already installing.".into());
    }
    let result = install(&app).await;
    state.installing.store(false, Ordering::SeqCst);
    result?;
    // Reached on platforms where the updater does not exit the process itself.
    app.restart()
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let known = app
        .state::<UpdateState>()
        .pending
        .lock()
        .map_err(|_| "Update state is unavailable.".to_string())?
        .clone();
    let update = match known {
        Some(update) => update,
        None => fetch(app)
            .await?
            .ok_or_else(|| "Novabox is already up to date.".to_string())?,
    };
    let mut downloaded = 0u64;
    let on_chunk = {
        let app = app.clone();
        move |chunk: usize, total: Option<u64>| {
            downloaded += chunk as u64;
            let _ = app.emit(
                EVENT_PROGRESS,
                UpdateProgress { phase: "downloading", downloaded, total },
            );
        }
    };
    let on_downloaded = {
        let app = app.clone();
        move || {
            let _ = app.emit(
                EVENT_PROGRESS,
                UpdateProgress { phase: "installing", downloaded: 0, total: None },
            );
        }
    };
    update
        .download_and_install(on_chunk, on_downloaded)
        .await
        .map_err(friendly)
}

/// Checks shortly after start-up and twice a day after that. A release found this way is
/// kept for Install and announced to the window with [`EVENT_AVAILABLE`]. Dev builds skip
/// this: their version is never a published one.
pub fn start_background_checks(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        let mut wait = FIRST_CHECK;
        loop {
            std::thread::sleep(wait);
            wait = CHECK_EVERY;
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                match fetch(&app).await {
                    Ok(Some(update)) => {
                        let info = info_of(&update);
                        remember(&app, Some(update));
                        let _ = app.emit(EVENT_AVAILABLE, info);
                    }
                    Ok(None) => {}
                    Err(err) => eprintln!("Novabox update check: {err}"),
                }
            });
        }
    });
}
