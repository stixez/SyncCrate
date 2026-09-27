//! Save handoff: friends in a crew take turns on one save (a Sims legacy
//! family, a Minecraft or Terraria world, a Factorio base). Whoever holds a
//! shared save has its newest copy; "playing" tells the others to wait.
//!
//! There is no server, so the save moves between the host and a friend in a
//! session, and the friend's PC does the moving: *take* downloads the host's
//! copy (the host must hold it and not be playing), *give* uploads the
//! friend's copy to the host (the friend must hold it). Two friends hand a
//! save over through whoever hosts: give it to the host, then take it.
//!
//! Who holds what is a per-crew list of `SharedSave` records, spread like the
//! crew's member list: last writer wins by `version`, carried both ways by a
//! `HandoffSync` poll (feature `FEATURE`). Like everything in a crew it's
//! advisory between friends: the checks that matter are the host's, at the
//! moment a save actually moves.
//!
//! A shared save never moves in a normal sync (`is_shared`): the host's copy
//! is often older than the holder's, and "use theirs" in a sync plan would
//! quietly overwrite the newer game.
use crate::registry::ContentType;
use crate::state::FileInfo;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const FEATURE: &str = "handoff";
pub const MAX_SAVES_PER_CREW: usize = 64;
/// Files in one save: a Sims slot is a handful, a big Minecraft world a few
/// thousand region files.
pub const MAX_SAVE_FILES: usize = 20_000;
pub const MAX_SAVE_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const MAX_UNIT_CHARS: usize = 160;
/// Real saves change hands a few times a week; anything near u64::MAX is an
/// attempt to win every merge (and overflow the next change).
pub const MAX_VERSION: u64 = 1_000_000;
const MAX_CLOCK_SKEW_SECS: u64 = 24 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SharedSave {
    pub game: String,
    /// `<content type id>/<name>`, see `unit_of`.
    pub unit: String,
    /// Node id of whoever has the newest copy.
    pub holder: String,
    pub holder_name: String,
    /// The holder is playing it: nobody else should take it.
    #[serde(default)]
    pub playing: bool,
    /// No longer shared (kept so an old copy of the record can't bring it back).
    #[serde(default)]
    pub removed: bool,
    pub version: u64,
    pub updated_at: u64,
}

impl SharedSave {
    /// Newest wins: version, then time, then holder (two friends who both
    /// changed "v3" offline still agree afterwards).
    fn key(&self) -> (u64, u64, &str) {
        (self.version, self.updated_at, self.holder.as_str())
    }

    pub fn is(&self, game: &str, unit: &str) -> bool {
        self.game == game && self.unit.eq_ignore_ascii_case(unit)
    }

    /// The next version of this record, changed by `f`.
    pub fn bumped(&self, now: u64, f: impl FnOnce(&mut SharedSave)) -> SharedSave {
        let mut next = self.clone();
        f(&mut next);
        next.version = (self.version + 1).min(MAX_VERSION);
        next.updated_at = now.max(self.updated_at);
        next
    }
}

/// One crew's records, on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrewSaves {
    pub crew: String,
    pub saves: Vec<SharedSave>,
}

/// Client → host, inside `HandoffSync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandoffRequest {
    /// Take the host's copy.
    Take { crew: String, game: String, unit: String },
    /// Hand our copy to the host: these are all of the save's files now.
    Give { crew: String, game: String, unit: String, files: Vec<FileInfo> },
    /// Every upload `Give` asked for has arrived: put them in place.
    Commit { crew: String, game: String, unit: String },
}

/// Host → client, inside `HandoffStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandoffReply {
    /// `Take` granted: these are the save's files on the host now.
    Granted { files: Vec<FileInfo> },
    /// `Give` accepted: upload these (the host already has the rest as-is).
    Upload { paths: Vec<String> },
    /// `Commit` done: the host holds the save.
    Done,
    Refused { message: String },
}

/// Host side: a friend's `Give` in progress.
#[derive(Debug, Clone)]
pub struct PendingGive {
    pub crew: String,
    pub game: String,
    pub unit: String,
    /// Every file of the save on the friend's PC.
    pub files: Vec<FileInfo>,
    /// Paths still to upload (a subset of `files`).
    pub needed: HashSet<String>,
    /// Uploaded so far: path -> temp file next to its destination.
    pub staged: std::collections::HashMap<String, std::path::PathBuf>,
}

pub fn supports(features: &[String]) -> bool {
    features.iter().any(|f| f == FEATURE)
}

// ---------------------------------------------------------------------------
// Which save a file belongs to

/// The save a game-folder-relative path belongs to, as `<content type
/// id>/<name>`: the first folder inside a Save content type's folder (a
/// Minecraft world, a Project Zomboid save) or, for a file directly in it,
/// its name up to the first dot (`Slot_00000002.save.ver1` and
/// `Slot_00000002.save` → `Slot_00000002`; `World.wld.bak` → `World`).
/// None outside Save content types.
pub fn unit_of(cts: &[ContentType], path: &str) -> Option<String> {
    let (ct, rel) = crate::sync::diff::content_type_for(cts, path)?;
    if ct.file_type != "Save" {
        return None;
    }
    let name = match rel.split_once('/') {
        Some((folder, _)) => folder.to_string(),
        None => {
            let file = rel.strip_suffix(crate::commands::files::DISABLED_SUFFIX).unwrap_or(&rel);
            file.split('.').next().unwrap_or(file).to_string()
        }
    };
    (!name.is_empty()).then(|| format!("{}/{}", ct.id, name))
}

/// The display name of a unit (`saves/Slot_00000002` → `Slot_00000002`).
pub fn unit_name(unit: &str) -> &str {
    unit.split_once('/').map_or(unit, |(_, n)| n)
}

pub fn files_of<'a>(cts: &[ContentType], files: impl Iterator<Item = &'a FileInfo>, unit: &str) -> Vec<FileInfo> {
    let mut out: Vec<FileInfo> = files.filter(|f| unit_of(cts, &f.relative_path).is_some_and(|u| u.eq_ignore_ascii_case(unit))).cloned().collect();
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    out
}

/// Whether `path` is part of a save that's shared (`units` lowercase):
/// those only move by take and give, never in a normal sync.
pub fn is_shared(cts: &[ContentType], units: &HashSet<String>, path: &str) -> bool {
    !units.is_empty() && unit_of(cts, path).is_some_and(|u| units.contains(&u.to_lowercase()))
}

/// Lowercase units of `game` shared in any of these crews.
pub fn shared_units<'a>(crews: impl Iterator<Item = &'a crate::crews::Crew>, game: &str) -> HashSet<String> {
    crews.flat_map(|c| c.saves.iter()).filter(|s| s.game == game && !s.removed).map(|s| s.unit.to_lowercase()).collect()
}

// ---------------------------------------------------------------------------
// Merging records from a peer (untrusted)

fn valid_unit(unit: &str) -> bool {
    let Some((ct, name)) = unit.split_once('/') else { return false };
    unit.chars().count() <= MAX_UNIT_CHARS
        && !ct.is_empty()
        && ct.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(|c| c.is_control() || crate::chat::is_bidi_control(c))
}

/// A peer's record, checked and normalised; None if it can't be one.
pub fn validate(mut s: SharedSave, is_known_game: impl Fn(&str) -> bool, now: u64) -> Option<SharedSave> {
    if !is_known_game(&s.game) || !valid_unit(&s.unit) || !crate::crews::is_valid_node_id(&s.holder) || s.version == 0 || s.version > MAX_VERSION {
        return None;
    }
    s.holder_name = crate::crews::clean_name(&s.holder_name).unwrap_or_else(|| "A friend".into());
    s.updated_at = s.updated_at.min(now + MAX_CLOCK_SKEW_SECS);
    Some(s)
}

/// Merge one record; true if it changed anything.
pub fn merge(saves: &mut Vec<SharedSave>, incoming: SharedSave) -> bool {
    match saves.iter().position(|s| s.is(&incoming.game, &incoming.unit)) {
        Some(i) if incoming.key() > saves[i].key() => {
            saves[i] = incoming;
            true
        }
        Some(_) => false,
        None if saves.len() < MAX_SAVES_PER_CREW => {
            saves.push(incoming);
            true
        }
        None => false,
    }
}

/// Merge a peer's records for one crew (validated); true if anything changed.
pub fn receive(saves: &mut Vec<SharedSave>, incoming: Vec<SharedSave>, is_known_game: impl Fn(&str) -> bool, now: u64) -> bool {
    let mut changed = false;
    for s in incoming.into_iter().take(MAX_SAVES_PER_CREW) {
        if let Some(s) = validate(s, &is_known_game, now) {
            changed |= merge(saves, s);
        }
    }
    changed
}

// ---------------------------------------------------------------------------
// Moving a save

/// The host's check of a `Give`: every file belongs to `unit`, has a safe
/// path, a real hash and a sane size, and there are no duplicates.
pub fn check_give(cts: &[ContentType], unit: &str, files: Vec<FileInfo>) -> Result<Vec<FileInfo>, String> {
    if files.is_empty() {
        return Err("That save has no files.".into());
    }
    if files.len() > MAX_SAVE_FILES {
        return Err("That save has too many files to hand over.".into());
    }
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for f in &files {
        let hash_ok = f.hash.len() == 64 && f.hash.bytes().all(|b| b.is_ascii_hexdigit());
        if crate::utils::validate_relative(&f.relative_path).is_err()
            || !unit_of(cts, &f.relative_path).is_some_and(|u| u.eq_ignore_ascii_case(unit))
            || crate::utils::is_dangerous_extension(&f.relative_path)
            || !hash_ok
            || f.size > crate::network::transfer::MAX_FILE_SIZE
            || !seen.insert(crate::sync::diff::match_key(&f.relative_path))
        {
            return Err(format!("{} can't be part of this save.", f.relative_path));
        }
        total = total.saturating_add(f.size);
    }
    if total > MAX_SAVE_BYTES {
        return Err("That save is too big to hand over.".into());
    }
    Ok(files)
}

/// Make `have` (one save's files on this PC) match `want` (the other
/// side's): what to fetch (missing or different) and which of ours to
/// delete (gone on the other side, like a rotated Sims backup).
pub fn diff_save(have: &[FileInfo], want: &[FileInfo]) -> (Vec<FileInfo>, Vec<String>) {
    use crate::sync::diff::match_key;
    let ours: std::collections::HashMap<String, &FileInfo> = have.iter().map(|f| (match_key(&f.relative_path), f)).collect();
    let theirs: HashSet<String> = want.iter().map(|f| match_key(&f.relative_path)).collect();
    let fetch = want.iter().filter(|w| ours.get(&match_key(&w.relative_path)).is_none_or(|h| h.hash != w.hash)).cloned().collect();
    let mut delete: Vec<String> = have.iter().filter(|h| !theirs.contains(&match_key(&h.relative_path))).map(|h| h.relative_path.clone()).collect();
    delete.sort();
    (fetch, delete)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ct(id: &str, folder: &str, file_type: &str) -> ContentType {
        ContentType {
            id: id.into(), label: id.into(), folder: folder.into(), extensions: vec![], file_type: file_type.into(),
            classify_by_extension: Default::default(), icon: String::new(), color: String::new(), syncable: true, recursive: true,
            must_contain: None, exclude_files: vec![], exclude_patterns: vec![],
        }
    }

    fn f(path: &str, hash: char) -> FileInfo {
        FileInfo { relative_path: path.into(), size: 3, hash: hash.to_string().repeat(64), modified: 0, file_type: "Save".into() }
    }

    const NODE_A: &str = "0101010101010101010101010101010101010101010101010101010101010101";
    const NODE_B: &str = "0202020202020202020202020202020202020202020202020202020202020202";

    fn rec(unit: &str, holder: &str, version: u64) -> SharedSave {
        SharedSave { game: "sims4".into(), unit: unit.into(), holder: holder.into(), holder_name: "A".into(), playing: false, removed: false, version, updated_at: 10 }
    }

    #[test]
    fn saves_are_grouped_by_folder_or_file_stem() {
        let cts = vec![ct("mods", "Mods", "CustomContent"), ct("saves", "Saves", "Save")];
        assert_eq!(unit_of(&cts, "Saves/Slot_00000002.save").as_deref(), Some("saves/Slot_00000002"));
        assert_eq!(unit_of(&cts, "Saves/Slot_00000002.save.ver3").as_deref(), Some("saves/Slot_00000002"));
        assert_eq!(unit_of(&cts, "Saves/My World/region/r.0.0.mca").as_deref(), Some("saves/My World"));
        assert_eq!(unit_of(&cts, "Mods/a.package"), None, "not a save");
        let all = [f("Saves/Slot_00000002.save", 'a'), f("Saves/slot_00000002.save.ver0", 'b'), f("Saves/Slot_00000003.save", 'c')];
        assert_eq!(files_of(&cts, all.iter(), "saves/Slot_00000002").len(), 2, "case doesn't split a save");
        let units: HashSet<String> = ["saves/slot_00000002".to_string()].into();
        assert!(is_shared(&cts, &units, "Saves/Slot_00000002.save.ver1") && !is_shared(&cts, &units, "Saves/Slot_00000003.save"));
        assert_eq!(unit_name("saves/My World"), "My World");
    }

    #[test]
    fn records_merge_newest_first_and_bad_ones_are_dropped() {
        let known = |g: &str| g == "sims4";
        let mut saves = vec![rec("saves/Slot_1", NODE_A, 2)];
        assert!(!merge(&mut saves, rec("saves/slot_1", NODE_B, 1)), "older loses");
        assert!(merge(&mut saves, rec("saves/Slot_1", NODE_B, 3)));
        assert_eq!(saves[0].holder, NODE_B);
        let bad = [
            SharedSave { game: "nope".into(), ..rec("saves/x", NODE_A, 1) },
            rec("saves/../x", NODE_A, 1),
            rec("saves/a/b", NODE_A, 1),
            rec("noslash", NODE_A, 1),
            rec("saves/x", "not-a-node", 1),
            rec("saves/x", NODE_A, MAX_VERSION + 1),
            rec("saves/x", NODE_A, 0),
        ];
        assert!(!receive(&mut saves, bad.to_vec(), known, 100));
        assert_eq!(saves.len(), 1);
        let far = SharedSave { updated_at: u64::MAX, holder_name: "\u{202e}x\n".into(), ..rec("saves/Slot_2", NODE_A, 1) };
        assert!(receive(&mut saves, vec![far], known, 100));
        assert!(saves[1].updated_at <= 100 + MAX_CLOCK_SKEW_SECS && saves[1].holder_name == "x");
        let next = saves[0].bumped(50, |s| s.playing = true);
        assert!(next.version == 4 && next.playing && next.updated_at == 50);
        assert_eq!(SharedSave { version: MAX_VERSION, ..rec("saves/x", NODE_A, 1) }.bumped(1, |_| {}).version, MAX_VERSION);
    }

    #[test]
    fn a_give_is_checked_file_by_file() {
        let cts = vec![ct("mods", "Mods", "CustomContent"), ct("saves", "Saves", "Save")];
        let unit = "saves/Slot_1";
        assert!(check_give(&cts, unit, vec![f("Saves/Slot_1.save", 'a'), f("Saves/Slot_1.save.ver0", 'b')]).is_ok());
        for bad in [f("Saves/Slot_2.save", 'a'), f("Mods/x.package", 'a'), f("Saves/../Mods/Slot_1.save", 'a'), FileInfo { hash: "zz".into(), ..f("Saves/Slot_1.save", 'a') }] {
            assert!(check_give(&cts, unit, vec![bad.clone()]).is_err(), "{}", bad.relative_path);
        }
        assert!(check_give(&cts, unit, vec![f("Saves/Slot_1.save", 'a'), f("Saves/SLOT_1.save", 'b')]).is_err(), "duplicate");
        assert!(check_give(&cts, unit, vec![]).is_err());
    }

    #[test]
    fn diffing_a_save_fetches_changes_and_drops_what_is_gone() {
        let have = [f("Saves/S.save", 'a'), f("Saves/S.save.ver0", 'b'), f("Saves/S.save.ver4", 'c')];
        let want = [f("Saves/S.save", 'x'), f("Saves/s.save.ver0", 'b'), f("Saves/S.save.ver1", 'd')];
        let (fetch, delete) = diff_save(&have, &want);
        let fetched: Vec<&str> = fetch.iter().map(|f| f.relative_path.as_str()).collect();
        assert_eq!(fetched, ["Saves/S.save", "Saves/S.save.ver1"]);
        assert_eq!(delete, ["Saves/S.save.ver4"]);
    }
}
