//! Save handoff on the wire (`crate::handoff` has the rules). The client
//! starts every exchange: a `HandoffSync` poll carries both sides' records,
//! and a take / give rides on it; each reply echoes its request's id, so a
//! late answer is never taken for the next one's. Uploads for a give reuse
//! the offer upload (`FileHeader` + chunks, answered by `OfferResult`), but
//! land in temp files next to their destination and only replace the host's
//! save on `Commit`, all together, with the old files kept in File history.
use crate::event_sink::Events;
use crate::handoff::{self, CrewSaves, HandoffReply, HandoffRequest, PendingGive, SharedSave};
use crate::network::protocol::{self, Message};
use crate::network::stream::PeerStream;
use crate::network::transfer;
use crate::registry::ContentType;
use crate::state::{AppState, FileInfo};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// How often an idle client exchanges records with the host.
pub const POLL: std::time::Duration = std::time::Duration::from_secs(5);
/// A take or commit hashes the whole save (cached where unchanged) and may
/// copy it into File history first; a big world takes a while. Like the
/// wait for a file header.
const REQUEST_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

/// Why handoff isn't offered on a LAN connection.
pub const NOT_PROVEN: &str = "To take or give a save, join the host from Crews (click Join on your crew). Joining over the local network or by IP won't work for this.";

/// Crews both we and `other` (a node id) are active members of.
fn common_crews<'a>(st: &'a AppState, other: &str) -> Vec<&'a crate::crews::Crew> {
    let me = st.local_node_id.as_deref().unwrap_or_default();
    st.crews.crews.iter().filter(|c| crate::crews::is_active_member(c, other) && crate::crews::is_active_member(c, me)).collect()
}

/// Our records for the crews we share with `other`.
pub fn records_for(st: &AppState, other: &str) -> Vec<CrewSaves> {
    common_crews(st, other).into_iter().map(|c| CrewSaves { crew: c.id.clone(), saves: c.saves.clone() }).collect()
}

/// Merge `other`'s records into the crews we share with them; true if
/// anything changed (and was saved).
pub fn merge_from(st: &mut AppState, other: &str, incoming: Vec<CrewSaves>) -> bool {
    let now = crate::utils::timestamp_now();
    let allowed: HashSet<String> = common_crews(st, other).into_iter().map(|c| c.id.clone()).collect();
    let known: HashSet<String> = st.game_registry.games.iter().map(|g| g.id.clone()).collect();
    let mut changed = false;
    for cs in incoming.into_iter().take(crate::crews::MAX_CREWS) {
        if !allowed.contains(&cs.crew) {
            continue;
        }
        if let Some(crew) = st.crews.get_mut(&cs.crew) {
            changed |= handoff::receive(&mut crew.saves, other, cs.saves, |g| known.contains(g), now);
        }
    }
    if changed {
        crate::crews::persist(st);
    }
    changed
}

/// Replace one record (a change made on this PC).
pub fn put(st: &mut AppState, crew: &str, next: SharedSave) -> Result<(), String> {
    let c = st.crews.get_mut(crew).ok_or("That crew is gone.")?;
    match c.saves.iter().position(|s| s.is(&next.game, &next.unit)) {
        Some(i) => c.saves[i] = next,
        None if handoff::make_room(&mut c.saves, &next) => c.saves.push(next),
        None => return Err(format!("A crew can share up to {} saves.", handoff::MAX_SAVES_PER_CREW)),
    }
    crate::crews::persist(st);
    Ok(())
}

pub fn record(st: &AppState, crew: &str, game: &str, unit: &str) -> Option<SharedSave> {
    st.crews.get(crew)?.saves.iter().find(|s| s.is(game, unit) && !s.removed).cloned()
}

/// A friend's give ends (done, failed, or they left): delete what it staged.
pub fn drop_pending(st: &mut AppState, peer_id: &str) {
    st.handoff_grants.remove(peer_id);
    if let Some(p) = st.handoff_in.remove(peer_id) {
        for tmp in p.staged.values() {
            let _ = std::fs::remove_file(tmp);
        }
    }
}

/// A save's files as they are on disk now: the game may have just written
/// new ones (Minecraft region files) that no scan has seen, and a give
/// without them left the host an incomplete world.
fn unit_files_on_disk(base: &str, cts: &[ContentType], unit: &str) -> Result<Vec<String>, String> {
    let (ct_id, name) = unit.split_once('/').ok_or("Not a save.")?;
    let ct = cts.iter().find(|c| c.id == ct_id && c.save_unit_depth.is_some()).ok_or("This game's saves can't be handed over.")?;
    // Outside the game folder for Valheim or Stardew (`ContentType::roots`).
    let root = crate::utils::ct_dir(base, ct).ok_or("This save's folder isn't on this PC. Start the game once, or set the folder in Settings.")?;
    let (walk_root, max_depth) = if ct.save_unit_depth == Some(0) {
        (root.clone(), 1)
    } else {
        if name.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.contains(['\\', ':'])) {
            return Err("Not a save.".into());
        }
        (root.join(name), 64)
    };
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(&walk_root).follow_links(false).max_depth(max_depth).into_iter().flatten() {
        if !e.file_type().is_file() {
            continue;
        }
        let Some(rel) = crate::utils::manifest_path(base, ct, e.path()) else { continue };
        if crate::sync::diff::is_disabled_path(&rel) || !crate::commands::files::content_type_accepts_in(ct, e.path(), Some(&root)) {
            continue;
        }
        if handoff::unit_of(cts, &rel).is_some_and(|u| u.eq_ignore_ascii_case(unit)) {
            out.push(rel);
            if out.len() > handoff::MAX_SAVE_FILES {
                return Err("That save has too many files to hand over.".into());
            }
        }
    }
    out.sort();
    Ok(out)
}

/// The save's files on this PC, hashed now (a stale hash would hand over
/// the wrong version).
pub(crate) async fn fresh_files(state: &Arc<Mutex<AppState>>, unit: &str) -> Result<(String, Vec<FileInfo>), String> {
    let (base, cts) = {
        let st = state.lock().await;
        let cts = crate::commands::files::get_game_def(&st.game_registry, &st.active_game).map(|g| g.content_types.clone()).unwrap_or_default();
        (st.active_game_path()?, cts)
    };
    let (b, u) = (base.clone(), unit.to_string());
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<FileInfo>, String> {
        // Unchanged files reuse the scan's hash: re-reading a whole world on
        // every step of a handover outlasted the reply wait.
        let cache = crate::commands::files::hash_lookup();
        unit_files_on_disk(&b, &cts, &u)?
            .into_iter()
            .map(|rel| {
                let path = crate::utils::safe_join(&b, &rel)?;
                let meta = std::fs::metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
                let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                let hash = match cache.get(&path, meta.len(), modified) {
                    Some(h) => h,
                    None => crate::commands::files::compute_file_hash(&path)?,
                };
                Ok(FileInfo { relative_path: rel, size: meta.len(), hash, modified, file_type: "Save".into() })
            })
            .collect()
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok((base, files))
}

async fn game_running(state: &Arc<Mutex<AppState>>) -> bool {
    let procs = {
        let st = state.lock().await;
        crate::commands::files::get_game_def(&st.game_registry, &st.active_game).map(|g| g.process_names.clone()).unwrap_or_default()
    };
    crate::commands::game_state::is_game_running(&procs).await.unwrap_or(false)
}

async fn content_types(state: &Arc<Mutex<AppState>>, game: &str) -> Vec<ContentType> {
    let st = state.lock().await;
    crate::commands::files::get_game_def(&st.game_registry, game).map(|g| g.content_types.clone()).unwrap_or_default()
}

fn refused(message: impl Into<String>) -> HandoffReply {
    HandoffReply::Refused { message: message.into() }
}

// ---------------------------------------------------------------------------
// Host side

/// One `HandoffSync` from a friend: merge their records for crews we're
/// both in, answer the request, reply with our records. `proven`: the
/// friend's node id as the iroh connection proved it (None over LAN TCP).
pub async fn host_sync(state: &Arc<Mutex<AppState>>, app: &Events, peer_id: &str, proven: Option<String>, id: u64, saves: Vec<CrewSaves>, request: Option<HandoffRequest>) -> Message {
    let (node, peer_name) = {
        let mut st = state.lock().await;
        let Some((node, name)) = proven.and_then(|n| Some((n, st.connections.get(peer_id)?.info.name.clone()))) else {
            let reply = request.map(|_| refused(NOT_PROVEN));
            return Message::HandoffStatus { id, saves: Vec::new(), reply };
        };
        if merge_from(&mut st, &node, saves) {
            let _ = app.emit("handoff-updated", serde_json::json!({}));
        }
        (node, name)
    };
    let reply = match request {
        Some(r) => Some(host_request(state, app, peer_id, &node, &peer_name, r).await),
        None => None,
    };
    let saves = records_for(&*state.lock().await, &node);
    Message::HandoffStatus { id, saves, reply }
}

async fn host_request(state: &Arc<Mutex<AppState>>, app: &Events, peer_id: &str, node: &str, peer_name: &str, r: HandoffRequest) -> HandoffReply {
    let (crew, game, unit) = match &r {
        HandoffRequest::Take { crew, game, unit }
        | HandoffRequest::TakeDone { crew, game, unit, .. }
        | HandoffRequest::Give { crew, game, unit, .. }
        | HandoffRequest::Commit { crew, game, unit } => (crew.clone(), game.clone(), unit.clone()),
    };
    let (me, my_name, current) = {
        let st = state.lock().await;
        if !common_crews(&st, node).iter().any(|c| c.id == crew) {
            return refused("You and the host aren't in that crew together.");
        }
        if st.active_game != game {
            return refused("The host has another game open.");
        }
        let Some(me) = st.local_node_id.clone() else { return refused("The host's SyncCrate isn't ready yet. Try again in a moment.") };
        (me, st.session_name.clone(), record(&st, &crew, &game, &unit))
    };
    let Some(current) = current else { return refused("That save isn't shared in this crew any more.") };
    let now = crate::utils::timestamp_now();
    match r {
        HandoffRequest::Take { .. } => {
            if current.holder != me {
                return refused(format!("{} has this save, not the host.", current.holder_name));
            }
            if current.playing {
                return refused(format!("{my_name} is playing it right now."));
            }
            if game_running(state).await {
                return refused("The host has the game open. Ask them to close it first, so the save is complete.");
            }
            // Only a list: the save changes hands once the friend's copy is
            // complete (`TakeDone`). Moving it here left a friend whose
            // download failed "holding" a broken copy they could then give.
            match fresh_files(state, &unit).await {
                Ok((_, f)) if !f.is_empty() => {
                    state.lock().await.handoff_grants.insert(peer_id.to_string(), f.iter().map(|x| x.relative_path.clone()).collect());
                    HandoffReply::Granted { files: f }
                }
                Ok(_) => refused("The host doesn't have this save's files."),
                Err(e) => refused(format!("The host couldn't read the save: {e}")),
            }
        }
        HandoffRequest::TakeDone { files, .. } => {
            state.lock().await.handoff_grants.remove(peer_id);
            if current.holder != me || current.playing {
                return refused(format!("The save changed hands meanwhile: {} has it now.", current.holder_name));
            }
            if game_running(state).await {
                return refused("The host has the game open. Ask them to close it first.");
            }
            match fresh_files(state, &unit).await {
                Ok((_, host_files)) if handoff::same_save(&host_files, &files) => {}
                Ok(_) => return refused("The host's copy changed while you were downloading. Take it again."),
                Err(e) => return refused(format!("The host couldn't read the save: {e}")),
            }
            let mut st = state.lock().await;
            if record(&st, &crew, &game, &unit).as_ref() != Some(&current) {
                return refused("The save changed hands just now. Try again.");
            }
            let next = current.bumped(now, |s| {
                s.holder = node.to_string();
                s.holder_name = peer_name.to_string();
                s.playing = false;
            });
            if let Err(e) = put(&mut st, &crew, next) {
                return refused(e);
            }
            let _ = app.emit("handoff-updated", serde_json::json!({}));
            HandoffReply::Done
        }
        HandoffRequest::Give { files, .. } => {
            if current.holder != node {
                return refused(format!("{} has this save, so only they can hand it over.", current.holder_name));
            }
            // Only their own word that they hold it (they took it over, or
            // it happened in a session we weren't in): the host decides.
            if current.claimed {
                let _ = app.emit("handoff-approval", serde_json::json!({ "name": peer_name, "save": handoff::unit_name(&unit) }));
                return refused(format!("{my_name} needs to accept your copy first: they'll see a button under Shared saves. Then give it again."));
            }
            if game_running(state).await {
                return refused("The host has the game open. Ask them to close it first.");
            }
            let cts = content_types(state, &game).await;
            if let Err(e) = handoff::check_files(&cts, &unit, &files) {
                return refused(e);
            }
            let host_files = match fresh_files(state, &unit).await {
                Ok((_, f)) => f,
                Err(e) => return refused(format!("The host couldn't read its copy: {e}")),
            };
            let paths: Vec<String> = handoff::diff_save(&host_files, &files).0.into_iter().map(|f| f.relative_path).collect();
            let mut st = state.lock().await;
            drop_pending(&mut st, peer_id);
            st.handoff_in.insert(
                peer_id.to_string(),
                PendingGive { crew, game, unit, files, needed: paths.iter().cloned().collect(), staged: Default::default() },
            );
            HandoffReply::Upload { paths }
        }
        HandoffRequest::Commit { .. } => {
            let pending = {
                let mut st = state.lock().await;
                match st.handoff_in.get(peer_id) {
                    Some(p) if p.crew == crew && p.game == game && p.unit.eq_ignore_ascii_case(&unit) => {}
                    _ => return refused("Nothing was handed over. Try again."),
                }
                st.handoff_in.remove(peer_id).expect("checked")
            };
            let cleanup = |p: &PendingGive| {
                for tmp in p.staged.values() {
                    let _ = std::fs::remove_file(tmp);
                }
            };
            if !pending.needed.iter().all(|p| pending.staged.contains_key(p)) {
                cleanup(&pending);
                return refused("Some of the save's files didn't arrive. Try again.");
            }
            if current.holder != node || current.claimed {
                cleanup(&pending);
                return refused(format!("{} has this save now.", current.holder_name));
            }
            if game_running(state).await {
                cleanup(&pending);
                return refused("The host has the game open. Ask them to close it first.");
            }
            let (base, host_files) = match fresh_files(state, &unit).await {
                Ok(v) => v,
                Err(e) => {
                    cleanup(&pending);
                    return refused(format!("The host couldn't read its copy: {e}"));
                }
            };
            // Files are replaced now: no restore, undo or sync alongside.
            let Some(_replacing) = crate::commands::backup::try_begin_restoring() else {
                cleanup(&pending);
                return refused("The host is restoring a backup. Try again when it's done.");
            };
            let (who, g) = (peer_name.to_string(), game.clone());
            let applied = tokio::task::spawn_blocking(move || apply_give(&base, &g, &who, &pending, &host_files)).await.map_err(|e| e.to_string()).and_then(|r| r);
            if let Err(e) = applied {
                return refused(format!("The host couldn't put the save in place: {e}"));
            }
            {
                let mut st = state.lock().await;
                if let Some(cur) = record(&st, &crew, &game, &unit) {
                    let next = cur.bumped(now, |s| {
                        s.holder = me.clone();
                        s.holder_name = my_name.clone();
                        s.playing = false;
                    });
                    let _ = put(&mut st, &crew, next);
                }
            }
            let _ = app.emit("handoff-updated", serde_json::json!({}));
            let st = state.clone();
            tokio::spawn(async move {
                let _ = crate::commands::files::scan_files_inner(&st, None, true).await;
            });
            HandoffReply::Done
        }
    }
}

/// Put a friend's staged save in place of ours: their files replace ours,
/// ours that they don't have go, the old versions stay in File history.
fn apply_give(base: &str, game: &str, from: &str, p: &PendingGive, host_files: &[FileInfo]) -> Result<(), String> {
    use crate::commands::history;
    let (_, delete) = handoff::diff_save(host_files, &p.files);
    let existing: HashSet<String> = host_files.iter().map(|f| crate::sync::diff::match_key(&f.relative_path)).collect();
    let mut targets: Vec<String> = p.staged.keys().filter(|k| existing.contains(&crate::sync::diff::match_key(k))).cloned().collect();
    targets.extend(delete.iter().cloned());
    let root = crate::utils::backups_dir();
    let now = crate::utils::timestamp_now();
    let capture = (!targets.is_empty() && crate::commands::sync::read_sync_config().keep_file_history).then(|| {
        let id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = history::begin_capture(&root, game, base, &targets, from, &id, now) {
            log::warn!("File history: couldn't keep the old save: {e}");
        }
        id
    });
    let mut changed: Vec<(String, &str)> = Vec::new();
    let mut errors = Vec::new();
    for (rel, tmp) in &p.staged {
        let res = crate::utils::safe_join(base, rel).and_then(|dest| {
            crate::utils::make_replaceable(&dest);
            std::fs::rename(tmp, &dest).map_err(|e| e.to_string())
        });
        match res {
            Ok(()) if existing.contains(&crate::sync::diff::match_key(rel)) => changed.push((rel.clone(), history::REASON_REPLACED)),
            Ok(()) => {}
            Err(e) => {
                let _ = std::fs::remove_file(tmp);
                errors.push(format!("{rel}: {e}"));
            }
        }
    }
    // Only once everything arrived in place: a half-replaced save plus
    // deleted leftovers would be worse than two mixed versions.
    if errors.is_empty() {
        for rel in &delete {
            match crate::utils::safe_join(base, rel).and_then(|p| std::fs::remove_file(p).map_err(|e| e.to_string())) {
                Ok(()) => changed.push((rel.clone(), history::REASON_DELETED)),
                Err(e) => errors.push(format!("{rel}: {e}")),
            }
        }
    }
    if let Some(id) = capture {
        if let Err(e) = history::finish_local(&root, game, &id, &changed, crate::utils::timestamp_now()) {
            log::warn!("File history: {e}");
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

/// Whether a `FileHeader` from this friend is an upload their give asked for.
pub async fn expects_upload(state: &Arc<Mutex<AppState>>, peer_id: &str, path: &str, size: u64, hash: &str) -> bool {
    let st = state.lock().await;
    st.handoff_in
        .get(peer_id)
        .is_some_and(|p| p.needed.contains(path) && !p.staged.contains_key(path) && p.files.iter().any(|f| f.relative_path == path && f.size == size && f.hash == hash))
}

/// Receive one give upload into a temp file next to its destination (a name
/// scans skip) and answer like an offer upload.
pub async fn host_receive(state: &Arc<Mutex<AppState>>, s: &mut PeerStream, peer_id: &str, path: String, size: u64, hash: String) -> Message {
    let result = |ok: bool, message: &str| Message::OfferResult { path: path.clone(), ok, message: message.to_string() };
    let dest = { state.lock().await.active_game_path().and_then(|b| crate::utils::safe_join(&b, &path)) };
    let dest = match dest {
        Ok(d) => d,
        Err(_) => {
            transfer::drain_until_file_end(s, Some(size)).await;
            return result(false, "Invalid file path.");
        }
    };
    if let Some(parent) = dest.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            transfer::drain_until_file_end(s, Some(size)).await;
            return result(false, &e.to_string());
        }
    }
    let name = dest.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = dest.with_file_name(format!(".{name}.synccrate-handoff-{}.tmp", uuid::Uuid::new_v4()));
    match transfer::receive_file_body(s, &tmp, size, &hash, &path).await {
        Ok(()) => {
            let mut st = state.lock().await;
            match st.handoff_in.get_mut(peer_id) {
                Some(p) => {
                    p.staged.insert(path.clone(), tmp);
                    result(true, "")
                }
                None => {
                    let _ = std::fs::remove_file(&tmp);
                    result(false, "The handover was cancelled.")
                }
            }
        }
        Err((e, in_sync)) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            if !in_sync {
                transfer::drain_until_file_end(s, Some(size)).await;
            }
            result(false, &e)
        }
    }
}

// ---------------------------------------------------------------------------
// Client side

/// One exchange with the host, stream lock held by the caller. Returns the
/// host's records, its reply, and a message the loop must still act on (the
/// host's `Disconnect` or `GameInfoExchange`). A request waits long enough
/// for the host to hash the save; giving up marks the stream so later polls
/// allow for the answer still queued at the host.
pub async fn round_trip(s: &mut PeerStream, saves: Vec<CrewSaves>, request: Option<HandoffRequest>) -> Result<(Vec<CrewSaves>, Option<HandoffReply>, Option<Message>), String> {
    let id = rand::random::<u64>();
    let is_request = request.is_some();
    protocol::send_message(s, &Message::HandoffSync { id, saves, request }).await?;
    let mut pending = None;
    loop {
        let wait = if is_request { REQUEST_WAIT } else { transfer::poll_reply_wait(s) };
        let Some(msg) = protocol::try_recv_message(s, wait).await? else {
            s.mark_abandoned();
            return Err("The host stopped answering".into());
        };
        match msg {
            Message::HandoffStatus { id: got, saves, reply } if got == id => return Ok((saves, reply, pending)),
            // A late answer to an earlier exchange we gave up on.
            Message::HandoffStatus { .. } => {}
            m @ Message::Disconnect => return Ok((Vec::new(), None, Some(m))),
            m @ Message::GameInfoExchange { .. } => pending = Some(m),
            other => log::debug!("Handoff poll: ignoring {:?}", other),
        }
    }
}

/// `round_trip` from a command: a host that disconnected is an error here
/// (the message loop sees the closed connection and cleans up).
async fn command_trip(s: &mut PeerStream, saves: Vec<CrewSaves>, request: HandoffRequest) -> Result<(Vec<CrewSaves>, HandoffReply), String> {
    let (saves, reply, pending) = round_trip(s, saves, Some(request)).await?;
    if matches!(pending, Some(Message::Disconnect)) {
        return Err("The host disconnected.".into());
    }
    Ok((saves, reply.ok_or("The host didn't answer.")?))
}

/// The host we're connected to: (peer id, node id, stream).
async fn host_of(state: &Arc<Mutex<AppState>>) -> Result<(String, String, Arc<Mutex<PeerStream>>), String> {
    let st = state.lock().await;
    if st.session_type != crate::state::SessionType::Client {
        return Err("Join your friend's session first.".into());
    }
    if !st.handoff_available {
        return Err("The host's SyncCrate is too old for save handoff. Ask them to update.".into());
    }
    if !st.host_proven {
        return Err(NOT_PROVEN.into());
    }
    let (id, conn) = st.connections.iter().next().ok_or("Join your friend's session first.")?;
    let node = conn.info.node_id.clone().ok_or("The host's SyncCrate didn't say who it is. Ask them to update.")?;
    Ok((id.clone(), node, conn.stream.clone()))
}

/// Send one request (and our records) from a command; merges the host's
/// records and returns its reply.
pub async fn request(state: &Arc<Mutex<AppState>>, app: &Events, req: HandoffRequest) -> Result<HandoffReply, String> {
    let (peer_id, node, stream) = host_of(state).await?;
    // Read before taking the stream: elsewhere the AppState lock is taken
    // first, so taking it while holding the stream could deadlock.
    let out = records_for(&*state.lock().await, &node);
    let (saves, reply) = {
        let mut s = transfer::lock_stream(&stream, state, &peer_id, false).await?;
        command_trip(&mut s, out, req).await?
    };
    if merge_from(&mut *state.lock().await, &node, saves) {
        let _ = app.emit("handoff-updated", serde_json::json!({}));
    }
    Ok(reply)
}

/// Hand our copy of a save to the host: tell it the files, upload the ones
/// it asks for, then commit. The stream stays ours throughout, like a file
/// download.
pub async fn give(state: &Arc<Mutex<AppState>>, app: &Events, crew: &str, game: &str, unit: &str) -> Result<(), String> {
    let (peer_id, node, stream) = host_of(state).await?;
    let (base, files) = fresh_files(state, unit).await?;
    if files.is_empty() {
        return Err("This save has no files on this PC.".into());
    }
    let out = records_for(&*state.lock().await, &node);
    let mut s = transfer::lock_stream(&stream, state, &peer_id, false).await?;
    let req = HandoffRequest::Give { crew: crew.into(), game: game.into(), unit: unit.into(), files: files.clone() };
    let paths = match command_trip(&mut s, out.clone(), req).await?.1 {
        HandoffReply::Upload { paths } => paths,
        HandoffReply::Refused { message } => return Err(message),
        _ => return Err("The host didn't answer.".into()),
    };
    for path in &paths {
        let f = files.iter().find(|f| &f.relative_path == path).ok_or("The host asked for a file that isn't part of the save.")?;
        match transfer::upload_offered_file(&mut s, &base, f).await? {
            Ok((true, _)) => {}
            Ok((false, message)) => return Err(format!("{}: {message}", f.relative_path)),
            Err(_) => return Err("The host stopped the handover.".into()),
        }
    }
    let commit = HandoffRequest::Commit { crew: crew.into(), game: game.into(), unit: unit.into() };
    let (saves, reply) = command_trip(&mut s, out, commit).await?;
    drop(s);
    if merge_from(&mut *state.lock().await, &node, saves) {
        let _ = app.emit("handoff-updated", serde_json::json!({}));
    }
    match reply {
        HandoffReply::Done => Ok(()),
        HandoffReply::Refused { message } => Err(message),
        _ => Err("The host didn't answer.".into()),
    }
}

/// Take a save: get the host's list, make our copy match it (downloads
/// replace only files still as we hashed them; our old versions go to File
/// history), check it, and only then ask the host to hand the save over. A
/// failure anywhere leaves the save with the host, so "Take it" works again.
pub async fn take(state: &Arc<Mutex<AppState>>, app: &Events, crew: &str, game: &str, unit: &str) -> Result<usize, String> {
    let files = match request(state, app, HandoffRequest::Take { crew: crew.into(), game: game.into(), unit: unit.into() }).await? {
        HandoffReply::Granted { files } => files,
        HandoffReply::Refused { message } => return Err(message),
        _ => return Err("The host didn't answer.".into()),
    };
    handoff::check_files(&content_types(state, game).await, unit, &files)?;
    let (peer_id, _, _) = host_of(state).await?;
    // Our copy is replaced now: no restore, undo or sync alongside (they all
    // check this flag), and a Cancel left over from an earlier sync must not
    // stop the downloads.
    let _replacing = crate::commands::backup::try_begin_restoring().ok_or("A restore is running. Wait for it to finish, then take the save.")?;
    crate::commands::sync::CANCEL_SYNC.store(false, std::sync::atomic::Ordering::SeqCst);
    let (base, ours) = fresh_files(state, unit).await?;
    let (fetch, delete) = handoff::diff_save(&ours, &files);
    let by_key: std::collections::HashMap<String, &FileInfo> = ours.iter().map(|f| (crate::sync::diff::match_key(&f.relative_path), f)).collect();
    let mut targets: Vec<String> = fetch.iter().filter_map(|f| by_key.get(&crate::sync::diff::match_key(&f.relative_path)).map(|o| o.relative_path.clone())).collect();
    targets.extend(delete.iter().cloned());
    use crate::commands::history;
    let root = crate::utils::backups_dir();
    let now = crate::utils::timestamp_now();
    let capture = (!targets.is_empty() && crate::commands::sync::read_sync_config().keep_file_history).then(|| {
        let id = uuid::Uuid::new_v4().to_string();
        if let Err(e) = history::begin_capture(&root, game, &base, &targets, "", &id, now) {
            log::warn!("File history: couldn't keep your old save: {e}");
        }
        id
    });
    let mut changed: Vec<(String, &str)> = Vec::new();
    let mut result = Ok(());
    for f in &fetch {
        let local = by_key.get(&crate::sync::diff::match_key(&f.relative_path));
        let policy = match local {
            Some(o) => transfer::ReplacePolicy::ReplaceIfHash(o.hash.clone()),
            None => transfer::ReplacePolicy::MustNotExist,
        };
        let local_path = local.map_or(f.relative_path.as_str(), |o| o.relative_path.as_str());
        let req = transfer::ReceiveRequest {
            remote_path: &f.relative_path,
            local_path,
            expected_hash: &f.hash,
            expected_size: Some(f.size),
            policy,
            modified_secs: (f.modified > 0).then_some(f.modified),
        };
        match transfer::receive_file(state, &peer_id, &base, req).await {
            Ok(_) => {
                if local.is_some() {
                    changed.push((local_path.to_string(), history::REASON_REPLACED));
                }
            }
            Err(e) => {
                result = Err(format!("{}: {e}", f.relative_path));
                break;
            }
        }
    }
    // Leftovers only go once the new copy is complete.
    if result.is_ok() {
        for rel in &delete {
            match crate::utils::safe_join(&base, rel).and_then(|p| std::fs::remove_file(p).map_err(|e| e.to_string())) {
                Ok(()) => changed.push((rel.clone(), history::REASON_DELETED)),
                Err(e) => result = Err(format!("Couldn't remove {rel} from your old copy: {e}")),
            }
        }
    }
    if let Some(id) = capture {
        if let Err(e) = history::finish_local(&root, game, &id, &changed, crate::utils::timestamp_now()) {
            log::warn!("File history: {e}");
        }
    }
    let _ = crate::commands::files::scan_files_inner(state, None, true).await;
    result.map_err(|e| format!("{e}. The save stays with the host; take it again."))?;
    // Our copy must now be exactly the host's before it's ours.
    let (_, now_ours) = fresh_files(state, unit).await?;
    if !handoff::same_save(&now_ours, &files) {
        return Err("Your copy doesn't match the host's after downloading. The save stays with the host; take it again.".into());
    }
    match request(state, app, HandoffRequest::TakeDone { crew: crew.into(), game: game.into(), unit: unit.into(), files }).await? {
        HandoffReply::Done => Ok(fetch.len()),
        HandoffReply::Refused { message } => Err(message),
        _ => Err("The host didn't answer.".into()),
    }
}

/// Staged uploads a host left behind (it crashed or was closed before a
/// commit): scans hide them, so nothing else would ever remove them. Run
/// when hosting starts, before any give can be staged.
pub fn sweep_staging(base: &str, cts: &[ContentType]) -> usize {
    let mut removed = 0;
    for ct in cts.iter().filter(|c| c.save_unit_depth.is_some()) {
        let Some(dir) = crate::utils::ct_dir(base, ct) else { continue };
        for e in walkdir::WalkDir::new(&dir).follow_links(false).max_depth(8).into_iter().flatten() {
            let name = e.file_name().to_string_lossy();
            if e.file_type().is_file() && name.contains(".synccrate-handoff-") && name.ends_with(".tmp") && std::fs::remove_file(e.path()).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// The idle loop's poll input: our records, if the host shares a crew with us.
pub async fn poll_input(state: &Arc<Mutex<AppState>>, peer_id: &str) -> Option<(String, Vec<CrewSaves>)> {
    let st = state.lock().await;
    if !st.host_proven || !st.handoff_available {
        return None;
    }
    let node = st.connections.get(peer_id)?.info.node_id.clone()?;
    let out = records_for(&st, &node);
    (!out.is_empty()).then_some((node, out))
}

/// Apply the host's records from an idle poll.
pub async fn poll_apply(state: &Arc<Mutex<AppState>>, app: &Events, node: &str, saves: Vec<CrewSaves>) {
    if merge_from(&mut *state.lock().await, node, saves) {
        let _ = app.emit("handoff-updated", serde_json::json!({}));
    }
}
