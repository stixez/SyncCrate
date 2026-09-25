//! Session chat commands (`crate::chat` has the model and why clients poll).
use crate::chat::ChatLog;
use crate::state::{AppState, SessionType};
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_chat(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<ChatLog, String> {
    Ok(state.lock().await.chat.clone())
}

/// Host: posted straight to the log. Client: queued, and delivered on the
/// next poll (within about a second, or right after a running sync).
#[tauri::command]
pub async fn send_chat(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, text: String) -> Result<ChatLog, String> {
    let log = send_chat_inner(state.inner(), &text).await?;
    let _ = app.emit("chat-updated", serde_json::json!({}));
    Ok(log)
}

pub(crate) async fn send_chat_inner(state: &Arc<Mutex<AppState>>, text: &str) -> Result<ChatLog, String> {
    let mut s = state.lock().await;
    match s.session_type {
        SessionType::Host => {
            let name = if s.local_display_name.is_empty() { s.session_name.clone() } else { s.local_display_name.clone() };
            s.chat.post(&name, text, false, crate::utils::timestamp_now()).ok_or("Type a message first.")?;
        }
        SessionType::Client if s.chat.available => s.chat.queue(text)?,
        SessionType::Client => return Err("This host's SyncCrate is too old for chat.".to_string()),
        SessionType::None => return Err("Chat works during a session: host or join first.".to_string()),
    }
    Ok(s.chat.clone())
}
