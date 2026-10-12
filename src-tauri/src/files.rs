use std::fs;
use std::path::{Path, PathBuf};

use crate::util::safe_id;

pub fn account_dir(data_dir: &Path, account_id: &str) -> PathBuf {
    data_dir.join("files").join(safe_id(account_id))
}

pub fn attachment_path(
    data_dir: &Path,
    account_id: &str,
    message_id: &str,
    attachment_id: &str,
) -> PathBuf {
    account_dir(data_dir, account_id)
        .join(safe_id(message_id))
        .join(safe_id(attachment_id))
}

pub fn write_cached(
    data_dir: &Path,
    account_id: &str,
    message_id: &str,
    attachment_id: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let path = attachment_path(data_dir, account_id, message_id, attachment_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, bytes).map_err(|e| e.to_string())
}

pub fn read_cached(
    data_dir: &Path,
    account_id: &str,
    message_id: &str,
    attachment_id: &str,
) -> Option<Vec<u8>> {
    fs::read(attachment_path(
        data_dir,
        account_id,
        message_id,
        attachment_id,
    ))
    .ok()
}

pub fn remove_account_files(data_dir: &Path, account_id: &str) {
    let _ = fs::remove_dir_all(account_dir(data_dir, account_id));
}
