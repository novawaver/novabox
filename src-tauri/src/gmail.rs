use reqwest::Method;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use url::Url;

use crate::auth::{self, ensure_access};
use crate::model::{Label, SendAs};
use crate::util::redact;
use crate::AppState;

pub struct GmailProfile {
    pub history_id: String,
}

pub async fn gmail(
    app: &AppHandle,
    account_id: &str,
    method: Method,
    url: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let http = app.state::<AppState>().http.clone();
    for attempt in 0..2 {
        let token = ensure_access(app, account_id).await?;
        let mut request = http.request(method.clone(), &url).bearer_auth(&token);
        if let Some(body) = &body {
            request = request.json(body);
        } else if method == Method::POST || method == Method::PUT || method == Method::PATCH {
            // An empty POST is sent with no length, and Google answers 411. `{}` gives the request a length.
            request = request.json(&json!({}));
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Could not reach Gmail: {e}"))?;
        let status = response.status();
        if status.as_u16() == 401 && attempt == 0 {
            auth::invalidate(app, account_id);
            continue;
        }
        if status.as_u16() == 204 {
            return Ok(Value::Null);
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("Could not read Gmail's response: {e}"))?;
        let text = String::from_utf8_lossy(&bytes);
        if status.as_u16() == 404 {
            return Err("gmail_404:Gmail could not find that conversation.".into());
        }
        if !status.is_success() {
            let detail = if text.trim_start().starts_with('<') {
                "Gmail rejected the request.".to_string()
            } else {
                redact(&text)
            };
            return Err(format!("Gmail returned {status}: {detail}"));
        }
        if text.trim().is_empty() {
            return Err("Gmail returned an empty message.".into());
        }
        return serde_json::from_str(&text)
            .map_err(|_| "Gmail returned a response Novabox could not read.".to_string());
    }
    Err("Google rejected the saved sign-in. Remove the account and sign in again.".into())
}

pub async fn profile(app: &AppHandle, account_id: &str) -> Result<GmailProfile, String> {
    let value = gmail(app, account_id, Method::GET, users("profile"), None).await?;
    Ok(GmailProfile {
        history_id: string_field(&value, "historyId"),
    })
}

pub async fn labels(app: &AppHandle, account_id: &str) -> Result<Vec<Label>, String> {
    let value = gmail(app, account_id, Method::GET, users("labels"), None).await?;
    let mut labels = Vec::new();
    for item in value["labels"].as_array().cloned().unwrap_or_default() {
        let id = string_field(&item, "id");
        if id.is_empty() {
            continue;
        }
        labels.push(Label {
            account_id: account_id.to_string(),
            id,
            name: string_field(&item, "name"),
            kind: string_field(&item, "type"),
            color: item
                .get("color")
                .and_then(|color| color.get("backgroundColor"))
                .and_then(|value| value.as_str())
                .map(|value| value.to_string()),
            unread: item
                .get("threadsUnread")
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
        });
    }
    Ok(labels)
}

pub async fn send_as(app: &AppHandle, account_id: &str) -> Result<Vec<SendAs>, String> {
    let value = gmail(
        app,
        account_id,
        Method::GET,
        "https://gmail.googleapis.com/gmail/v1/users/me/settings/sendAs".into(),
        None,
    )
    .await?;
    Ok(value["sendAs"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|item| SendAs {
            email: string_field(&item, "sendAsEmail"),
            display_name: string_field(&item, "displayName"),
            is_default: item
                .get("isDefault")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
            is_primary: item
                .get("isPrimary")
                .and_then(|value| value.as_bool())
                .unwrap_or(false),
        })
        .filter(|item| !item.email.is_empty())
        .collect())
}

pub async fn list_message_ids(
    app: &AppHandle,
    account_id: &str,
    query: &str,
    page_token: Option<&str>,
) -> Result<(Vec<String>, Option<String>), String> {
    list_ids(app, account_id, query, None, page_token).await
}

/// Lists one system label such as SPAM or TRASH. Gmail leaves both out of a plain list
/// unless `includeSpamTrash` is set.
pub async fn list_label_ids(
    app: &AppHandle,
    account_id: &str,
    label_id: &str,
    page_token: Option<&str>,
) -> Result<(Vec<String>, Option<String>), String> {
    list_ids(app, account_id, "", Some(label_id), page_token).await
}

async fn list_ids(
    app: &AppHandle,
    account_id: &str,
    query: &str,
    label_id: Option<&str>,
    page_token: Option<&str>,
) -> Result<(Vec<String>, Option<String>), String> {
    let mut url = Url::parse(&users("messages")).map_err(|e| e.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("maxResults", "50");
        if !query.is_empty() {
            pairs.append_pair("q", query);
        }
        if let Some(label_id) = label_id {
            pairs.append_pair("labelIds", label_id);
            pairs.append_pair("includeSpamTrash", "true");
        }
        if let Some(token) = page_token {
            pairs.append_pair("pageToken", token);
        }
    }
    let value = gmail(app, account_id, Method::GET, url.to_string(), None).await?;
    let ids = value["messages"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("id").and_then(|id| id.as_str()).map(|id| id.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let next = value
        .get("nextPageToken")
        .and_then(|token| token.as_str())
        .map(|token| token.to_string());
    Ok((ids, next))
}

pub async fn get_message(app: &AppHandle, account_id: &str, message_id: &str) -> Result<Value, String> {
    let full = gmail(
        app,
        account_id,
        Method::GET,
        message_url(message_id, "format=full"),
        None,
    )
    .await;
    match full {
        Ok(value) => Ok(value),
        Err(err) => {
            let headers = "format=metadata&metadataHeaders=From&metadataHeaders=To&metadataHeaders=Cc&metadataHeaders=Subject&metadataHeaders=Date";
            match gmail(app, account_id, Method::GET, message_url(message_id, headers), None).await {
                Ok(value) => Ok(value),
                Err(_) => Err(err),
            }
        }
    }
}

fn message_url(message_id: &str, query: &str) -> String {
    format!("https://gmail.googleapis.com/gmail/v1/users/me/messages/{message_id}?{query}")
}

pub async fn history(
    app: &AppHandle,
    account_id: &str,
    start_history_id: &str,
    page_token: Option<&str>,
) -> Result<Value, String> {
    let mut url = Url::parse(&users("history")).map_err(|e| e.to_string())?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs
            .append_pair("startHistoryId", start_history_id)
            .append_pair("maxResults", "100")
            .append_pair("historyTypes", "messageAdded")
            .append_pair("historyTypes", "messageDeleted")
            .append_pair("historyTypes", "labelAdded")
            .append_pair("historyTypes", "labelRemoved");
        if let Some(token) = page_token {
            pairs.append_pair("pageToken", token);
        }
    }
    gmail(app, account_id, Method::GET, url.to_string(), None).await
}

pub async fn modify_thread(
    app: &AppHandle,
    account_id: &str,
    thread_id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), String> {
    let url = thread_url(thread_id, "modify")?;
    gmail(
        app,
        account_id,
        Method::POST,
        url,
        Some(json!({ "addLabelIds": add, "removeLabelIds": remove })),
    )
    .await?;
    Ok(())
}

pub async fn trash_thread(app: &AppHandle, account_id: &str, thread_id: &str) -> Result<(), String> {
    gmail(app, account_id, Method::POST, thread_url(thread_id, "trash")?, None).await?;
    Ok(())
}

pub async fn untrash_thread(app: &AppHandle, account_id: &str, thread_id: &str) -> Result<(), String> {
    gmail(
        app,
        account_id,
        Method::POST,
        thread_url(thread_id, "untrash")?,
        None,
    )
    .await?;
    Ok(())
}

pub async fn send_raw(
    app: &AppHandle,
    account_id: &str,
    raw: &str,
    thread_id: Option<&str>,
) -> Result<Value, String> {
    let mut body = json!({ "raw": raw });
    if let Some(thread_id) = thread_id {
        body["threadId"] = json!(thread_id);
    }
    gmail(app, account_id, Method::POST, users("messages/send"), Some(body)).await
}

pub async fn create_draft(
    app: &AppHandle,
    account_id: &str,
    raw: &str,
    thread_id: Option<&str>,
) -> Result<Value, String> {
    let mut message = json!({ "raw": raw });
    if let Some(thread_id) = thread_id {
        message["threadId"] = json!(thread_id);
    }
    gmail(
        app,
        account_id,
        Method::POST,
        users("drafts"),
        Some(json!({ "message": message })),
    )
    .await
}

pub async fn update_draft(
    app: &AppHandle,
    account_id: &str,
    draft_id: &str,
    raw: &str,
    thread_id: Option<&str>,
) -> Result<Value, String> {
    let mut message = json!({ "raw": raw });
    if let Some(thread_id) = thread_id {
        message["threadId"] = json!(thread_id);
    }
    let url = gmail_path(&["drafts", draft_id])?;
    gmail(
        app,
        account_id,
        Method::PUT,
        url,
        Some(json!({ "id": draft_id, "message": message })),
    )
    .await
}

pub async fn delete_draft(app: &AppHandle, account_id: &str, draft_id: &str) -> Result<(), String> {
    gmail(app, account_id, Method::DELETE, gmail_path(&["drafts", draft_id])?, None).await?;
    Ok(())
}

pub async fn list_drafts(app: &AppHandle, account_id: &str) -> Result<Vec<(String, String)>, String> {
    let value = gmail(
        app,
        account_id,
        Method::GET,
        users("drafts?maxResults=100"),
        None,
    )
    .await?;
    Ok(value["drafts"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|draft| {
            let id = string_field(&draft, "id");
            let message_id = draft
                .get("message")
                .map(json_id)
                .unwrap_or_default();
            if id.is_empty() || message_id.is_empty() {
                None
            } else {
                Some((id, message_id))
            }
        })
        .collect())
}

pub async fn attachment(
    app: &AppHandle,
    account_id: &str,
    message_id: &str,
    attachment_id: &str,
) -> Result<String, String> {
    let url = gmail_path(&["messages", message_id, "attachments", attachment_id])?;
    let value = gmail(app, account_id, Method::GET, url, None).await?;
    Ok(string_field(&value, "data"))
}

fn users(path: &str) -> String {
    format!("https://gmail.googleapis.com/gmail/v1/users/me/{path}")
}

fn thread_url(thread_id: &str, action: &str) -> Result<String, String> {
    gmail_path(&["threads", thread_id, action])
}

fn gmail_path(segments: &[&str]) -> Result<String, String> {
    let mut url = Url::parse("https://gmail.googleapis.com/gmail/v1/users/me").map_err(|e| e.to_string())?;
    {
        let mut path = url
            .path_segments_mut()
            .map_err(|_| "Could not build a Gmail URL.".to_string())?;
        for segment in segments {
            path.push(segment);
        }
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::{gmail_path, thread_url};

    #[test]
    fn thread_modify_url_matches_gmail() {
        let url = thread_url("1a1175d5d4e33395", "modify").unwrap();
        assert_eq!(
            url,
            "https://gmail.googleapis.com/gmail/v1/users/me/threads/1a1175d5d4e33395/modify"
        );
        assert!(!url.contains("//1"));
        let attachment = gmail_path(&["messages", "abc", "attachments", "id+part/1"]).unwrap();
        assert_eq!(
            attachment,
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/abc/attachments/id+part%2F1"
        );
    }

    #[test]
    fn trash_post_with_json_object_is_not_411() {
        let status = tauri::async_runtime::block_on(async {
            let http = reqwest::Client::builder()
                .http1_only()
                .timeout(std::time::Duration::from_secs(20))
                .build()
                .unwrap();
            let url = "https://gmail.googleapis.com/gmail/v1/users/me/threads/1a1175d5d4e33395/trash";
            let empty = http.post(url).body("").send().await.unwrap().status().as_u16();
            let sized = http
                .post(url)
                .json(&serde_json::json!({}))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16();
            (empty, sized)
        });
        assert_eq!(status.1, 401);
    }
}

fn string_field(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

fn json_id(value: &Value) -> String {
    string_field(value, "id")
}
