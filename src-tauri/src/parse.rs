use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use encoding_rs::Encoding;
use serde_json::Value;

use crate::model::{Address, AttachmentMeta, StoredMessage};

pub struct CachedFile {
    pub attachment_id: String,
    pub bytes: Vec<u8>,
}

pub struct ParsedMessage {
    pub message: StoredMessage,
    pub files: Vec<CachedFile>,
}

pub fn parse_message(account_id: &str, value: &Value) -> Result<ParsedMessage, String> {
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Gmail message is missing an id")?
        .to_string();
    let thread_id = value
        .get("threadId")
        .and_then(|v| v.as_str())
        .unwrap_or(&id)
        .to_string();
    let payload = value.get("payload").cloned().unwrap_or(Value::Null);
    let from = parse_address_list(&header(&payload, "From"));
    let (from_name, from_email) = from
        .first()
        .map(|addr| (plain(&addr.name), plain(&addr.email)))
        .unwrap_or_else(|| (String::new(), String::new()));
    let mut parsed = BodyParse::default();
    let mut index = 0;
    walk(&payload, &mut parsed, &mut index);
    if parsed.html.len() > 1_500_000 {
        parsed.html.truncate(1_500_000);
    }
    if parsed.text.len() > 1_500_000 {
        parsed.text.truncate(1_500_000);
    }
    let snippet = plain(
        value
            .get("snippet")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
    );
    if parsed.html.is_empty() && parsed.text.is_empty() && !snippet.is_empty() {
        parsed.text = snippet.clone();
    }
    let date = value
        .get("internalDate")
        .and_then(|v| v.as_str())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let label_ids = value
        .get("labelIds")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let size_estimate = value.get("sizeEstimate").and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(ParsedMessage {
        message: StoredMessage {
            account_id: account_id.to_string(),
            id,
            thread_id,
            from_name,
            from_email,
            to: parse_address_list(&header(&payload, "To")),
            cc: parse_address_list(&header(&payload, "Cc")),
            bcc: parse_address_list(&header(&payload, "Bcc")),
            subject: plain(&header(&payload, "Subject")),
            snippet,
            body_html: parsed.html,
            body_text: parsed.text,
            date,
            label_ids,
            attachments: parsed.attachments,
            message_id_header: header(&payload, "Message-ID"),
            references_header: header(&payload, "References"),
            draft_id: None,
            size_estimate,
        },
        files: parsed.files,
    })
}

#[derive(Default)]
struct BodyParse {
    html: String,
    text: String,
    attachments: Vec<AttachmentMeta>,
    files: Vec<CachedFile>,
}

fn walk(part: &Value, out: &mut BodyParse, index: &mut i32) {
    let mime = part
        .get("mimeType")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let filename = part
        .get("filename")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let body = part.get("body").cloned().unwrap_or(Value::Null);
    let attachment_id = body
        .get("attachmentId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let size = body.get("size").and_then(|v| v.as_i64()).unwrap_or(0);
    let data = body.get("data").and_then(|v| v.as_str());
    if !mime.starts_with("multipart/") {
        let is_body = filename.is_empty()
            && attachment_id.is_none()
            && (mime == "text/html" || mime == "text/plain" || mime.is_empty());
        if is_body {
            if let Some(data) = data {
                let text = decode_text(&decode_b64(data), &charset_of(part));
                if mime == "text/html" {
                    if text.len() > out.html.len() {
                        out.html = text;
                    }
                } else if text.len() > out.text.len() {
                    out.text = text;
                }
            }
        } else if !filename.is_empty() || attachment_id.is_some() {
            *index += 1;
            let id = attachment_id.unwrap_or_else(|| format!("inline-{index}"));
            let name = if filename.is_empty() {
                format!("attachment-{index}")
            } else {
                filename
            };
            if let Some(data) = data {
                let bytes = decode_b64(data);
                if !bytes.is_empty() && bytes.len() <= 1_000_000 {
                    out.files.push(CachedFile {
                        attachment_id: id.clone(),
                        bytes,
                    });
                }
            }
            out.attachments.push(AttachmentMeta {
                id,
                filename: name,
                mime_type: if mime.is_empty() {
                    "application/octet-stream".into()
                } else {
                    mime.clone()
                },
                size,
            });
        }
    }
    if let Some(parts) = part.get("parts").and_then(|v| v.as_array()) {
        for child in parts {
            walk(child, out, index);
        }
    }
}

fn header(payload: &Value, name: &str) -> String {
    payload
        .get("headers")
        .and_then(|v| v.as_array())
        .and_then(|headers| {
            headers.iter().find(|header| {
                header
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|value| value.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            })
        })
        .and_then(|header| header.get("value").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

fn charset_of(part: &Value) -> String {
    let content_type = header(part, "Content-Type");
    for piece in content_type.split(';') {
        let piece = piece.trim();
        if let Some(value) = piece
            .strip_prefix("charset=")
            .or_else(|| piece.strip_prefix("charset ="))
        {
            return value.trim_matches('"').to_string();
        }
    }
    "utf-8".into()
}

fn decode_b64(data: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD
        .decode(data)
        .or_else(|_| URL_SAFE.decode(data))
        .unwrap_or_default()
}

fn decode_text(bytes: &[u8], charset: &str) -> String {
    let encoding = Encoding::for_label(charset.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    plain(&text)
}

fn plain(value: &str) -> String {
    value.replace('\0', "")
}

pub fn parse_address_list(input: &str) -> Vec<Address> {
    split_address_list(input)
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| parse_one_address(&part))
        .collect()
}

fn split_address_list(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut angle = 0;
    for ch in input.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                current.push(ch);
            }
            '<' if !quoted => {
                angle += 1;
                current.push(ch);
            }
            '>' if !quoted && angle > 0 => {
                angle -= 1;
                current.push(ch);
            }
            ',' if !quoted && angle == 0 => {
                out.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        out.push(current.trim().to_string());
    }
    out
}

fn parse_one_address(input: &str) -> Address {
    if let (Some(start), Some(end)) = (input.rfind('<'), input.rfind('>')) {
        if start < end {
            let email = input[start + 1..end].trim().to_string();
            let name = input[..start].trim().trim_matches('"').trim().to_string();
            return Address { name, email };
        }
    }
    Address {
        name: String::new(),
        email: input.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_nested_html_and_addresses() {
        let value = json!({
            "id": "m1",
            "threadId": "t1",
            "snippet": "Hello",
            "internalDate": "1710000000000",
            "labelIds": ["INBOX", "UNREAD"],
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "From", "value": "\"Ada Lovelace\" <ada@harbor.test>"},
                    {"name": "To", "value": "Maya <maya@northmail.test>, sam@studio.test"},
                    {"name": "Subject", "value": "Proof"},
                    {"name": "Message-ID", "value": "<abc@harbor.test>"}
                ],
                "parts": [
                    {
                        "mimeType": "text/plain",
                        "body": {"data": URL_SAFE_NO_PAD.encode("Hello plain")}
                    },
                    {
                        "mimeType": "text/html",
                        "body": {"data": URL_SAFE_NO_PAD.encode("<p>Hello</p>")}
                    },
                    {
                        "mimeType": "image/png",
                        "filename": "shot.png",
                        "body": {"attachmentId": "att1", "size": 12}
                    }
                ]
            }
        });
        let parsed = parse_message("acct", &value).unwrap();
        assert_eq!(parsed.message.from_email, "ada@harbor.test");
        assert_eq!(parsed.message.from_name, "Ada Lovelace");
        assert_eq!(parsed.message.to.len(), 2);
        assert_eq!(parsed.message.body_html, "<p>Hello</p>");
        assert_eq!(parsed.message.attachments[0].id, "att1");
        assert!(parsed.message.label_ids.contains(&"UNREAD".into()));
    }
}
