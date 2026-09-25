//! Commands for "friends can offer mods to the host" (`crate::offers` has the
//! model and the wire flow).
use crate::offers::{IncomingOffer, OfferState, OfferedFile, OutgoingOffer};
use crate::state::{AppState, SessionType};
use serde::Serialize;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Default)]
pub struct OutgoingOfferView {
    /// Connected to a host that takes offers.
    pub available: bool,
    pub offer: Option<OutgoingOffer>,
}

#[tauri::command]
pub async fn get_outgoing_offer(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<OutgoingOfferView, String> {
    let s = state.lock().await;
    Ok(OutgoingOfferView { available: s.session_type == SessionType::Client && s.offers_available, offer: s.offer_out.clone() })
}

/// Client: offer these local files to the host. Only files the host doesn't
/// have, inside the game's content folders and of allowed types, go in.
#[tauri::command]
pub async fn offer_files(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, paths: Vec<String>) -> Result<OutgoingOffer, String> {
    let offer = offer_files_inner(state.inner(), paths).await?;
    let _ = app.emit("offer-updated", serde_json::json!({}));
    Ok(offer)
}

pub(crate) async fn offer_files_inner(state: &Arc<Mutex<AppState>>, paths: Vec<String>) -> Result<OutgoingOffer, String> {
    {
        let s = state.lock().await;
        if s.session_type != SessionType::Client || s.connections.is_empty() {
            return Err("Join a host first, then offer them files.".into());
        }
        if !s.offers_available {
            return Err("This host's SyncCrate is too old to accept offers.".into());
        }
        if s.offer_out.as_ref().is_some_and(|o| o.active()) {
            return Err("You already have an offer waiting for the host. Cancel it to offer something else.".into());
        }
    }
    // Offers carry hashes the host checks each upload against.
    crate::commands::files::scan_files_inner(state, None, true).await?;
    let mut s = state.lock().await;
    let cts = crate::commands::files::get_game_def(&s.game_registry, &s.active_game).map(|g| g.content_types.clone()).unwrap_or_default();
    let host_manifest = s.connections.values().next().and_then(|c| c.remote_manifest.clone()).unwrap_or_default();
    let wanted: std::collections::HashSet<String> = paths.iter().map(|p| crate::sync::diff::match_key(p)).collect();
    let files: Vec<_> = s
        .local_manifest
        .files
        .values()
        .filter(|f| wanted.contains(&crate::sync::diff::match_key(&f.relative_path)))
        .cloned()
        .collect();
    let valid = crate::offers::valid_offer(files, &cts, &host_manifest);
    if valid.is_empty() {
        return Err("None of these can be offered: the host already has them, or they aren't mod files in this game's folders.".into());
    }
    let offer = OutgoingOffer {
        files: valid.into_iter().map(|file| OfferedFile { file, state: OfferState::Pending, message: None }).collect(),
        delivered: false,
    };
    s.offer_out = Some(offer.clone());
    Ok(offer)
}

/// Client: drop our offer. (A host that already accepted files just won't
/// receive them.)
#[tauri::command]
pub async fn cancel_offer(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle) -> Result<(), String> {
    state.lock().await.offer_out = None;
    let _ = app.emit("offer-updated", serde_json::json!({}));
    Ok(())
}

#[tauri::command]
pub async fn get_incoming_offers(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<IncomingOffer>, String> {
    let s = state.lock().await;
    let mut offers: Vec<_> = s.offers_in.values().cloned().collect();
    offers.sort_by(|a, b| a.peer_name.cmp(&b.peer_name));
    Ok(offers)
}

/// Host: take (or turn down) files from a friend's offer. Accepted files are
/// uploaded by the friend's app on its next poll and checked again on arrival.
#[tauri::command]
pub async fn decide_offer(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    peer_id: String,
    accept: Vec<String>,
    decline: Vec<String>,
) -> Result<IncomingOffer, String> {
    let offer = decide_offer_inner(state.inner(), &peer_id, &accept, &decline).await?;
    let _ = app.emit("offers-updated", serde_json::json!({}));
    Ok(offer)
}

pub(crate) async fn decide_offer_inner(state: &Arc<Mutex<AppState>>, peer_id: &str, accept: &[String], decline: &[String]) -> Result<IncomingOffer, String> {
    let mut s = state.lock().await;
    if s.session_type != SessionType::Host {
        return Err("Only the host decides on offers.".into());
    }
    let offer = s.offers_in.get_mut(peer_id).ok_or("That friend's offer is gone (they disconnected or cancelled).")?;
    for f in offer.files.iter_mut().filter(|f| f.state == OfferState::Pending) {
        if accept.contains(&f.file.relative_path) {
            f.state = OfferState::Accepted;
        } else if decline.contains(&f.file.relative_path) {
            f.state = OfferState::Declined;
        }
    }
    Ok(offer.clone())
}
