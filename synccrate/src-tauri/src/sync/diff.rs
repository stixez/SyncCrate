use crate::registry::ContentType;
use crate::state::{FileInfo, FileManifest, SyncAction, SyncPlan};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

/// Suffix rename-disable games (Sims 3/4) append to a disabled mod.
const DISABLED_SUFFIX: &str = ".disabled";
/// Folder older SyncCrate versions moved disabled mods into (`Mods/_Disabled/...`).
const LEGACY_DISABLED_DIR: &str = "_disabled";

/// Key used to decide whether a host file and a local file are "the same file".
/// Real paths are never rewritten; this is only for matching.
///
/// - Case-insensitive, and trailing dots/spaces per segment are ignored:
///   Windows treats `Mods/CC/Hair.package` and `Mods/cc/hair.package` as one
///   file, so matching them case-sensitively turned a changed file into a plain
///   "receive" that silently overwrote the local copy.
/// - A trailing `.disabled` and a legacy `_Disabled/` segment are dropped, so a
///   mod the user disabled locally matches the host's enabled copy instead of
///   being downloaded again next to it.
pub fn match_key(path: &str) -> String {
    let norm = path.replace('\\', "/");
    let mut segments: Vec<String> = norm
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .map(|s| s.trim_end_matches(['.', ' ']).to_lowercase())
        .collect();
    if let Some(last) = segments.last_mut() {
        if let Some(stripped) = last.strip_suffix(DISABLED_SUFFIX) {
            *last = stripped.trim_end_matches(['.', ' ']).to_string();
        }
    }
    // Only a directory segment can be the legacy folder, never the file itself.
    if segments.len() > 1 {
        if let Some(i) = segments[..segments.len() - 1]
            .iter()
            .position(|s| s == LEGACY_DISABLED_DIR)
        {
            segments.remove(i);
        }
    }
    segments.join("/")
}

/// Whether a path is a disabled mod (`x.package.disabled` or under `_Disabled/`).
pub fn is_disabled_path(path: &str) -> bool {
    let norm = path.replace('\\', "/").to_lowercase();
    let mut segments: Vec<&str> = norm.split('/').filter(|s| !s.is_empty()).collect();
    let Some(last) = segments.pop() else { return false };
    last.trim_end_matches(['.', ' ']).ends_with(DISABLED_SUFFIX)
        || segments.iter().any(|s| *s == LEGACY_DISABLED_DIR)
}

/// Whether some content type of the game accepts a (remote) relative path,
/// using the same rules as a scan: folder (depth per `recursive`), extension
/// list seen through `.disabled`, and `exclude_files`. `must_contain` can't be
/// checked without the file's bytes, so it's ignored here.
///
/// Clients use this to drop host files that don't belong to their game. Hosts
/// older than 0.5.6 don't say which game they share, so a Sims 4 host could
/// otherwise still push its Mods folder into an ETS2 client's folder.
pub fn path_accepted_by(content_types: &[ContentType], rel: &str) -> bool {
    content_type_for(content_types, rel).is_some()
}

/// The first content type accepting a game-folder-relative path (same rules
/// as `path_accepted_by`), with the path relative to that type's folder
/// ('/'-separated, original case). Backups store files per content type.
pub fn content_type_for<'a>(content_types: &'a [ContentType], rel: &str) -> Option<(&'a ContentType, String)> {
    let rel = rel.replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    let file_name = rel.rsplit('/').next().unwrap_or(rel);
    content_types.iter().find_map(|ct| {
        let folder = ct.folder.replace('\\', "/");
        let folder = folder.trim_end_matches('/').trim_start_matches("./");
        let rest = if folder.is_empty() || folder == "." {
            rel
        } else {
            let head = rel.get(..folder.len())?;
            let tail = rel.get(folder.len()..)?;
            if head.to_lowercase() != folder.to_lowercase() {
                return None;
            }
            tail.strip_prefix('/')?
        };
        if rest.is_empty() || (!ct.recursive && rest.contains('/')) {
            return None;
        }
        if ct.exclude_files.iter().any(|x| x.eq_ignore_ascii_case(file_name)) {
            return None;
        }
        if !ct.extensions.is_empty() {
            let ext = crate::commands::files::effective_extension(std::path::Path::new(rest));
            if !ct.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)) {
                return None;
            }
        }
        Some((ct, rest.to_string()))
    })
}

/// Remove host files no content type of the active game accepts. Returns how
/// many were dropped.
pub fn drop_foreign(remote: &mut FileManifest, content_types: &[ContentType]) -> usize {
    let before = remote.files.len();
    remote.files.retain(|path, _| path_accepted_by(content_types, path));
    before - remote.files.len()
}

/// Warning for the plan when an older host (no game id) seems to share a
/// different game: most of its files don't fit this game's folders.
pub fn foreign_warning(host_game: Option<&str>, skipped_foreign: usize, remote_total: usize) -> Option<String> {
    if skipped_foreign == 0 {
        return None;
    }
    let unknown_host = host_game.map_or(true, |g| g.is_empty());
    if unknown_host && skipped_foreign * 2 > remote_total {
        return Some(format!(
            "This host may be sharing a different game: {} of its {} files don't belong to this game's folders and were skipped. Nothing outside your game's folders will be synced.",
            skipped_foreign, remote_total
        ));
    }
    None
}

/// Pick the host file to compare when several host paths share one match key
/// (e.g. the host has both `x.package` and `x.package.disabled`): prefer the
/// enabled copy, then the lexicographically first path. Never both, so a key
/// can't produce two downloads into one local file.
fn pick_remote<'a>(candidates: &[&'a FileInfo]) -> &'a FileInfo {
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|a, b| {
        is_disabled_path(&a.relative_path)
            .cmp(&is_disabled_path(&b.relative_path))
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    sorted[0]
}

/// Local file to show in a conflict: exact path, then same disabled state,
/// then the lexicographically first.
fn pick_local<'a>(candidates: &[&'a FileInfo], remote: &FileInfo) -> &'a FileInfo {
    let remote_disabled = is_disabled_path(&remote.relative_path);
    let mut sorted = candidates.to_vec();
    sorted.sort_by(|a, b| {
        (a.relative_path != remote.relative_path)
            .cmp(&(b.relative_path != remote.relative_path))
            .then_with(|| {
                (is_disabled_path(&a.relative_path) != remote_disabled)
                    .cmp(&(is_disabled_path(&b.relative_path) != remote_disabled))
            })
            .then_with(|| a.relative_path.cmp(&b.relative_path))
    });
    sorted[0]
}

/// Compare manifests by `match_key`.
///
/// Disabled mods (Sims 3/4 rename-disable, legacy `_Disabled/`):
/// - Client has only a disabled copy with the host's content: in sync
///   ("disabled locally"); nothing is downloaded, the user's choice stays.
/// - Client's disabled copy differs from the host's: conflict. "Use theirs"
///   writes to the client's (disabled) path so it stays disabled.
/// - Host has it disabled, client enabled: no action ("disabled on host"),
///   whatever the content — the host isn't using that copy, and downloading
///   it would add a second, disabled twin next to the client's file.
pub fn compute_diff(local: &FileManifest, remote: &FileManifest) -> SyncPlan {
    let mut actions = Vec::new();
    let mut total_bytes = 0u64;
    let mut disabled_locally = 0usize;
    let mut disabled_on_host = 0usize;

    let mut local_by_key: HashMap<String, Vec<&FileInfo>> = HashMap::new();
    for info in local.files.values() {
        local_by_key.entry(match_key(&info.relative_path)).or_default().push(info);
    }
    // BTreeMap: deterministic action order.
    let mut remote_by_key: BTreeMap<String, Vec<&FileInfo>> = BTreeMap::new();
    for info in remote.files.values() {
        remote_by_key.entry(match_key(&info.relative_path)).or_default().push(info);
    }

    for (key, remote_candidates) in &remote_by_key {
        let remote_info = pick_remote(remote_candidates);
        let Some(local_candidates) = local_by_key.get(key) else {
            total_bytes += remote_info.size;
            actions.push(SyncAction::ReceiveFromRemote(remote_info.clone()));
            continue;
        };
        let remote_disabled = is_disabled_path(&remote_info.relative_path);
        let all_local_disabled = local_candidates.iter().all(|l| is_disabled_path(&l.relative_path));
        let any_local_enabled = !all_local_disabled;

        if local_candidates.iter().any(|l| l.hash == remote_info.hash && !l.hash.is_empty()) {
            if all_local_disabled && !remote_disabled {
                disabled_locally += 1;
            } else if remote_disabled && any_local_enabled {
                disabled_on_host += 1;
            }
            continue;
        }
        if remote_disabled && any_local_enabled {
            disabled_on_host += 1;
            continue;
        }
        let local_info = pick_local(local_candidates, remote_info);
        total_bytes += remote_info.size.max(local_info.size);
        actions.push(SyncAction::Conflict {
            local: local_info.clone(),
            remote: remote_info.clone(),
        });
    }

    // Files in local but not in remote → send
    let mut local_only: Vec<&FileInfo> = local
        .files
        .values()
        .filter(|l| !remote_by_key.contains_key(&match_key(&l.relative_path)))
        .collect();
    local_only.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    for local_info in local_only {
        total_bytes += local_info.size;
        actions.push(SyncAction::SendToRemote(local_info.clone()));
    }

    SyncPlan {
        actions,
        total_bytes,
        disabled_locally,
        disabled_on_host,
        ..Default::default()
    }
}

/// Diff a modpack's file list against the local manifest and a connected
/// host's remote manifest. Unlike `compute_diff`, only paths the pack lists
/// are ever considered: nothing outside it is uploaded, deleted, or even
/// looked at, so a pack sync can never touch a file it doesn't mention.
///
/// For each pack file: already matching locally (by hash, any local twin) →
/// skipped. Otherwise, only a host file at the same path with the pack's
/// *exact* hash counts — a host file at that path with different content
/// isn't "the pack's file" and is reported unavailable rather than treated
/// as a substitute. A brand new local path is a plain receive; an existing,
/// differently-hashed one is a conflict (same resolution UI as a normal sync).
pub fn compute_pack_plan(
    local: &FileManifest,
    remote: &FileManifest,
    pack_files: &[crate::state::PackFile],
) -> (SyncPlan, Vec<String>) {
    let mut actions = Vec::new();
    let mut total_bytes = 0u64;
    let mut unavailable = Vec::new();

    let mut local_by_key: HashMap<String, Vec<&FileInfo>> = HashMap::new();
    for info in local.files.values() {
        local_by_key.entry(match_key(&info.relative_path)).or_default().push(info);
    }
    let mut remote_by_key: HashMap<String, Vec<&FileInfo>> = HashMap::new();
    for info in remote.files.values() {
        remote_by_key.entry(match_key(&info.relative_path)).or_default().push(info);
    }

    for pf in pack_files {
        let key = match_key(&pf.relative_path);
        let local_candidates = local_by_key.get(&key);
        let already_have = local_candidates.is_some_and(|cands| {
            cands.iter().any(|l| !pf.hash.is_empty() && l.hash == pf.hash)
        });
        if already_have {
            continue;
        }

        let matching_remote: Vec<&FileInfo> = remote_by_key
            .get(&key)
            .map(|cands| cands.iter().filter(|r| !pf.hash.is_empty() && r.hash == pf.hash).copied().collect())
            .unwrap_or_default();
        let Some(remote_info) = (!matching_remote.is_empty()).then(|| pick_remote(&matching_remote)) else {
            unavailable.push(pf.relative_path.clone());
            continue;
        };

        match local_candidates {
            None => {
                total_bytes += remote_info.size;
                actions.push(SyncAction::ReceiveFromRemote(remote_info.clone()));
            }
            Some(cands) => {
                let local_info = pick_local(cands, remote_info);
                total_bytes += remote_info.size.max(local_info.size);
                actions.push(SyncAction::Conflict { local: local_info.clone(), remote: remote_info.clone() });
            }
        }
    }

    (SyncPlan { actions, total_bytes, ..Default::default() }, unavailable)
}

/// Why a plan may not run against the current game/folder, if any. Plans are
/// computed against one folder; running one after the game or its path
/// changed would download (and replace) files in the wrong place.
pub fn plan_target_mismatch(plan: &SyncPlan, active_game: &str, active_path: &str) -> Option<String> {
    if plan.game_id.is_empty() || plan.game_id != active_game || plan.base_path != active_path {
        Some("Game folder changed — compare again".to_string())
    } else {
        None
    }
}

/// Name for the remote copy in a "keep both" resolution: `x_remote.package`,
/// then `x_remote2.package`, ... The suffix goes before the real extension and
/// any `.disabled` (`x_remote.package.disabled`) so the kept file is still
/// picked up by scans and keeps the local file's disabled state. `taken` gets
/// each candidate and must say whether it collides with a local file, a host
/// file (a keep-both name that equals a real host path would hijack that
/// file's download) or another pending keep-both.
pub fn keep_both_name(local_path: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    let norm = local_path.replace('\\', "/");
    let (parent, name) = match norm.rfind('/') {
        Some(i) => (&norm[..=i], &norm[i + 1..]),
        None => ("", norm.as_str()),
    };
    let (base, disabled) = if name.to_lowercase().ends_with(DISABLED_SUFFIX) && name.len() > DISABLED_SUFFIX.len() {
        name.split_at(name.len() - DISABLED_SUFFIX.len())
    } else {
        (name, "")
    };
    let (stem, ext) = match base.rfind('.') {
        Some(i) if i > 0 => (&base[..i], &base[i..]),
        _ => (base, ""),
    };
    (1..1000).find_map(|n| {
        let suffix = if n == 1 { "_remote".to_string() } else { format!("_remote{}", n) };
        let candidate = format!("{}{}{}{}{}", parent, stem, suffix, ext, disabled);
        (!taken(&candidate)).then_some(candidate)
    })
}

/// Original file name of a leftover "keep both" temp file
/// (`x.package.synccrate-keep-<ts>.tmp`), left by an interrupted keep-both in
/// versions that moved the local copy aside during the download.
pub fn keep_tmp_original(file_name: &str) -> Option<&str> {
    let rest = file_name.strip_suffix(".tmp")?;
    let i = rest.rfind(".synccrate-keep-")?;
    let ts = &rest[i + ".synccrate-keep-".len()..];
    if i == 0 || ts.is_empty() || !ts.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(&rest[..i])
}

/// Remove actions that cannot execute in the pull-only transfer model and
/// recompute `total_bytes`.
///
/// SyncCrate's file transfer is pull-only: the host serves files and clients
/// download them. `SendToRemote` (pushing a client's local-only files up to the
/// host) has no protocol support — the host never requests files — so leaving
/// those actions in the plan would report "uploads" that silently never happen.
/// Drop them here so the plan reflects what will actually transfer.
pub fn retain_pull_only(plan: &mut SyncPlan) {
    plan.actions.retain(|a| !matches!(a, SyncAction::SendToRemote(_)));
    // Keep total_bytes self-consistent so this function is correct in isolation
    // (and unit-testable). compute_sync_plan recomputes it again after its own
    // permission/exclude filtering, so this value is transient in that path.
    plan.total_bytes = plan
        .actions
        .iter()
        .map(|a| match a {
            SyncAction::ReceiveFromRemote(f) => f.size,
            SyncAction::Conflict { local, remote } => local.size.max(remote.size),
            SyncAction::SendToRemote(f) => f.size,
            SyncAction::Delete(_) => 0,
        })
        .sum();
}

/// Compute a deterministic hash of a sync plan's actions.
/// Used to detect if the plan has changed between sessions.
pub fn compute_plan_hash(plan: &SyncPlan) -> String {
    let mut entries: Vec<String> = plan.actions.iter().map(|action| {
        match action {
            SyncAction::SendToRemote(f) => format!("send:{}:{}", f.relative_path, f.hash),
            SyncAction::ReceiveFromRemote(f) => format!("recv:{}:{}", f.relative_path, f.hash),
            SyncAction::Conflict { local, remote } => {
                format!("conflict:{}:{}:{}", local.relative_path, local.hash, remote.hash)
            }
            SyncAction::Delete(p) => format!("delete:{}", p),
        }
    }).collect();
    entries.sort();

    let mut hasher = Sha256::new();
    for entry in &entries {
        hasher.update(entry.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{FileInfo, FileManifest};
    use std::collections::HashMap;

    fn make_file(path: &str, hash: &str, size: u64) -> FileInfo {
        FileInfo {
            relative_path: path.to_string(),
            size,
            hash: hash.to_string(),
            modified: 1000,
            file_type: "Mod".to_string(),
        }
    }

    fn make_manifest(files: Vec<FileInfo>) -> FileManifest {
        let mut map = HashMap::new();
        for f in files {
            map.insert(f.relative_path.clone(), f);
        }
        FileManifest {
            files: map,
            generated_at: 1000,
        }
    }

    fn ct(folder: &str, exts: &[&str], recursive: bool) -> ContentType {
        ContentType {
            id: folder.to_string(),
            label: folder.to_string(),
            folder: folder.to_string(),
            extensions: exts.iter().map(|e| e.to_string()).collect(),
            file_type: "Mod".to_string(),
            classify_by_extension: Default::default(),
            icon: String::new(),
            color: String::new(),
            syncable: true,
            recursive,
            must_contain: None,
            exclude_files: vec!["ReShade.ini".to_string()],
        }
    }

    #[test]
    fn test_diff_remote_only_receive() {
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "abc123", 1000)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a.package"));
        assert_eq!(plan.total_bytes, 1000);
    }

    #[test]
    fn test_diff_local_only_send() {
        let local = make_manifest(vec![make_file("Mods/b.package", "def456", 2000)]);
        let remote = make_manifest(vec![]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::SendToRemote(f) if f.relative_path == "Mods/b.package"));
        assert_eq!(plan.total_bytes, 2000);
    }

    #[test]
    fn test_diff_same_hash_no_action() {
        let file = make_file("Mods/c.package", "same_hash", 500);
        let local = make_manifest(vec![file.clone()]);
        let remote = make_manifest(vec![file]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 0);
        assert_eq!(plan.total_bytes, 0);
    }

    #[test]
    fn test_diff_different_hash_conflict() {
        let local = make_manifest(vec![make_file("Mods/d.package", "hash_a", 1000)]);
        let remote = make_manifest(vec![make_file("Mods/d.package", "hash_b", 2000)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));
        assert_eq!(plan.total_bytes, 2000); // max(1000, 2000)
    }

    #[test]
    fn match_key_ignores_case_trailing_dots_and_disabled_state() {
        assert_eq!(match_key("Mods/CC/Hair.package"), match_key("Mods/cc/hair.package"));
        assert_eq!(match_key("Mods\\CC\\Hair.package"), "mods/cc/hair.package");
        assert_eq!(match_key("Mods/CC. /hair.package."), "mods/cc/hair.package");
        assert_eq!(match_key("Mods/hair.package.disabled"), "mods/hair.package");
        assert_eq!(match_key("Mods/hair.package.DISABLED"), "mods/hair.package");
        assert_eq!(match_key("Mods/_Disabled/CC/hair.package"), "mods/cc/hair.package");
        // A file literally named `_Disabled` is not the legacy folder.
        assert_eq!(match_key("Mods/_Disabled"), "mods/_disabled");
        assert_ne!(match_key("Mods/a.package"), match_key("Mods/b.package"));
    }

    #[test]
    fn is_disabled_path_detects_suffix_and_legacy_folder() {
        assert!(is_disabled_path("Mods/x.package.disabled"));
        assert!(is_disabled_path("Mods/_Disabled/x.package"));
        assert!(!is_disabled_path("Mods/x.package"));
        assert!(!is_disabled_path("Mods/_Disabled"));
    }

    #[test]
    fn case_variant_with_other_content_is_a_conflict_not_a_receive() {
        let local = make_manifest(vec![make_file("Mods/cc/hair.package", "mine", 10)]);
        let remote = make_manifest(vec![make_file("Mods/CC/Hair.package", "theirs", 20)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        match &plan.actions[0] {
            SyncAction::Conflict { local, remote } => {
                assert_eq!(local.relative_path, "Mods/cc/hair.package");
                assert_eq!(remote.relative_path, "Mods/CC/Hair.package");
            }
            other => panic!("expected conflict, got {:?}", other),
        }
    }

    #[test]
    fn case_variant_with_same_content_is_in_sync() {
        let local = make_manifest(vec![make_file("Mods/cc/hair.package", "h", 10)]);
        let remote = make_manifest(vec![make_file("Mods/CC/Hair.package", "h", 10)]);
        assert!(compute_diff(&local, &remote).actions.is_empty());
    }

    #[test]
    fn locally_disabled_twin_with_same_content_is_in_sync() {
        let local = make_manifest(vec![make_file("Mods/x.package.disabled", "h", 10)]);
        let remote = make_manifest(vec![make_file("Mods/x.package", "h", 10)]);
        let plan = compute_diff(&local, &remote);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.disabled_locally, 1);

        let legacy = make_manifest(vec![make_file("Mods/_Disabled/x.package", "h", 10)]);
        let plan = compute_diff(&legacy, &remote);
        assert!(plan.actions.is_empty());
        assert_eq!(plan.disabled_locally, 1);
    }

    #[test]
    fn locally_disabled_twin_with_other_content_conflicts_on_the_disabled_path() {
        let local = make_manifest(vec![make_file("Mods/x.package.disabled", "old", 10)]);
        let remote = make_manifest(vec![make_file("Mods/x.package", "new", 10)]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0],
            SyncAction::Conflict { local, .. } if local.relative_path == "Mods/x.package.disabled"));
    }

    #[test]
    fn host_disabled_copy_never_adds_a_disabled_twin() {
        let local = make_manifest(vec![make_file("Mods/x.package", "mine", 10)]);
        for hash in ["mine", "other"] {
            let remote = make_manifest(vec![make_file("Mods/x.package.disabled", hash, 10)]);
            let plan = compute_diff(&local, &remote);
            assert!(plan.actions.is_empty(), "hash {}", hash);
            assert_eq!(plan.disabled_on_host, 1);
        }
    }

    #[test]
    fn host_with_both_twins_yields_one_action() {
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![
            make_file("Mods/x.package", "a", 10),
            make_file("Mods/x.package.disabled", "b", 10),
        ]);
        let plan = compute_diff(&local, &remote);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0],
            SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/x.package"));
    }

    #[test]
    fn path_accepted_by_matches_scan_rules() {
        let cts = vec![ct("Mods", &["package", "ts4script"], true), ct(".", &["fx"], false)];
        assert!(path_accepted_by(&cts, "Mods/CC/hair.package"));
        assert!(path_accepted_by(&cts, "mods/cc/hair.PACKAGE"));
        assert!(path_accepted_by(&cts, "Mods/hair.package.disabled"));
        assert!(path_accepted_by(&cts, "Mods/_Disabled/hair.package"));
        assert!(path_accepted_by(&cts, "shader.fx"));
        assert!(!path_accepted_by(&cts, "sub/shader.fx"), "non-recursive");
        assert!(!path_accepted_by(&cts, "Mods/readme.txt"), "extension");
        assert!(!path_accepted_by(&cts, "mod/x.package"), "wrong folder (ETS2)");
        assert!(!path_accepted_by(&cts, "ModsX/x.package"), "folder prefix must be a segment");
        assert!(!path_accepted_by(&cts, "ReShade.ini"), "exclude_files");
        let any_ext = vec![ct("saves", &[], true)];
        assert!(path_accepted_by(&any_ext, "saves/slot1/game.sii"));
        assert!(!path_accepted_by(&any_ext, "saves"));
    }

    #[test]
    fn drop_foreign_counts_and_warns_only_for_unknown_mostly_foreign_hosts() {
        let cts = vec![ct("mod", &["scs"], true)];
        let mut remote = make_manifest(vec![
            make_file("Mods/a.package", "1", 1),
            make_file("Mods/b.package", "2", 1),
            make_file("mod/truck.scs", "3", 1),
        ]);
        assert_eq!(drop_foreign(&mut remote, &cts), 2);
        assert_eq!(remote.files.len(), 1);
        assert!(foreign_warning(None, 2, 3).is_some());
        assert!(foreign_warning(Some(""), 2, 3).is_some());
        assert!(foreign_warning(Some("ets2"), 2, 3).is_none());
        assert!(foreign_warning(None, 1, 3).is_none());
        assert!(foreign_warning(None, 0, 0).is_none());
    }

    #[test]
    fn plan_target_mismatch_requires_same_game_and_path() {
        let plan = SyncPlan { game_id: "sims4".into(), base_path: "C:/A".into(), ..Default::default() };
        assert!(plan_target_mismatch(&plan, "sims4", "C:/A").is_none());
        assert!(plan_target_mismatch(&plan, "sims4", "C:/B").is_some());
        assert!(plan_target_mismatch(&plan, "ets2", "C:/A").is_some());
        assert!(plan_target_mismatch(&SyncPlan::default(), "", "").is_some());
    }

    fn pf(path: &str, hash: &str) -> crate::state::PackFile {
        crate::state::PackFile { relative_path: path.to_string(), size: 1000, hash: hash.to_string() }
    }

    #[test]
    fn pack_plan_downloads_a_missing_file_the_host_has() {
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "abc123", 1000)]);
        let (plan, unavailable) = compute_pack_plan(&local, &remote, &[pf("Mods/a.package", "abc123")]);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(f) if f.relative_path == "Mods/a.package"));
        assert!(unavailable.is_empty());
    }

    #[test]
    fn pack_plan_skips_a_file_already_present_with_the_pack_hash() {
        let local = make_manifest(vec![make_file("Mods/a.package", "abc123", 1000)]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "abc123", 1000)]);
        let (plan, unavailable) = compute_pack_plan(&local, &remote, &[pf("Mods/a.package", "abc123")]);
        assert!(plan.actions.is_empty());
        assert!(unavailable.is_empty());
    }

    #[test]
    fn pack_plan_conflicts_a_locally_different_file() {
        let local = make_manifest(vec![make_file("Mods/a.package", "local_hash", 1000)]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "pack_hash", 1000)]);
        let (plan, unavailable) = compute_pack_plan(&local, &remote, &[pf("Mods/a.package", "pack_hash")]);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::Conflict { .. }));
        assert!(unavailable.is_empty());
    }

    #[test]
    fn pack_plan_reports_unavailable_when_host_lacks_the_exact_hash() {
        // Host doesn't have the path at all.
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![]);
        let (plan, unavailable) = compute_pack_plan(&local, &remote, &[pf("Mods/a.package", "abc123")]);
        assert!(plan.actions.is_empty());
        assert_eq!(unavailable, vec!["Mods/a.package".to_string()]);

        // Host has the path, but different content than the pack specifies.
        let remote2 = make_manifest(vec![make_file("Mods/a.package", "different_hash", 1000)]);
        let (plan2, unavailable2) = compute_pack_plan(&local, &remote2, &[pf("Mods/a.package", "abc123")]);
        assert!(plan2.actions.is_empty());
        assert_eq!(unavailable2, vec!["Mods/a.package".to_string()]);
    }

    #[test]
    fn pack_plan_never_touches_files_outside_the_pack() {
        let local = make_manifest(vec![make_file("Mods/extra_local.package", "x", 5)]);
        let remote = make_manifest(vec![
            make_file("Mods/a.package", "abc123", 1000),
            make_file("Mods/extra_remote.package", "y", 5),
        ]);
        let (plan, _) = compute_pack_plan(&local, &remote, &[pf("Mods/a.package", "abc123")]);
        assert_eq!(plan.actions.len(), 1, "only the pack's own file should appear");
        assert!(plan.actions.iter().all(|a| !matches!(a, SyncAction::Delete(_) | SyncAction::SendToRemote(_))));
    }

    #[test]
    fn pack_plan_matches_case_only_and_disabled_names() {
        // Client already has it, disabled, different case: counts as "have", no action.
        let local = make_manifest(vec![make_file("Mods/cc/hair.package.disabled", "abc123", 1000)]);
        let remote = make_manifest(vec![make_file("Mods/CC/Hair.package", "abc123", 1000)]);
        let (plan, unavailable) = compute_pack_plan(&local, &remote, &[pf("Mods/CC/Hair.package", "abc123")]);
        assert!(plan.actions.is_empty());
        assert!(unavailable.is_empty());
    }

    #[test]
    fn keep_both_name_keeps_compound_extensions_and_skips_taken() {
        assert_eq!(keep_both_name("Mods/x.package", |_| false).as_deref(), Some("Mods/x_remote.package"));
        assert_eq!(
            keep_both_name("Mods/x.package.disabled", |_| false).as_deref(),
            Some("Mods/x_remote.package.disabled")
        );
        assert_eq!(keep_both_name("README", |_| false).as_deref(), Some("README_remote"));
        assert_eq!(keep_both_name(".hidden", |_| false).as_deref(), Some(".hidden_remote"));
        let taken = ["Mods/x_remote.package", "Mods/x_remote2.package"];
        assert_eq!(
            keep_both_name("Mods/x.package", |c| taken.contains(&c)).as_deref(),
            Some("Mods/x_remote3.package")
        );
        assert_eq!(keep_both_name("x.package", |_| true), None);
    }

    #[test]
    fn keep_tmp_original_parses_only_keep_temp_names() {
        assert_eq!(keep_tmp_original("x.package.synccrate-keep-1711548000.tmp"), Some("x.package"));
        assert_eq!(keep_tmp_original("x.package.tmp"), None);
        assert_eq!(keep_tmp_original("x.synccrate-keep-abc.tmp"), None);
        assert_eq!(keep_tmp_original(".synccrate-keep-1.tmp"), None);
    }

    #[test]
    fn test_retain_pull_only_drops_uploads() {
        // local has an extra file (would be SendToRemote), remote has one we lack
        // (ReceiveFromRemote), plus a differing file (Conflict).
        let local = make_manifest(vec![
            make_file("Mods/local_only.package", "h_local", 4000),
            make_file("Mods/conflict.package", "h_a", 1000),
        ]);
        let remote = make_manifest(vec![
            make_file("Mods/remote_only.package", "h_remote", 3000),
            make_file("Mods/conflict.package", "h_b", 2000),
        ]);
        let mut plan = compute_diff(&local, &remote);
        // Before filtering: receive + send + conflict
        assert!(plan.actions.iter().any(|a| matches!(a, SyncAction::SendToRemote(_))));

        retain_pull_only(&mut plan);

        // No uploads remain
        assert!(!plan.actions.iter().any(|a| matches!(a, SyncAction::SendToRemote(_))));
        // Receive + conflict remain
        assert_eq!(plan.actions.len(), 2);
        // total_bytes = receive(3000) + conflict max(1000,2000)=2000
        assert_eq!(plan.total_bytes, 5000);
    }

    #[test]
    fn test_retain_pull_only_keeps_receives_and_conflicts() {
        let local = make_manifest(vec![]);
        let remote = make_manifest(vec![make_file("Mods/a.package", "h", 500)]);
        let mut plan = compute_diff(&local, &remote);
        retain_pull_only(&mut plan);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(&plan.actions[0], SyncAction::ReceiveFromRemote(_)));
        assert_eq!(plan.total_bytes, 500);
    }

    #[test]
    fn test_plan_hash_deterministic() {
        let local = make_manifest(vec![make_file("a.txt", "h1", 100)]);
        let remote = make_manifest(vec![make_file("b.txt", "h2", 200)]);
        let plan = compute_diff(&local, &remote);
        let hash1 = compute_plan_hash(&plan);
        let hash2 = compute_plan_hash(&plan);
        assert_eq!(hash1, hash2, "same plan should produce same hash");
    }

    #[test]
    fn test_plan_hash_different_plans() {
        let local1 = make_manifest(vec![]);
        let remote1 = make_manifest(vec![make_file("a.txt", "h1", 100)]);
        let plan1 = compute_diff(&local1, &remote1);

        let local2 = make_manifest(vec![]);
        let remote2 = make_manifest(vec![make_file("b.txt", "h2", 200)]);
        let plan2 = compute_diff(&local2, &remote2);

        assert_ne!(compute_plan_hash(&plan1), compute_plan_hash(&plan2));
    }
}
