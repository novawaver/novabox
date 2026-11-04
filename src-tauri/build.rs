use std::fs;
use std::path::Path;

/// Settings baked into the executable: the Google OAuth client and an optional update feed
/// override. Values come from the environment, or from a `.env` file in the project root or
/// in `src-tauri/` (environment wins).
const BUILD_ENV: [&str; 3] = ["NOVABOX_GOOGLE_CLIENT_ID", "NOVABOX_GOOGLE_CLIENT_SECRET", "NOVABOX_UPDATE_URL"];

fn main() {
    // The Windows title bar, taskbar, and tray icon are loaded from icon.ico
    // inside the executable. Cargo does not rebuild that resource when only
    // the icon file changes, so watch it explicitly.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=icons/icon.png");
    println!("cargo:rerun-if-changed=icons/32x32.png");
    println!("cargo:rerun-if-changed=icons/128x128.png");
    println!("cargo:rerun-if-changed=icons/128x128@2x.png");
    println!("cargo:rerun-if-changed=icons/icon.icns");
    load_build_env();
    tauri_build::build()
}

fn load_build_env() {
    let files = ["../.env", ".env"];
    // Cargo treats a missing rerun-if-changed path as always stale. Watch only the files
    // that exist; while none does, watch both so creating .env is picked up next build.
    let existing: Vec<&str> = files.iter().copied().filter(|file| Path::new(file).exists()).collect();
    for file in if existing.is_empty() { &files[..] } else { &existing[..] } {
        println!("cargo:rerun-if-changed={file}");
    }
    for key in BUILD_ENV {
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| files.iter().find_map(|file| read_dotenv(Path::new(file), key)));
        match value {
            Some(value) => println!("cargo:rustc-env={key}={}", value.trim()),
            None if key == "NOVABOX_GOOGLE_CLIENT_ID" => {
                println!("cargo:warning={key} is not set; Google sign-in will be unavailable in this build. Add it to .env.")
            }
            None => {}
        }
    }
}

/// Reads one `KEY=value` line from a dotenv file. Supports comments, `export`, and quotes.
fn read_dotenv(path: &Path, key: &str) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            println!("cargo:warning=Could not read {} as UTF-8 ({err}). Save it as UTF-8.", path.display());
            return None;
        }
    };
    // Some Windows tools write a byte-order mark, which would hide the first key.
    text.trim_start_matches('\u{FEFF}').lines().find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let (name, value) = line.split_once('=')?;
        if name.trim() != key {
            return None;
        }
        let value = value.trim();
        let quoted = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')));
        // Unquoted values may carry a trailing ` # comment`.
        let value = match quoted {
            Some(inner) => inner,
            None => value.split(" #").next().unwrap_or(value).split("\t#").next().unwrap_or(value).trim(),
        };
        (!value.is_empty()).then(|| value.to_string())
    })
}
