use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn redact(input: &str) -> String {
    let mut out = input.to_string();
    while let Some(index) = out.find("ya29.") {
        let end = (index + 16).min(out.len());
        out.replace_range(index..end, "ya29.redacted");
    }
    if out.len() > 500 {
        out.truncate(500);
    }
    out
}

pub fn safe_id(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(100)
        .collect()
}

pub fn validate_label(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 80 {
        return Err("That label id is not valid.".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("That label id is not valid.".into());
    }
    Ok(())
}
