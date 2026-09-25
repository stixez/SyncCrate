//! "Friends can offer mods to the host": a client lists files it has that
//! the host doesn't; the host picks which to take; only then does the client
//! upload them. Pull-only still holds in spirit: nothing lands on the host
//! without the host accepting that exact file (path, size and hash).
//!
//! Wire flow (feature `FEATURE`, used only when the host's Welcome lists it):
//! the client's idle loop, holding the stream lock like the chat poll, sends
//! `OfferSync` (with the file list the first time) and reads `OfferStatus`.
//! For each accepted file it then sends `FileHeader` + chunks + `FileComplete`
//! and reads `OfferResult`. The host never sends anything unprompted.
//!
//! The host re-checks every upload: it was offered and accepted, its path is
//! in the game's content folders, it's not a blocked file type, size and hash
//! match the offer, and it never replaces an existing file.
use crate::registry::ContentType;
use crate::state::{FileInfo, FileManifest};
use serde::Serialize;
use std::collections::HashSet;

pub const FEATURE: &str = "offers";
pub const MAX_OFFER_FILES: usize = 500;
pub const MAX_OFFER_BYTES: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OfferState {
    Pending,
    Accepted,
    Declined,
    Received,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct OfferedFile {
    pub file: FileInfo,
    pub state: OfferState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Host side: what one connected friend offered.
#[derive(Debug, Clone, Serialize)]
pub struct IncomingOffer {
    pub peer_id: String,
    pub peer_name: String,
    pub files: Vec<OfferedFile>,
}

/// Client side: our current offer to the host.
#[derive(Debug, Clone, Serialize, Default)]
pub struct OutgoingOffer {
    pub files: Vec<OfferedFile>,
    /// The file list has reached the host.
    #[serde(skip)]
    pub delivered: bool,
}

impl OutgoingOffer {
    /// Still waiting on the host or an upload.
    pub fn active(&self) -> bool {
        self.files.iter().any(|f| matches!(f.state, OfferState::Pending | OfferState::Accepted))
    }
}

/// The subset of `files` that can be offered (client) or accepted as an
/// offer (host): in a content folder, not a blocked type, a real hash, sane
/// size, not something the other side already has, no duplicates. Capped.
pub fn valid_offer(files: Vec<FileInfo>, cts: &[ContentType], already_there: &FileManifest) -> Vec<FileInfo> {
    let present: HashSet<String> = already_there.files.keys().map(|k| crate::sync::diff::match_key(k)).collect();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    let mut out = Vec::new();
    for f in files {
        if out.len() >= MAX_OFFER_FILES {
            break;
        }
        let key = crate::sync::diff::match_key(&f.relative_path);
        let hash_ok = f.hash.len() == 64 && f.hash.bytes().all(|b| b.is_ascii_hexdigit());
        if !hash_ok
            || f.size == 0
            || f.size > crate::network::transfer::MAX_FILE_SIZE
            || total.saturating_add(f.size) > MAX_OFFER_BYTES
            || !crate::sync::diff::path_accepted_by(cts, &f.relative_path)
            || crate::utils::is_dangerous_extension(&f.relative_path)
            || present.contains(&key)
            || !seen.insert(key)
        {
            continue;
        }
        total += f.size;
        out.push(f);
    }
    out
}

pub fn supports(features: &[String]) -> bool {
    features.iter().any(|f| f == FEATURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ct(id: &str, folder: &str) -> ContentType {
        ContentType {
            id: id.into(), label: id.into(), folder: folder.into(), extensions: vec![], file_type: "CustomContent".into(),
            classify_by_extension: Default::default(), icon: String::new(), color: String::new(), syncable: true, recursive: true,
            must_contain: None, exclude_files: vec![],
        }
    }

    fn f(path: &str, size: u64) -> FileInfo {
        FileInfo { relative_path: path.into(), size, hash: "a".repeat(64), modified: 0, file_type: "CustomContent".into() }
    }

    #[test]
    fn only_new_safe_files_in_content_folders_can_be_offered() {
        let cts = vec![ct("mods", "Mods")];
        let mut host = FileManifest::default();
        host.files.insert("Mods/have.package".into(), f("Mods/have.package", 1));
        let offer = vec![
            f("Mods/new.package", 10),
            f("Mods/HAVE.package", 10),         // host has it (case-insensitive)
            f("dinput8.dll", 10),               // outside the content folders
            f("Mods/evil.exe", 10),             // blocked type
            f("Mods/new.package", 10),          // duplicate
            f("Mods/empty.package", 0),         // empty
            FileInfo { hash: "zz".into(), ..f("Mods/badhash.package", 1) },
        ];
        let ok = valid_offer(offer, &cts, &host);
        assert_eq!(ok.iter().map(|x| x.relative_path.as_str()).collect::<Vec<_>>(), vec!["Mods/new.package"]);
    }

    #[test]
    fn offers_are_capped() {
        let cts = vec![ct("mods", "Mods")];
        let many: Vec<FileInfo> = (0..MAX_OFFER_FILES + 20).map(|i| f(&format!("Mods/{i}.package"), 1)).collect();
        assert_eq!(valid_offer(many, &cts, &FileManifest::default()).len(), MAX_OFFER_FILES);
        let huge = vec![f("Mods/a.package", MAX_OFFER_BYTES), f("Mods/b.package", 1)];
        assert!(valid_offer(huge, &cts, &FileManifest::default()).len() <= 1, "total size cap");
    }
}
