//! Save handoff on the wire (`crate::handoff` has the rules). The client
//! starts every exchange: a `HandoffSync` poll carries both sides' records,
//! and a take / give rides on it. Uploads for a give reuse the offer upload
//! (`FileHeader` + chunks, answered by `OfferResult`), but land in temp files
//! next to their destination and only replace the host's save on `Commit`,
//! all together, with the old files kept in File history.
use crate::event_sink::Events;
use crate::handoff::{self, CrewSaves, HandoffReply, HandoffRequest, PendingGive, SharedSave};
use crate::network::protocol::{self, Message};
use crate::network::stream::PeerStream;
use crate::network::transfer;
use crate::state::{AppState, FileInfo};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Why handoff isn't offered on a LAN connection.
pub const NOT_PROVEN: &str = "Save handoff works when you join through your crew (Crews, then Join): that connection proves who's who. A LAN connection can't.";

/// How often an idle client exchanges records with the host.
pub const POLL: std::time::Duration = std::time::Duration::from_secs(5);

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
            changed |= handoff::receive(&mut crew.saves, cs.saves, |g| known.contains(g), now);
        }
    }
    if changed {
        crate::crews::persist(st);
    }
    changed
}

/// Replace one record (a local change: take, give, playing, share).
pub fn put(st: &mut AppState, crew: &str, next: SharedSave) -> Result<(), String> {
    let c = st.crews.get_mut(crew).ok_or("That crew is gone.")?;
    match c.saves.iter().position(|s| s.is(&next.game, &next.unit)) {
        Some(i) => c.saves[i] = next,
        None if c.saves.len() < handoff::MAX_SAVES_PER_CREW => c.saves.push(next),
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
    if let Some(p) = st.handoff_in.remove(peer_id) {
        for tmp in p.staged.values() {
            let _ = std::fs::remove_file(tmp);
        }
    }
}

/// The save's files on this PC, hashed now (a quick scan leaves hashes
/// empty, and a stale hash would hand over the wrong version).
pub(crate) async fn fresh_files(state: &Arc<Mutex<AppState>>, unit: &str) -> Result<(String, Vec<FileInfo>), String> {
    let (base, files) = {
        let st = state.lock().await;
        let cts = crate::commands::files::get_game_def(&st.game_registry, &st.active_game).map(|g| g.content_types.clone()).unwrap_or_default();
        (st.active_game_path()?, handoff::files_of(&cts, st.local_manifest.files.values(), unit))
    };
    let b = base.clone();
    let files = tokio::task::spawn_blocking(move || -> Result<Vec<FileInfo>, String> {
        files
            .into_iter()
            .filter(|f| !crate::sync::diff::is_disabled_path(&f.relative_path))
            .map(|mut f| {
                let path = crate::utils::safe_join(&b, &f.relative_path)?;
                let meta = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", f.relative_path))?;
                f.size = meta.len();
                f.hash = crate::commands::files::compute_file_hash(&path)?;
                Ok(f)
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

fn refused(message: impl Into<String>) -> HandoffReply {
    HandoffReply::Refused { message: message.into() }
}

// ---------------------------------------------------------------------------
// Host side

/// One `HandoffSync` from a friend: merge their records for crews we're
/// both in, answer the request, reply with our records.
/// `proven`: the friend's node id as the iroh connection proved it (None
/// over LAN TCP, where handoff isn't offered).
pub async fn host_sync(state: &Arc<Mutex<AppState>>, app: &Events, peer_id: &str, proven: Option<String>, saves: Vec<CrewSaves>, request: Option<HandoffRequest>) -> Message {
    let (node, peer_name) = {
        let mut st = state.lock().await;
        let Some((node, name)) = proven.and_then(|n| Some((n, st.connections.get(peer_id)?.info.name.clone()))) else {
            let reply = request.map(|_| refused(NOT_PROVEN));
            return Message::HandoffStatus { saves: Vec::new(), reply };
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
    Message::HandoffStatus { saves, reply }
}

async fn host_request(state: &Arc<Mutex<AppState>>, app: &Events, peer_id: &str, node: &str, peer_name: &str, r: HandoffRequest) -> HandoffReply {
    let (crew, game, unit) = match &r {
        HandoffRequest::Take { crew, game, unit } | HandoffRequest::Give { crew, game, unit, .. } | HandoffRequest::Commit { crew, game, unit } => (crew.clone(), game.clone(), unit.clone()),
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
            // Also when the friend already holds it: their download failed
            // and they're fetching it again.
            if current.holder != me && current.holder != node {
                return refused(format!("{} has this save, not the host.", current.holder_name));
            }
            if current.holder == me && current.playing {
                return refused(format!("{my_name} is playing it right now."));
            }
            if game_running(state).await {
                return refused("The host has the game open. Ask them to close it first, so the save is complete.");
            }
            let files = match fresh_files(state, &unit).await {
                Ok((_, f)) if !f.is_empty() => f,
                Ok(_) => return refused("The host doesn't have this save's files."),
                Err(e) => return refused(format!("The host couldn't read the save: {e}")),
            };
            let mut st = state.lock().await;
            // It may have changed while we hashed.
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
            HandoffReply::Granted { files }
        }
        HandoffRequest::Give { files, .. } => {
            if current.holder != node {
                return refused(format!("{} has this save, so only they can hand it over.", current.holder_name));
            }
            if game_running(state).await {
                return refused("The host has the game open. Ask them to close it first.");
            }
            let cts = {
                let st = state.lock().await;
                crate::commands::files::get_game_def(&st.game_registry, &game).map(|g| g.content_types.clone()).unwrap_or_default()
            };
            let files = match handoff::check_give(&cts, &unit, files) {
                Ok(f) => f,
                Err(e) => return refused(e),
            };
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
            if current.holder != node {
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
/// host's records, its reply, and a message the loop must still act on.
pub async fn round_trip(s: &mut PeerStream, saves: Vec<CrewSaves>, request: Option<HandoffRequest>) -> Result<(Vec<CrewSaves>, Option<HandoffReply>, Option<Message>), String> {
    protocol::send_message(s, &Message::HandoffSync { saves, request }).await?;
    let mut pending = None;
    loop {
        let wait = transfer::poll_reply_wait(s);
        let msg = protocol::try_recv_message(s, wait).await?.ok_or_else(|| "The host stopped answering".to_string())?;
        match msg {
            Message::HandoffStatus { saves, reply } => return Ok((saves, reply, pending)),
            Message::Disconnect => return Err("The host disconnected.".into()),
            m @ Message::GameInfoExchange { .. } => pending = Some(m),
            other => log::debug!("Handoff poll: ignoring {:?}", other),
        }
    }
}

/// The host we're connected to: (peer id, node id).
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
    let out = records_for(&*state.lock().await, &node);
    let (saves, reply) = {
        let mut s = transfer::lock_stream(&stream, state, &peer_id, false).await?;
        let (saves, reply, _) = round_trip(&mut s, out, Some(req)).await?;
        (saves, reply)
    };
    if merge_from(&mut *state.lock().await, &node, saves) {
        let _ = app.emit("handoff-updated", serde_json::json!({}));
    }
    reply.ok_or_else(|| "The host didn't answer.".into())
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
    // Read before taking the stream: elsewhere the AppState lock is taken
    // first, so taking it while holding the stream could deadlock.
    let out = records_for(&*state.lock().await, &node);
    let mut s = transfer::lock_stream(&stream, state, &peer_id, false).await?;
    let req = HandoffRequest::Give { crew: crew.into(), game: game.into(), unit: unit.into(), files: files.clone() };
    let (_, reply, _) = round_trip(&mut s, out.clone(), Some(req)).await?;
    let paths = match reply {
        Some(HandoffReply::Upload { paths }) => paths,
        Some(HandoffReply::Refused { message }) => return Err(message),
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
    let (saves, reply, _) = round_trip(&mut s, out, Some(commit)).await?;
    drop(s);
    if merge_from(&mut *state.lock().await, &node, saves) {
        let _ = app.emit("handoff-updated", serde_json::json!({}));
    }
    match reply {
        Some(HandoffReply::Done) => Ok(()),
        Some(HandoffReply::Refused { message }) => Err(message),
        _ => Err("The host didn't answer.".into()),
    }
}

/// The client's side of taking a save: after `Granted`, make our copy
/// match the host's (downloads replace only files still as we hashed them;
/// our old versions go to File history).
pub async fn take(state: &Arc<Mutex<AppState>>, app: &Events, crew: &str, game: &str, unit: &str) -> Result<usize, String> {
    let files = match request(state, app, HandoffRequest::Take { crew: crew.into(), game: game.into(), unit: unit.into() }).await? {
        HandoffReply::Granted { files } => files,
        HandoffReply::Refused { message } => return Err(message),
        _ => return Err("The host didn't answer.".into()),
    };
    let (peer_id, _, _) = host_of(state).await?;
    let (base, ours) = fresh_files(state, unit).await?;
    let (fetch, delete) = handoff::diff_save(&ours, &files);
    let cts = {
        let st = state.lock().await;
        crate::commands::files::get_game_def(&st.game_registry, game).map(|g| g.content_types.clone()).unwrap_or_default()
    };
    // The host's list is checked like a give: only this save's files, safe
    // paths, real hashes.
    handoff::check_give(&cts, unit, files.clone())?;
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
                Err(e) => log::warn!("Couldn't remove {rel} from the old save: {e}"),
            }
        }
    }
    if let Some(id) = capture {
        if let Err(e) = history::finish_local(&root, game, &id, &changed, crate::utils::timestamp_now()) {
            log::warn!("File history: {e}");
        }
    }
    let _ = crate::commands::files::scan_files_inner(state, None, true).await;
    result.map(|()| fetch.len())
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
