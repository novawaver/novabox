use keyring::Entry;

const SERVICE: &str = "com.novawave.novabox";

fn entry(account_id: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, account_id).map_err(|e| format!("Windows Credential Manager: {e}"))
}

/// Stores an account's secret: a Google refresh token, or the password of an IMAP account.
pub fn save(account_id: &str, secret: &str) -> Result<(), String> {
    entry(account_id)?
        .set_password(secret)
        .map_err(|e| format!("Could not store the sign-in in Windows Credential Manager: {e}"))
}

pub fn load(account_id: &str) -> Result<String, String> {
    entry(account_id)?
        .get_password()
        .map_err(|_| "This account needs to be signed in again.".to_string())
}

pub fn delete(account_id: &str) -> Result<(), String> {
    match entry(account_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(err) => {
            let text = err.to_string().to_lowercase();
            if text.contains("no entry")
                || text.contains("not found")
                || text.contains("cannot find")
            {
                Ok(())
            } else {
                Err(format!("Could not remove the saved sign-in: {err}"))
            }
        }
    }
}
