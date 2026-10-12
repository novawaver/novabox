use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::Utc;
use rand::RngCore;

use crate::model::OutgoingAttachment;

pub struct OutgoingMail<'a> {
    pub from_name: &'a str,
    pub from_email: &'a str,
    pub to: &'a [String],
    pub cc: &'a [String],
    pub bcc: &'a [String],
    pub subject: &'a str,
    pub html: &'a str,
    pub attachments: &'a [OutgoingAttachment],
    pub attachment_bytes: &'a [Vec<u8>],
    pub in_reply_to: Option<&'a str>,
    pub references: Option<&'a str>,
}

pub fn split_emails(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for part in input.split([',', ';']) {
        let email = part.trim();
        if email.is_empty() {
            continue;
        }
        if !is_email(email) {
            return Err(format!("Invalid email address: {email}"));
        }
        out.push(email.to_string());
    }
    Ok(out)
}

fn is_email(value: &str) -> bool {
    if value.contains([' ', '\r', '\n', '<', '>', '"']) {
        return false;
    }
    let mut parts = value.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        _ => false,
    }
}

pub fn build_rfc822(mail: &OutgoingMail) -> Result<Vec<u8>, String> {
    if mail.to.is_empty() && mail.cc.is_empty() && mail.bcc.is_empty() {
        return Err("Add at least one recipient.".into());
    }
    if !is_email(mail.from_email) {
        return Err("The From address is not valid.".into());
    }
    let mut total = mail.html.len();
    for bytes in mail.attachment_bytes {
        total += bytes.len();
    }
    if total > 24 * 1024 * 1024 {
        return Err("This message is over Gmail's 25 MB limit.".into());
    }

    let alt = boundary();
    let mixed = boundary();
    let has_files = !mail.attachment_bytes.is_empty();
    let mut out = String::new();
    out.push_str(&format!(
        "From: {}\r\n",
        format_addr(mail.from_name, mail.from_email)
    ));
    if !mail.to.is_empty() {
        out.push_str(&format!("To: {}\r\n", join_addrs(mail.to)));
    }
    if !mail.cc.is_empty() {
        out.push_str(&format!("Cc: {}\r\n", join_addrs(mail.cc)));
    }
    if !mail.bcc.is_empty() {
        out.push_str(&format!("Bcc: {}\r\n", join_addrs(mail.bcc)));
    }
    out.push_str(&format!("Subject: {}\r\n", encode_header(mail.subject)));
    out.push_str(&format!("Date: {}\r\n", Utc::now().to_rfc2822()));
    out.push_str(&format!("Message-ID: <{}>\r\n", message_id()));
    if let Some(reply) = mail.in_reply_to.filter(|v| !v.is_empty()) {
        out.push_str(&format!("In-Reply-To: {}\r\n", header_safe(reply)));
    }
    if let Some(refs) = mail.references.filter(|v| !v.is_empty()) {
        out.push_str(&format!("References: {}\r\n", header_safe(refs)));
    }
    out.push_str("MIME-Version: 1.0\r\n");
    if has_files {
        out.push_str(&format!(
            "Content-Type: multipart/mixed; boundary=\"{mixed}\"\r\n\r\n"
        ));
        out.push_str(&format!("--{mixed}\r\n"));
    }
    out.push_str(&format!(
        "Content-Type: multipart/alternative; boundary=\"{alt}\"\r\n\r\n"
    ));
    let plain = html_to_text(mail.html);
    push_text_part(&mut out, &alt, "text/plain", &plain);
    push_text_part(&mut out, &alt, "text/html", mail.html);
    out.push_str(&format!("--{alt}--\r\n"));
    if has_files {
        for (index, bytes) in mail.attachment_bytes.iter().enumerate() {
            let meta = mail
                .attachments
                .get(index)
                .map(|item| (item.filename.as_str(), item.mime_type.as_str()))
                .unwrap_or(("attachment", "application/octet-stream"));
            out.push_str(&format!("--{mixed}\r\n"));
            out.push_str(&format!(
                "Content-Type: {}; name=\"{}\"\r\n",
                header_safe(meta.1),
                header_safe(meta.0)
            ));
            out.push_str(&format!(
                "Content-Disposition: attachment; filename=\"{}\"\r\n",
                header_safe(meta.0)
            ));
            out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
            out.push_str(&wrap_base64(&STANDARD.encode(bytes)));
            out.push_str("\r\n");
        }
        out.push_str(&format!("--{mixed}--\r\n"));
    }
    Ok(out.into_bytes())
}

pub fn to_gmail_raw(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn push_text_part(out: &mut String, boundary: &str, mime: &str, body: &str) {
    out.push_str(&format!("--{boundary}\r\n"));
    out.push_str(&format!("Content-Type: {mime}; charset=\"UTF-8\"\r\n"));
    out.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    out.push_str(&wrap_base64(&STANDARD.encode(body.as_bytes())));
    out.push_str("\r\n");
}

fn format_addr(name: &str, email: &str) -> String {
    let name = header_safe(name);
    if name.is_empty() {
        email.to_string()
    } else {
        format!("{} <{email}>", encode_header(&name))
    }
}

fn join_addrs(emails: &[String]) -> String {
    emails
        .iter()
        .map(|email| header_safe(email))
        .collect::<Vec<_>>()
        .join(", ")
}

fn header_safe(value: &str) -> String {
    value.replace(['\r', '\n'], "")
}

fn encode_header(value: &str) -> String {
    let clean = header_safe(value);
    if clean.is_ascii() {
        clean
    } else {
        format!("=?UTF-8?B?{}?=", STANDARD.encode(clean.as_bytes()))
    }
}

fn html_to_text(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn wrap_base64(value: &str) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index > 0 && index % 76 == 0 {
            out.push_str("\r\n");
        }
        out.push(ch);
    }
    out
}

fn boundary() -> String {
    let mut bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("novabox_{}", hex(&bytes))
}

fn message_id() -> String {
    let mut bytes = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{}.{}@novabox.local", crate::util::now_secs(), hex(&bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_header_injection_and_builds_alternative() {
        let mail = OutgoingMail {
            from_name: "Maya\r\nBcc: evil@example.com",
            from_email: "maya@northmail.test",
            to: &["ada@harbor.test".into()],
            cc: &[],
            bcc: &[],
            subject: "Hello\r\nBcc: evil@example.com",
            html: "<p>Hello <b>there</b></p>",
            attachments: &[],
            attachment_bytes: &[],
            in_reply_to: Some("<abc@mail.test>"),
            references: None,
        };
        let raw = String::from_utf8(build_rfc822(&mail).unwrap()).unwrap();
        assert!(raw.contains("Subject: HelloBcc: evil@example.com"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("In-Reply-To: <abc@mail.test>"));
        assert!(!raw.contains("\r\nBcc: evil@example.com"));
        assert!(split_emails("a@b.co, c@d.co").unwrap().len() == 2);
        assert!(split_emails("not-an-email").is_err());
    }
}
