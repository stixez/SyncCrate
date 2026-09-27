//! Save handoff commands (`crate::handoff` has the rules, `network::handoff_net`
//! the wire). Local changes (share, playing, take over, stop sharing) only
//! edit this PC's record; the next poll carries them to the host and from
//! there to everyone else.
use crate::handoff::{self, SharedSave};
use crate::network::handoff_net;
use crate::state::{AppState, SessionType};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Serialize)]
pub struct SaveRow {
    pub unit: String,
    pub name: String,
    /// The crew it's shared in (None: only on this PC, not shared).
    pub crew: Option<String>,
    pub crew_name: Option<String>,
    pub record: Option<SharedSave>,
    /// This PC's copy.
    pub files: usize,
    pub bytes: u64,
    pub modified: u64,
    pub holder_is_me: bool,
    /// The holder is the other side of this session (our host, or as host a
    /// connected friend).
    pub holder_connected: bool,
}

#[derive(Debug, Serialize)]
pub struct CrewRef {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SharedSavesView {
    /// Crews this game is shared in could be picked from (ours, for this game).
    pub crews: Vec<CrewRef>,
    pub rows: Vec<SaveRow>,
    pub session: String,
    /// The host's name when we're a client.
    pub host_name: Option<String>,
    /// Client: the host takes part in handoff.
    pub host_supports: bool,
    /// Client: joined in a way that proves who the host is (through the
    /// crew, over the internet); handoff needs it.
    pub proven: bool,
}

fn my_node(st: &AppState) -> Result<String, String> {
    st.local_node_id.clone().ok_or_else(|| "SyncCrate is still starting up. Try again in a moment.".into())
}

pub(crate) fn view(st: &AppState, game: &str) -> SharedSavesView {
    let me = st.local_node_id.clone().unwrap_or_default();
    let cts = crate::commands::files::get_game_def(&st.game_registry, game).map(|g| g.content_types.clone()).unwrap_or_default();
    let connected: Vec<String> = st.connections.values().filter_map(|c| c.info.node_id.clone()).collect();
    let mut rows: Vec<SaveRow> = Vec::new();
    // Records first (a save may be shared that this PC doesn't have yet).
    for c in &st.crews.crews {
        for s in c.saves.iter().filter(|s| s.game == game && !s.removed) {
            if rows.iter().any(|r| r.unit.eq_ignore_ascii_case(&s.unit)) {
                continue;
            }
            rows.push(SaveRow {
                unit: s.unit.clone(),
                name: handoff::unit_name(&s.unit).to_string(),
                crew: Some(c.id.clone()),
                crew_name: Some(c.name.clone()),
                holder_is_me: s.holder == me,
                holder_connected: connected.contains(&s.holder),
                record: Some(s.clone()),
                files: 0,
                bytes: 0,
                modified: 0,
            });
        }
    }
    if st.active_game == game {
        for f in st.local_manifest.files.values() {
            if crate::sync::diff::is_disabled_path(&f.relative_path) {
                continue;
            }
            let Some(unit) = handoff::unit_of(&cts, &f.relative_path) else { continue };
            let row = match rows.iter_mut().position(|r| r.unit.eq_ignore_ascii_case(&unit)) {
                Some(i) => &mut rows[i],
                None => {
                    rows.push(SaveRow {
                        name: handoff::unit_name(&unit).to_string(),
                        unit,
                        crew: None,
                        crew_name: None,
                        record: None,
                        files: 0,
                        bytes: 0,
                        modified: 0,
                        holder_is_me: false,
                        holder_connected: false,
                    });
                    rows.last_mut().expect("just pushed")
                }
            };
            row.files += 1;
            row.bytes += f.size;
            row.modified = row.modified.max(f.modified);
        }
    }
    rows.sort_by(|a, b| b.crew.is_some().cmp(&a.crew.is_some()).then(b.modified.cmp(&a.modified)).then(a.name.cmp(&b.name)));
    let is_client = st.session_type == SessionType::Client;
    SharedSavesView {
        crews: st
            .crews
            .crews
            .iter()
            .filter(|c| crate::crews::is_active_member(c, &me))
            .map(|c| CrewRef { id: c.id.clone(), name: c.name.clone() })
            .collect(),
        rows,
        session: match st.session_type {
            SessionType::Host => "host",
            SessionType::Client => "client",
            _ => "none",
        }
        .into(),
        host_name: is_client.then(|| st.connections.values().next().map(|c| c.info.name.clone())).flatten(),
        host_supports: is_client && st.handoff_available,
        proven: is_client && st.host_proven,
    }
}

#[tauri::command]
pub async fn get_shared_saves(state: tauri::State<'_, Arc<Mutex<AppState>>>, game: String) -> Result<SharedSavesView, String> {
    Ok(view(&*state.lock().await, &game))
}

/// Change this PC's record of a save with `f`, after `check` approves it.
async fn change(
    state: &Arc<Mutex<AppState>>,
    app: &crate::event_sink::Events,
    crew: &str,
    game: &str,
    unit: &str,
    check: impl FnOnce(&SharedSave, &str) -> Result<(), String>,
    f: impl FnOnce(&mut SharedSave, &str, &str),
) -> Result<(), String> {
    let mut st = state.lock().await;
    let me = my_node(&st)?;
    let my_name = st.session_name.clone();
    let cur = handoff_net::record(&st, crew, game, unit).ok_or("That save isn't shared any more.")?;
    check(&cur, &me)?;
    let next = cur.bumped(crate::utils::timestamp_now(), |s| f(s, &me, &my_name));
    handoff_net::put(&mut st, crew, next)?;
    let _ = app.emit("handoff-updated", serde_json::json!({}));
    Ok(())
}

pub(crate) async fn share_inner(state: &Arc<Mutex<AppState>>, app: &crate::event_sink::Events, crew: &str, game: &str, unit: &str) -> Result<(), String> {
    let mut st = state.lock().await;
    let me = my_node(&st)?;
    if st.active_game != game {
        return Err("Open this game in SyncCrate first.".into());
    }
    let c = st.crews.get(crew).ok_or("That crew is gone.")?;
    if !crate::crews::is_active_member(c, &me) {
        return Err("You're not in that crew any more.".into());
    }
    if st.crews.crews.iter().any(|c| c.saves.iter().any(|s| s.is(game, unit) && !s.removed)) {
        return Err("This save is already shared.".into());
    }
    let cts = crate::commands::files::get_game_def(&st.game_registry, game).map(|g| g.content_types.clone()).unwrap_or_default();
    if !handoff::game_supports(&cts) {
        return Err("This game's saves can't be handed over yet.".into());
    }
    if handoff::files_of(&cts, st.local_manifest.files.values(), unit).is_empty() {
        return Err("This save isn't on this PC.".into());
    }
    let now = crate::utils::timestamp_now();
    let name = st.session_name.clone();
    // Sharing again after "stop sharing" continues its record, so the old
    // tombstone can't win against it.
    let next = match st.crews.get(crew).and_then(|c| c.saves.iter().find(|s| s.is(game, unit)).cloned()) {
        Some(old) => old.bumped(now, |s| {
            s.removed = false;
            s.holder = me.clone();
            s.holder_name = name.clone();
            s.playing = false;
        }),
        None => SharedSave { game: game.into(), unit: unit.into(), holder: me, holder_name: name, playing: false, removed: false, version: 1, updated_at: now, claimed: false },
    };
    handoff_net::put(&mut st, crew, next)?;
    let _ = app.emit("handoff-updated", serde_json::json!({}));
    Ok(())
}

#[tauri::command]
pub async fn share_save(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<(), String> {
    share_inner(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit).await
}

#[tauri::command]
pub async fn unshare_save(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<(), String> {
    change(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit, |_, _| Ok(()), |s, _, _| {
        s.removed = true;
        s.playing = false;
    })
    .await
}

#[tauri::command]
pub async fn set_save_playing(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String, playing: bool) -> Result<(), String> {
    change(
        state.inner(),
        &crate::event_sink::from_app(&app),
        &crew,
        &game,
        &unit,
        |cur, me| if cur.holder == me { Ok(()) } else { Err(format!("{} has this save now.", cur.holder_name)) },
        |s, _, _| s.playing = playing,
    )
    .await
}

/// The holder is gone for good (or lost the save): take it as it is on this
/// PC. Their changes since the last handover are lost; the UI says so.
#[tauri::command]
pub async fn take_over_save(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<(), String> {
    change(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit, |_, _| Ok(()), |s, me, name| {
        s.holder = me.to_string();
        s.holder_name = name.to_string();
        s.playing = false;
    })
    .await
}

/// Host: trust a friend who says they hold a save (they took it over, or
/// got it in a session this PC wasn't in), so their copy may replace ours.
pub(crate) async fn accept_copy_inner(state: &Arc<Mutex<AppState>>, app: &crate::event_sink::Events, crew: &str, game: &str, unit: &str) -> Result<(), String> {
    let mut st = state.lock().await;
    let mut rec = handoff_net::record(&st, crew, game, unit).ok_or("That save isn't shared any more.")?;
    // Local only (no new version): whether to trust is this PC's call.
    rec.claimed = false;
    handoff_net::put(&mut st, crew, rec)?;
    let _ = app.emit("handoff-updated", serde_json::json!({}));
    Ok(())
}

#[tauri::command]
pub async fn accept_save_copy(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<(), String> {
    accept_copy_inner(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit).await
}

/// Before a take or give: nothing else may be moving files, and the game
/// must be closed (it would write the save while it's replaced, or not
/// have written it yet).
async fn ready_to_move(state: &Arc<Mutex<AppState>>) -> Result<(), String> {
    crate::commands::backup::refuse_during_restore()?;
    let (procs, label) = {
        let st = state.lock().await;
        if st.is_any_syncing() {
            return Err("Wait for the sync to finish first.".into());
        }
        let def = crate::commands::files::get_game_def(&st.game_registry, &st.active_game);
        (def.map(|g| g.process_names.clone()).unwrap_or_default(), def.map(|g| g.label.clone()).unwrap_or_default())
    };
    if crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false) {
        return Err(format!("Close {label} first, so the save is complete."));
    }
    Ok(())
}

#[tauri::command]
pub async fn take_save(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<usize, String> {
    ready_to_move(state.inner()).await?;
    handoff_net::take(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit).await
}

#[tauri::command]
pub async fn give_save(state: tauri::State<'_, Arc<Mutex<AppState>>>, app: tauri::AppHandle, crew: String, game: String, unit: String) -> Result<(), String> {
    ready_to_move(state.inner()).await?;
    handoff_net::give(state.inner(), &crate::event_sink::from_app(&app), &crew, &game, &unit).await
}
