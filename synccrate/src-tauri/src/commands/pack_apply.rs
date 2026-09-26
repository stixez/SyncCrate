//! "Apply pack exactly": make the game's mods folder match a pack. Missing
//! and differing pack files still go through the ordinary pack sync
//! (`modpack::compute_pack_sync_plan` → `execute_sync`, conflicts included);
//! this module owns only the rename step afterwards: re-enable pack files
//! that sit locally as disabled copies with the pack's exact content, and
//! disable local mods the pack doesn't list, using the game's own disable
//! method via `files::toggle_file` (the same move `toggle_mod` makes).
//! Nothing is ever deleted or overwritten.
//!
//! Scope is the game's first content type only (the "mods" folder, as the
//! duplicate finder, profiles and `toggle_mod` define it), and only when the
//! pack actually has files there: a saves-only pack must never disable every
//! mod. Saves, trays, screenshots etc. are never renamed.
//!
//! Every rename is written to a per-game apply record (old path, new path,
//! size, mtime) so "Revert pack apply" can undo exactly those moves. Only the
//! last apply per game is kept. Downloads stay with "undo last sync" — its
//! record and presync backup already cover them, and it needs a disconnected
//! client while this step runs right after a sync, still connected.
use crate::commands::files::{self, get_game_def, missing_folder_error, resolve_game};
use crate::commands::modpack::{compare_pack_to_local, validate_pack, PackFileStatus};
use crate::commands::{backup, game_state};
use crate::registry::ContentType;
use crate::state::{AppState, FileManifest, ModPack, PackFile, SessionType};
use crate::sync::diff::{content_type_for, is_disabled_path, match_key};
use crate::utils;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApplyItem {
    pub relative_path: String,
    pub size: u64,
    /// Stat'ed at preview time; the apply refuses a file whose size or mtime
    /// changed since, so what the user confirmed is what gets renamed.
    pub mtime_ms: i64,
    pub hash: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SkippedFile {
    pub relative_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PackApplyPreview {
    pub game_id: String,
    pub base_path: String,
    pub pack_name: String,
    pub wrong_game: bool,
    /// False for `disable_method: "none"` games and packs without mods;
    /// `unavailable_reason` says why. Plain "get missing files" still works.
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub to_download: Vec<PackFileStatus>,
    pub conflicts: Vec<PackFileStatus>,
    pub to_enable: Vec<ApplyItem>,
    pub to_disable: Vec<ApplyItem>,
    /// Disabled pack files that can't be re-enabled because an enabled copy
    /// with other content holds the name. Shown, never touched.
    pub blocked: Vec<SkippedFile>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PackApplyResult {
    pub enabled: usize,
    pub disabled: usize,
    pub skipped: Vec<SkippedFile>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum MoveKind {
    Disabled,
    Enabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct MoveRecord {
    pub(crate) kind: MoveKind,
    pub(crate) old_path: String,
    pub(crate) new_path: String,
    pub(crate) size: u64,
    pub(crate) mtime_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ApplyRecord {
    pub(crate) game: String,
    pub(crate) base_path: String,
    pub(crate) created_at: u64,
    pub(crate) pack_name: String,
    pub(crate) moves: Vec<MoveRecord>,
}

// --- Plan (pure) ---

#[derive(Debug, Default)]
pub(crate) struct RenamePlan {
    pub(crate) to_enable: Vec<ApplyItem>,
    pub(crate) to_disable: Vec<ApplyItem>,
    pub(crate) blocked: Vec<SkippedFile>,
}

fn in_mods_scope(cts: &[ContentType], mods: &ContentType, rel: &str) -> bool {
    content_type_for(cts, rel).is_some_and(|(ct, _)| ct.id == mods.id)
}

/// Whether any pack file belongs to the mods content type. Without one there
/// is no "exact" mod set to match, only other content.
pub(crate) fn pack_has_mods(pack_files: &[PackFile], cts: &[ContentType], mods: &ContentType) -> bool {
    pack_files.iter().any(|pf| in_mods_scope(cts, mods, &pf.relative_path))
}

/// Which local mods to re-enable and which to disable so the mods folder
/// loads exactly the pack. Paths match the way the sync diff does
/// (`match_key`: case-insensitive, `.disabled`/`_Disabled` ignored), and
/// only files of the first content type are ever considered. `local` must
/// be a hashed scan. `mtime_ms` is left 0 here; the caller stats the files.
pub(crate) fn plan_renames(
    pack_files: &[PackFile],
    local: &FileManifest,
    cts: &[ContentType],
    mods: &ContentType,
) -> RenamePlan {
    let pack_hash: HashMap<String, &str> =
        pack_files.iter().map(|pf| (match_key(&pf.relative_path), pf.hash.as_str())).collect();

    // BTreeMap + sorted groups: a stable order for the preview and so the
    // same copy wins every time when two disabled copies both match.
    let mut by_key: BTreeMap<String, Vec<&crate::state::FileInfo>> = BTreeMap::new();
    for info in local.files.values() {
        if in_mods_scope(cts, mods, &info.relative_path) {
            by_key.entry(match_key(&info.relative_path)).or_default().push(info);
        }
    }

    let item = |f: &crate::state::FileInfo| ApplyItem {
        relative_path: f.relative_path.clone(),
        size: f.size,
        mtime_ms: 0,
        hash: f.hash.clone(),
        content_type: Some(mods.id.clone()),
    };

    let mut plan = RenamePlan::default();
    for (key, mut files) in by_key {
        files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
        let (disabled, enabled): (Vec<_>, Vec<_>) = files.into_iter().partition(|f| is_disabled_path(&f.relative_path));
        match pack_hash.get(&key) {
            Some(want) => {
                if want.is_empty() || enabled.iter().any(|f| f.hash == *want) {
                    continue;
                }
                let Some(copy) = disabled.iter().find(|f| f.hash == *want) else {
                    // Missing or different: the pack sync handles it.
                    continue;
                };
                if enabled.is_empty() {
                    plan.to_enable.push(item(copy));
                } else {
                    plan.blocked.push(SkippedFile {
                        relative_path: copy.relative_path.clone(),
                        reason: format!("an enabled copy with different content is in the way ({})", enabled[0].relative_path),
                    });
                }
            }
            None => plan.to_disable.extend(enabled.into_iter().map(item)),
        }
    }
    plan
}

// --- File checks and moves (blocking) ---

/// `path` is still the regular file (never a symlink) that was recorded:
/// same size and mtime. Used right before every rename, apply and revert.
pub(crate) fn file_matches(path: &Path, size: u64, mtime_ms: i64) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "it's no longer there".to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("it's no longer a regular file".into());
    }
    if meta.len() != size || backup::mtime_ms(&meta) != Some(mtime_ms) {
        return Err("it changed since the preview".into());
    }
    Ok(())
}

fn stat_mtime(base: &str, rel: &str) -> Option<i64> {
    let meta = std::fs::symlink_metadata(utils::safe_join(base, rel).ok()?).ok()?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return None;
    }
    backup::mtime_ms(&meta)
}

fn move_one(
    base: &str,
    mods_folder: &str,
    rename_method: bool,
    item: &ApplyItem,
    enable: bool,
) -> Result<MoveRecord, String> {
    let full = utils::safe_join(base, &item.relative_path)?;
    file_matches(&full, item.size, item.mtime_ms)?;
    if enable && files::compute_file_hash(&full)? != item.hash {
        return Err("its content no longer matches the pack".into());
    }
    let new_path = files::toggle_file(base, mods_folder, &item.relative_path, enable, rename_method)?
        .ok_or_else(|| if enable { "it's already enabled" } else { "it's already disabled" }.to_string())?;
    Ok(MoveRecord {
        kind: if enable { MoveKind::Enabled } else { MoveKind::Disabled },
        old_path: item.relative_path.replace('\\', "/"),
        new_path,
        size: item.size,
        mtime_ms: item.mtime_ms,
    })
}

/// Re-enable, then disable, each file after re-checking it. A file that
/// changed, vanished or whose target name is taken is skipped and reported.
pub(crate) fn apply_moves(
    base: &str,
    mods_folder: &str,
    rename_method: bool,
    to_enable: &[ApplyItem],
    to_disable: &[ApplyItem],
) -> (Vec<MoveRecord>, Vec<SkippedFile>) {
    let mut moves = Vec::new();
    let mut skipped = Vec::new();
    let work = to_enable.iter().map(|i| (i, true)).chain(to_disable.iter().map(|i| (i, false)));
    for (item, enable) in work {
        match move_one(base, mods_folder, rename_method, item, enable) {
            Ok(m) => moves.push(m),
            Err(reason) => skipped.push(SkippedFile { relative_path: item.relative_path.clone(), reason }),
        }
    }
    (moves, skipped)
}

#[derive(Debug, Default, Serialize)]
pub struct RevertResult {
    pub reverted: usize,
    pub skipped: Vec<SkippedFile>,
}

/// Undo the recorded moves, newest first. Each file must still sit at its
/// new path unchanged, and its old path must be free; otherwise it's
/// skipped (the user moved, edited or replaced it since).
pub(crate) fn revert_moves(base: &str, moves: &[MoveRecord]) -> (RevertResult, Vec<MoveRecord>) {
    let mut result = RevertResult::default();
    let mut done = Vec::new();
    for m in moves.iter().rev() {
        let attempt = (|| -> Result<(), String> {
            let from = utils::safe_join(base, &m.new_path)?;
            let to = utils::safe_join(base, &m.old_path)?;
            file_matches(&from, m.size, m.mtime_ms).map_err(|e| e.replace("since the preview", "since the apply"))?;
            if std::fs::symlink_metadata(&to).is_ok() {
                return Err(format!("{} exists again, so it wasn't overwritten", m.old_path));
            }
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            std::fs::rename(&from, &to).map_err(|e| e.to_string())
        })();
        match attempt {
            Ok(()) => {
                result.reverted += 1;
                done.push(m.clone());
            }
            Err(reason) => result.skipped.push(SkippedFile { relative_path: m.new_path.clone(), reason }),
        }
    }
    (result, done)
}

// --- Apply record ---

fn records_dir() -> PathBuf {
    let dir = utils::config_root().join("synccrate").join("pack_apply_records");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Same defensive filename filter as `undo::record_path`.
fn record_path(game: &str) -> PathBuf {
    let safe: String = game.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    records_dir().join(format!("{}.json", if safe.is_empty() { "unknown".to_string() } else { safe }))
}

pub(crate) fn read_record(game: &str) -> Option<ApplyRecord> {
    serde_json::from_str(&std::fs::read_to_string(record_path(game)).ok()?).ok()
}

pub(crate) fn write_record(record: &ApplyRecord) {
    let path = record_path(&record.game);
    let Ok(data) = serde_json::to_string_pretty(record) else { return };
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub(crate) fn delete_record(game: &str) {
    let _ = std::fs::remove_file(record_path(game));
}

// --- Commands ---

struct GameCtx {
    game_id: String,
    label: String,
    base_path: String,
    cts: Vec<ContentType>,
    mods: Option<ContentType>,
    rename_method: bool,
    disable_none: bool,
    process_names: Vec<String>,
}

fn game_ctx(app_state: &AppState, game_id: &str) -> Result<GameCtx, String> {
    let label = app_state.game_label(game_id);
    let base_path = app_state.game_paths.get(game_id).cloned().ok_or_else(|| format!("{} path not set.", label))?;
    let def = get_game_def(&app_state.game_registry, game_id);
    let method = def.and_then(|d| d.disable_method.as_deref());
    let cts = def.map(|d| d.content_types.clone()).unwrap_or_default();
    Ok(GameCtx {
        game_id: game_id.to_string(),
        label,
        base_path,
        mods: cts.first().cloned(),
        cts,
        rename_method: method == Some("rename"),
        disable_none: method == Some("none"),
        process_names: def.map(|d| d.process_names.clone()).unwrap_or_default(),
    })
}

/// The undo/restore guards, except that a *client* session is allowed: the
/// rename step runs right after the pack's download, still connected. A
/// host is refused — friends may be pulling from this folder.
fn check_guards(app_state: &AppState) -> Result<(), String> {
    if app_state.is_any_syncing() {
        return Err("Can't change mods while a sync is in progress.".into());
    }
    if app_state.session_type == SessionType::Host {
        return Err("Stop hosting first — friends may be pulling from this folder right now.".into());
    }
    if backup::restore_in_progress() {
        return Err("A restore is running — wait for it to finish.".into());
    }
    Ok(())
}

fn pack_is_for(app_state: &AppState, pack: &ModPack, game_id: &str) -> bool {
    pack.game_id == game_id || resolve_game(app_state, &pack.game_id).ok().as_deref() == Some(game_id)
}

#[tauri::command]
pub async fn preview_pack_apply(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    pack: ModPack,
) -> Result<PackApplyPreview, String> {
    preview_pack_apply_inner(state.inner(), pack).await
}

pub(crate) async fn preview_pack_apply_inner(state: &Arc<Mutex<AppState>>, pack: ModPack) -> Result<PackApplyPreview, String> {
    validate_pack(&pack)?;
    let ctx = {
        let app_state = state.lock().await;
        let active = app_state.active_game.clone();
        if !pack_is_for(&app_state, &pack, &active) {
            return Ok(PackApplyPreview { pack_name: pack.name, game_id: pack.game_id, wrong_game: true, ..Default::default() });
        }
        game_ctx(&app_state, &active)?
    };
    if !Path::new(&ctx.base_path).is_dir() {
        return Err(missing_folder_error(&ctx.label, &ctx.base_path));
    }

    let manifest = files::scan_files_inner(state, Some(ctx.game_id.clone()), true).await?;
    let cmp = compare_pack_to_local(&pack, &manifest, &ctx.cts);
    let mut preview = PackApplyPreview {
        game_id: ctx.game_id.clone(),
        base_path: ctx.base_path.clone(),
        pack_name: pack.name.clone(),
        to_download: cmp.missing,
        conflicts: cmp.different,
        ..Default::default()
    };

    let mods = ctx.mods.as_ref().filter(|m| pack_has_mods(&pack.files, &ctx.cts, m));
    if ctx.disable_none {
        preview.unavailable_reason = Some(files::disable_unsupported_message(&ctx.label));
    } else if let Some(mods) = mods {
        let plan = plan_renames(&pack.files, &manifest, &ctx.cts, mods);
        let stat = |mut items: Vec<ApplyItem>| -> Vec<ApplyItem> {
            items.retain_mut(|i| stat_mtime(&ctx.base_path, &i.relative_path).map(|m| i.mtime_ms = m).is_some());
            items
        };
        preview.to_enable = stat(plan.to_enable);
        preview.to_disable = stat(plan.to_disable);
        preview.blocked = plan.blocked;
        preview.available = true;
    } else {
        let what = ctx.mods.as_ref().map(|m| m.label.clone()).unwrap_or_else(|| "mods".into());
        preview.unavailable_reason = Some(format!(
            "This pack has no {} in it, so there's no exact set to match — Get Missing Files still works.",
            what.to_lowercase()
        ));
    }
    Ok(preview)
}

/// The rename step of "apply pack exactly", after the pack's download (if
/// any) finished. Refuses while pack files are still missing, so a failed or
/// partial download never leaves the user with their mods disabled and the
/// pack's not there.
#[tauri::command]
pub async fn apply_pack_exact(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    pack: ModPack,
    preview: PackApplyPreview,
) -> Result<PackApplyResult, String> {
    apply_pack_exact_inner(state.inner(), pack, preview).await
}

pub(crate) async fn apply_pack_exact_inner(
    state: &Arc<Mutex<AppState>>,
    pack: ModPack,
    preview: PackApplyPreview,
) -> Result<PackApplyResult, String> {
    validate_pack(&pack)?;
    let ctx = {
        let app_state = state.lock().await;
        check_guards(&app_state)?;
        let active = app_state.active_game.clone();
        let ctx = game_ctx(&app_state, &active)?;
        if preview.game_id != active || !pack_is_for(&app_state, &pack, &active) || preview.base_path != ctx.base_path {
            return Err("The game or its folder changed since the preview — preview the pack again.".into());
        }
        if ctx.disable_none {
            return Err(files::disable_unsupported_message(&ctx.label));
        }
        ctx
    };
    let Some(mods) = ctx.mods.clone().filter(|m| pack_has_mods(&pack.files, &ctx.cts, m)) else {
        return Err("This pack has no mods in it, so there's no exact set to match.".into());
    };
    if !Path::new(&ctx.base_path).is_dir() {
        return Err(missing_folder_error(&ctx.label, &ctx.base_path));
    }
    if game_state::is_game_running(&ctx.process_names).await.unwrap_or(false) {
        return Err(format!("{} is running. Close the game before applying a pack.", ctx.label));
    }

    let manifest = files::scan_files_inner(state, Some(ctx.game_id.clone()), true).await?;
    let cmp = compare_pack_to_local(&pack, &manifest, &ctx.cts);
    let missing = cmp.missing;
    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().take(3).map(|m| m.relative_path.as_str()).collect();
        return Err(format!(
            "{} pack file(s) are still missing ({}{}). Get them from a host first — nothing was disabled.",
            missing.len(),
            names.join(", "),
            if missing.len() > 3 { ", …" } else { "" }
        ));
    }
    // A conflict the user settled as "keep mine" leaves their version live,
    // and "keep both" adds the pack's copy next to it (the preview, made
    // before the sync, doesn't know that file): either way the result isn't
    // the pack, so don't report "Pack applied" over duplicate or wrong mods.
    // Mods only: this step only enables and disables mods, so a pack save the
    // user kept their own copy of is fine.
    let in_mods = |p: &str| crate::sync::diff::content_type_for(&ctx.cts, p).is_some_and(|(ct, _)| ct.id == mods.id);
    let different: Vec<_> = cmp.different.iter().filter(|m| in_mods(&m.relative_path)).collect();
    if !different.is_empty() {
        let names: Vec<&str> = different.iter().take(3).map(|m| m.relative_path.as_str()).collect();
        return Err(format!(
            "{} mod(s) aren't the pack's version ({}{}), usually because a conflict kept your copy. Resolve them with \"use theirs\" and apply again. Nothing was disabled.",
            different.len(),
            names.join(", "),
            if different.len() > 3 { ", …" } else { "" }
        ));
    }
    // The scan awaited; a sync or session may have started meanwhile.
    check_guards(&*state.lock().await)?;
    let Some(_guard) = backup::try_begin_restoring() else {
        return Err("A restore is already running.".into());
    };

    let (base, folder, rename) = (ctx.base_path.clone(), mods.folder.clone(), ctx.rename_method);
    let (to_enable, to_disable) = (preview.to_enable, preview.to_disable);
    let (moves, skipped) = tokio::task::spawn_blocking(move || apply_moves(&base, &folder, rename, &to_enable, &to_disable))
        .await
        .map_err(|e| e.to_string())?;

    for m in &moves {
        crate::commands::tags::move_tags(&ctx.game_id, &m.old_path, &m.new_path);
    }
    let result = PackApplyResult {
        enabled: moves.iter().filter(|m| m.kind == MoveKind::Enabled).count(),
        disabled: moves.iter().filter(|m| m.kind == MoveKind::Disabled).count(),
        skipped,
    };
    // An apply that moved nothing leaves the previous record revertable.
    if !moves.is_empty() {
        write_record(&ApplyRecord {
            game: ctx.game_id,
            base_path: ctx.base_path,
            created_at: utils::timestamp_now(),
            pack_name: pack.name,
            moves,
        });
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize)]
pub struct PackApplyStatus {
    pub created_at: u64,
    pub pack_name: String,
    pub disabled: usize,
    pub enabled: usize,
}

#[tauri::command]
pub async fn get_pack_apply_status(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<Option<PackApplyStatus>, String> {
    let (game_id, base_path) = {
        let app_state = state.lock().await;
        let id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        let base = app_state.game_paths.get(&id).cloned();
        (id, base)
    };
    Ok(read_record(&game_id).filter(|r| Some(&r.base_path) == base_path.as_ref()).map(|r| PackApplyStatus {
        created_at: r.created_at,
        pack_name: r.pack_name,
        disabled: r.moves.iter().filter(|m| m.kind == MoveKind::Disabled).count(),
        enabled: r.moves.iter().filter(|m| m.kind == MoveKind::Enabled).count(),
    }))
}

#[tauri::command]
pub async fn revert_pack_apply(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    game: Option<String>,
) -> Result<RevertResult, String> {
    revert_pack_apply_inner(state.inner(), game).await
}

pub(crate) async fn revert_pack_apply_inner(state: &Arc<Mutex<AppState>>, game: Option<String>) -> Result<RevertResult, String> {
    let ctx = {
        let app_state = state.lock().await;
        check_guards(&app_state)?;
        let id = match game {
            Some(ref g) => resolve_game(&app_state, g)?,
            None => app_state.active_game.clone(),
        };
        game_ctx(&app_state, &id)?
    };
    let Some(record) = read_record(&ctx.game_id) else {
        return Err("No pack apply to revert.".into());
    };
    if record.game != ctx.game_id || record.base_path != ctx.base_path {
        return Err("The game folder changed since that pack was applied — nothing to revert.".into());
    }
    if !Path::new(&ctx.base_path).is_dir() {
        return Err(missing_folder_error(&ctx.label, &ctx.base_path));
    }
    if game_state::is_game_running(&ctx.process_names).await.unwrap_or(false) {
        return Err(format!("{} is running. Close the game before reverting.", ctx.label));
    }
    let Some(_guard) = backup::try_begin_restoring() else {
        return Err("A restore is already running.".into());
    };

    let base = ctx.base_path.clone();
    let moves = record.moves.clone();
    let (result, done) = tokio::task::spawn_blocking(move || revert_moves(&base, &moves))
        .await
        .map_err(|e| e.to_string())?;
    for m in &done {
        crate::commands::tags::move_tags(&ctx.game_id, &m.new_path, &m.old_path);
    }
    delete_record(&ctx.game_id);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::FileInfo;

    fn sims4_cts() -> Vec<ContentType> {
        let reg = crate::registry::load_registry();
        get_game_def(&reg, "sims4").unwrap().content_types.clone()
    }

    fn manifest(entries: &[(&str, &str)]) -> FileManifest {
        let mut m = FileManifest::default();
        for (p, h) in entries {
            m.files.insert(
                p.to_string(),
                FileInfo { relative_path: p.to_string(), size: 1, hash: h.to_string(), modified: 0, file_type: "Mod".into() },
            );
        }
        m
    }

    fn pf(path: &str, hash: &str) -> PackFile {
        PackFile { relative_path: path.into(), size: 1, hash: hash.into() }
    }

    fn paths(items: &[ApplyItem]) -> Vec<&str> {
        items.iter().map(|i| i.relative_path.as_str()).collect()
    }

    #[test]
    fn plan_disables_extras_reenables_matching_disabled_copies_and_ignores_other_content() {
        let cts = sims4_cts();
        let pack = [pf("Mods/Keep.package", "k"), pf("Mods/CC/Hair.package", "h"), pf("Mods/Other.package", "o")];
        let local = manifest(&[
            // Case-only difference: still the pack's file, left alone.
            ("Mods/keep.PACKAGE", "k"),
            // Disabled copy with the pack's content, different case: re-enable.
            ("Mods/cc/hair.package.disabled", "h"),
            // Disabled copy with other content: a conflict for the sync, not a rename.
            ("Mods/Other.package.disabled", "x"),
            ("Mods/Extra.package", "e"),
            // Already disabled extras stay as they are.
            ("Mods/Old.package.disabled", "d"),
            ("Mods/_Disabled/Legacy.package", "l"),
            // Out of scope: saves and trays are never touched.
            ("Saves/Slot_1.save", "s"),
            ("Tray/household.trayitem", "t"),
        ]);
        let plan = plan_renames(&pack, &local, &cts, &cts[0]);
        assert_eq!(paths(&plan.to_enable), ["Mods/cc/hair.package.disabled"]);
        assert_eq!(paths(&plan.to_disable), ["Mods/Extra.package"]);
        assert!(plan.blocked.is_empty());
    }

    #[test]
    fn plan_reports_a_disabled_match_blocked_by_a_different_enabled_copy() {
        let cts = sims4_cts();
        let pack = [pf("Mods/a.package", "good")];
        let local = manifest(&[("Mods/a.package", "bad"), ("Mods/a.package.disabled", "good")]);
        let plan = plan_renames(&pack, &local, &cts, &cts[0]);
        assert!(plan.to_enable.is_empty() && plan.to_disable.is_empty());
        assert_eq!(plan.blocked.len(), 1);
        assert_eq!(plan.blocked[0].relative_path, "Mods/a.package.disabled");
    }

    #[test]
    fn plan_leaves_disabled_duplicates_when_the_enabled_copy_matches() {
        let cts = sims4_cts();
        let pack = [pf("Mods/a.package", "h")];
        let local = manifest(&[("Mods/a.package", "h"), ("Mods/a.package.disabled", "h")]);
        let plan = plan_renames(&pack, &local, &cts, &cts[0]);
        assert!(plan.to_enable.is_empty() && plan.to_disable.is_empty() && plan.blocked.is_empty());
    }

    #[test]
    fn a_pack_without_mods_has_no_exact_set() {
        let cts = sims4_cts();
        assert!(!pack_has_mods(&[pf("Saves/Slot_1.save", "s")], &cts, &cts[0]));
        assert!(pack_has_mods(&[pf("Saves/Slot_1.save", "s"), pf("mods/x.package", "x")], &cts, &cts[0]));
    }

    fn temp_base(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("synccrate_packapply_{}_{}", label, std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Mods")).unwrap();
        dir
    }

    fn write(base: &Path, rel: &str, data: &[u8]) {
        let p = base.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn item(base: &Path, rel: &str) -> ApplyItem {
        let meta = std::fs::metadata(base.join(rel)).unwrap();
        ApplyItem {
            relative_path: rel.into(),
            size: meta.len(),
            mtime_ms: backup::mtime_ms(&meta).unwrap(),
            hash: files::compute_file_hash(&base.join(rel)).unwrap(),
            content_type: None,
        }
    }

    #[test]
    fn rename_method_disables_with_suffix_and_never_overwrites() {
        let base = temp_base("rename");
        write(&base, "Mods/Creator/extra.package", b"E");
        write(&base, "Mods/taken.package", b"T");
        write(&base, "Mods/taken.package.disabled", b"OLD");
        write(&base, "Mods/pack.package.disabled", b"P");
        let b = base.to_string_lossy().to_string();
        let to_disable = [item(&base, "Mods/Creator/extra.package"), item(&base, "Mods/taken.package")];
        let to_enable = [item(&base, "Mods/pack.package.disabled")];
        let (moves, skipped) = apply_moves(&b, "Mods", true, &to_enable, &to_disable);

        assert_eq!(moves.len(), 2);
        assert_eq!(moves[0].kind, MoveKind::Enabled);
        assert_eq!(moves[0].new_path, "Mods/pack.package");
        assert_eq!(moves[1].new_path, "Mods/Creator/extra.package.disabled");
        assert_eq!(std::fs::read(base.join("Mods/Creator/extra.package.disabled")).unwrap(), b"E");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].relative_path, "Mods/taken.package");
        assert_eq!(std::fs::read(base.join("Mods/taken.package.disabled")).unwrap(), b"OLD", "never overwritten");
        assert!(base.join("Mods/taken.package").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn folder_method_moves_into_and_out_of_disabled_folder() {
        let base = temp_base("folder");
        write(&base, "Mods/Sub/extra.package", b"E");
        write(&base, "Mods/_Disabled/pack.package", b"P");
        let b = base.to_string_lossy().to_string();
        let (moves, skipped) = apply_moves(
            &b,
            "Mods",
            false,
            &[item(&base, "Mods/_Disabled/pack.package")],
            &[item(&base, "Mods/Sub/extra.package")],
        );
        assert!(skipped.is_empty(), "{skipped:?}");
        assert_eq!(moves[0].new_path, "Mods/pack.package");
        assert_eq!(moves[1].new_path, "Mods/_Disabled/Sub/extra.package");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn apply_rechecks_each_file_before_moving_it() {
        let base = temp_base("recheck");
        write(&base, "Mods/extra.package", b"E");
        write(&base, "Mods/pack.package.disabled", b"P");
        let b = base.to_string_lossy().to_string();
        let disable = item(&base, "Mods/extra.package");
        let mut enable = item(&base, "Mods/pack.package.disabled");
        enable.hash = "0".repeat(64); // content no longer the pack's
        write(&base, "Mods/extra.package", b"EDITED"); // size changed since the preview
        let (moves, skipped) = apply_moves(&b, "Mods", true, &[enable], &[disable]);
        assert!(moves.is_empty());
        assert_eq!(skipped.len(), 2);
        assert!(base.join("Mods/extra.package").exists() && base.join("Mods/pack.package.disabled").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn revert_undoes_moves_and_skips_files_moved_since() {
        let base = temp_base("revert");
        write(&base, "Mods/a.package", b"A");
        write(&base, "Mods/b.package", b"B");
        let b = base.to_string_lossy().to_string();
        let (moves, _) = apply_moves(&b, "Mods", true, &[], &[item(&base, "Mods/a.package"), item(&base, "Mods/b.package")]);
        assert_eq!(moves.len(), 2);
        std::fs::rename(base.join("Mods/b.package.disabled"), base.join("Mods/moved.package.disabled")).unwrap();

        let (result, done) = revert_moves(&b, &moves);
        assert_eq!(result.reverted, 1);
        assert_eq!(done[0].old_path, "Mods/a.package");
        assert_eq!(result.skipped.len(), 1);
        assert_eq!(result.skipped[0].relative_path, "Mods/b.package.disabled");
        assert_eq!(std::fs::read(base.join("Mods/a.package")).unwrap(), b"A");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn revert_never_overwrites_a_file_back_at_the_old_path() {
        let base = temp_base("revert-occupied");
        write(&base, "Mods/a.package", b"A");
        let b = base.to_string_lossy().to_string();
        let (moves, _) = apply_moves(&b, "Mods", true, &[], &[item(&base, "Mods/a.package")]);
        write(&base, "Mods/a.package", b"NEW");
        let (result, _) = revert_moves(&b, &moves);
        assert_eq!(result.reverted, 0);
        assert_eq!(std::fs::read(base.join("Mods/a.package")).unwrap(), b"NEW");
        assert!(base.join("Mods/a.package.disabled").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn file_matches_checks_size_mtime_and_existence() {
        let base = temp_base("matches");
        write(&base, "Mods/a.package", b"A");
        let i = item(&base, "Mods/a.package");
        let p = base.join("Mods/a.package");
        assert!(file_matches(&p, i.size, i.mtime_ms).is_ok());
        assert!(file_matches(&p, i.size + 1, i.mtime_ms).is_err());
        assert!(file_matches(&p, i.size, i.mtime_ms - 5000).is_err());
        assert!(file_matches(&base.join("Mods/nope.package"), 1, 0).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn record_roundtrips_and_keeps_only_the_last_apply() {
        let _g = crate::testutil::e2e_guard().await;
        let rec = |name: &str| ApplyRecord {
            game: "sims4".into(),
            base_path: "C:/Game".into(),
            created_at: 1,
            pack_name: name.into(),
            moves: vec![MoveRecord { kind: MoveKind::Disabled, old_path: "Mods/a.package".into(), new_path: "Mods/a.package.disabled".into(), size: 1, mtime_ms: 2 }],
        };
        write_record(&rec("first"));
        write_record(&rec("second"));
        let read = read_record("sims4").expect("record");
        assert_eq!(read.pack_name, "second");
        assert_eq!(read.moves, rec("second").moves);
        delete_record("sims4");
        assert!(read_record("sims4").is_none());
    }
}
