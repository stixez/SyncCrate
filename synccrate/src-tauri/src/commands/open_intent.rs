//! "Open from anywhere": `synccrate://pack/...`, `synccrate://join/...` and
//! `synccrate://crew/...` links and double-clicked `.scpack` files all end up here, whether they
//! started the app (cold start: argv, or macOS `RunEvent::Opened`) or were
//! forwarded to the running window (single-instance plugin).
//!
//! Everything arriving this way is untrusted: it's whatever a stranger
//! pasted in a chat. `classify` is a pure parser/validator; `resolve` turns
//! a target into an `OpenIntent` (reading a pack file at most). Nothing here
//! writes files, connects, syncs or switches games — the frontend only
//! *shows* the intent (pack import view, filled-in join box or the
//! wrong-game prompt) and every real action still needs a click.
//!
//! Intents are queued in `PendingIntents` and the frontend drains the queue
//! (`take_open_intents`) on mount and whenever `open-intent` fires, so a
//! link that launched the app isn't lost before the webview is listening.
use crate::state::{AppState, ModPack};
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

const SCHEME: &str = "synccrate:";
/// Pack links are capped at 8 KB of base64 on export; leave room for
/// percent-encoding and the prefix, and refuse anything past that up front.
pub(crate) const MAX_INPUT_BYTES: usize = crate::commands::modpack::MAX_LINK_BYTES * 3;
/// Real join codes are well under 100 characters (IPv4 list + iroh id).
const MAX_CODE_CHARS: usize = 256;
/// A link flood (a script opening hundreds of links) must not pile up.
const MAX_PENDING: usize = 8;
pub const EVENT: &str = "open-intent";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenTarget {
    /// The (percent-decoded) base64 payload of a pack link.
    PackLink(String),
    Join { code: String, game_id: String },
    /// A decoded and validated crew invite (`crews::decode_invite`).
    Crew(crate::crews::CrewInvite),
    PackFile(PathBuf),
    /// Our scheme, but unusable; the reason is shown to the user.
    Invalid(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpenIntent {
    Pack { pack: ModPack },
    Join { code: String, game_id: String },
    /// Shown as "Add crew?"; adding it never connects or syncs.
    Crew { invite: crate::crews::CrewInvite },
    Invalid { reason: String },
}

#[derive(Default)]
pub struct PendingIntents(std::sync::Mutex<VecDeque<OpenIntent>>);

impl PendingIntents {
    fn push(&self, intent: OpenIntent) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() >= MAX_PENDING {
            q.pop_front();
        }
        q.push_back(intent);
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // Byte-wise hex parsing: slicing `s` here could split a multi-byte
        // char and panic on hostile input.
        let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Chat apps and markdown glue punctuation onto links (`(see synccrate://…)`,
/// a sentence-ending `.`, `**bold**`). None of these can end a real link:
/// pack payloads are URL-safe base64 and codes/game ids are alphanumeric.
fn strip_trailing_junk(s: &str) -> &str {
    s.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, ')' | ']' | '}' | '>' | '.' | ',' | ';' | ':' | '!' | '?' | '"' | '\'' | '*' | '|' | '`')
    })
}

fn is_pack_path(s: &str) -> bool {
    s.len() > ".scpack".len() && s.to_ascii_lowercase().ends_with(".scpack")
}

fn is_plausible_game_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Classify one raw argument / URL. `None` means "not ours" (argv[0], flags,
/// random files) and is ignored silently; `Invalid` means it *is* a
/// SyncCrate link but can't be used, and the user should be told why.
pub fn classify(raw: &str, is_known_game: impl Fn(&str) -> bool) -> Option<OpenTarget> {
    let s = raw.trim().trim_start_matches('<').trim_matches('"');
    if s.is_empty() {
        return None;
    }
    let is_link = s.get(..SCHEME.len()).is_some_and(|p| p.eq_ignore_ascii_case(SCHEME));
    if !is_link {
        return (s.len() <= MAX_INPUT_BYTES && is_pack_path(s)).then(|| OpenTarget::PackFile(PathBuf::from(s)));
    }
    let invalid = |r: &str| Some(OpenTarget::Invalid(r.to_string()));
    if s.len() > MAX_INPUT_BYTES {
        return invalid("That link is too long to be a SyncCrate link.");
    }

    let rest = strip_trailing_junk(s[SCHEME.len()..].trim_start_matches('/'));
    let split = rest.find(['/', '?']).unwrap_or(rest.len());
    let (kind, tail) = (rest[..split].to_ascii_lowercase(), rest[split..].trim_start_matches('/'));
    let (path, query) = tail.split_once('?').unwrap_or((tail, ""));

    match kind.as_str() {
        "pack" => {
            let payload = percent_decode(path).trim().to_string();
            if payload.is_empty() {
                return invalid("That pack link is empty — it may have been cut off.");
            }
            Some(OpenTarget::PackLink(payload))
        }
        "join" => {
            let code = percent_decode(path.trim_end_matches('/')).trim().to_ascii_uppercase();
            if code.is_empty() || code.len() > MAX_CODE_CHARS || crate::network::joincode::decode(&code).is_err() {
                return invalid("That invite link has a broken join code — ask your friend to copy it again.");
            }
            let game = query
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| k.eq_ignore_ascii_case("game"))
                // Browsers sometimes append a `/` to custom-scheme URLs.
                .map(|(_, v)| percent_decode(v).trim().trim_end_matches('/').to_ascii_lowercase());
            let Some(game_id) = game.filter(|g| !g.is_empty()) else {
                return invalid("That invite link doesn't say which game it's for.");
            };
            if !is_plausible_game_id(&game_id) || !is_known_game(&game_id) {
                return invalid("That invite is for a game this version of SyncCrate doesn't know — try updating.");
            }
            Some(OpenTarget::Join { code, game_id })
        }
        "crew" => {
            let payload = percent_decode(path.trim_end_matches('/'));
            match crate::crews::decode_invite(&payload, is_known_game) {
                Ok(invite) => Some(OpenTarget::Crew(invite)),
                Err(e) => invalid(&e),
            }
        }
        _ => invalid("SyncCrate doesn't recognize this link — you may need a newer version."),
    }
}

/// Load/validate what a target points at. Reads a pack file (size-capped by
/// `read_pack_file`), nothing else touches the disk.
pub(crate) fn resolve(target: OpenTarget) -> OpenIntent {
    let invalid = |reason: String| OpenIntent::Invalid { reason };
    match target {
        OpenTarget::PackLink(payload) => match crate::commands::modpack::decode_pack_payload(&payload) {
            Ok(pack) => OpenIntent::Pack { pack },
            Err(e) => invalid(format!("Couldn't read that pack link: {e}")),
        },
        OpenTarget::PackFile(path) => {
            if !path.is_file() {
                return invalid("That pack file doesn't exist any more.".to_string());
            }
            match crate::commands::modpack::read_pack_file(&path) {
                Ok(pack) => OpenIntent::Pack { pack },
                Err(e) => invalid(format!("Couldn't open that pack file: {e}")),
            }
        }
        OpenTarget::Join { code, game_id } => OpenIntent::Join { code, game_id },
        OpenTarget::Crew(invite) => OpenIntent::Crew { invite },
        OpenTarget::Invalid(reason) => invalid(reason),
    }
}

/// The one entry point: raw argument → validated intent (or `None` if it
/// isn't ours). Refused while a sync, restore or undo is rewriting files, so
/// a link can't pull the UI away from (or race) one.
pub(crate) async fn intent_from_raw(state: &Arc<Mutex<AppState>>, raw: &str, cwd: Option<&Path>) -> Option<OpenIntent> {
    let (known, syncing): (std::collections::HashSet<String>, bool) = {
        let s = state.lock().await;
        (s.game_registry.games.iter().map(|g| g.id.clone()).collect(), s.is_any_syncing())
    };
    let target = classify(raw, |g| known.contains(g))?;
    if syncing || crate::commands::backup::restore_in_progress() {
        return Some(OpenIntent::Invalid {
            reason: "SyncCrate is busy syncing or restoring — open the link again when it's done.".to_string(),
        });
    }
    let target = match (target, cwd) {
        (OpenTarget::PackFile(p), Some(cwd)) if p.is_relative() => OpenTarget::PackFile(cwd.join(p)),
        (t, _) => t,
    };
    Some(tokio::task::spawn_blocking(move || resolve(target)).await.unwrap_or_else(|e| OpenIntent::Invalid { reason: e.to_string() }))
}

/// Queue intents for `args` (argv without the exe, or macOS open URLs), nudge
/// the frontend and bring the window forward. Called from setup (cold start),
/// the single-instance callback and `RunEvent::Opened`.
pub fn handle_args(app: &tauri::AppHandle, args: Vec<String>, cwd: Option<PathBuf>) {
    if args.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<Mutex<AppState>>>().inner().clone();
        let mut any = false;
        for arg in &args {
            if let Some(intent) = intent_from_raw(&state, arg, cwd.as_deref()).await {
                app.state::<PendingIntents>().push(intent);
                any = true;
            }
        }
        if any {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            let _ = app.emit(EVENT, ());
        }
    });
}

#[tauri::command]
pub fn take_open_intents(pending: tauri::State<'_, PendingIntents>) -> Vec<OpenIntent> {
    pending.0.lock().unwrap_or_else(|e| e.into_inner()).drain(..).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::joincode::{encode, JoinInfo};
    use base64::{engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD}, Engine as _};

    fn code() -> String {
        encode(&JoinInfo { addresses: vec!["192.168.1.20".parse().unwrap()], port: 9847, pin: None, internet_id: None }).unwrap()
    }
    fn known(g: &str) -> bool {
        matches!(g, "sims4" | "7daystodie")
    }
    fn is_invalid(t: Option<OpenTarget>) -> bool {
        matches!(t, Some(OpenTarget::Invalid(_)))
    }
    fn pack_json() -> String {
        serde_json::to_string(&crate::testutil::test_pack("sims4", &[("Mods/a.package", b"A")])).unwrap()
    }

    #[test]
    fn join_link_valid() {
        let c = code();
        let t = classify(&format!("synccrate://join/{c}?game=sims4"), known);
        assert_eq!(t, Some(OpenTarget::Join { code: c.clone(), game_id: "sims4".into() }));
        assert_eq!(
            classify(&format!("synccrate://join/{c}?game=7daystodie"), known),
            Some(OpenTarget::Join { code: c, game_id: "7daystodie".into() })
        );
    }

    #[test]
    fn scheme_and_kind_are_case_insensitive() {
        let c = code();
        let t = classify(&format!("SyncCrate://JOIN/{}?Game=Sims4", c.to_lowercase()), known);
        assert_eq!(t, Some(OpenTarget::Join { code: c, game_id: "sims4".into() }));
    }

    #[test]
    fn discord_trailing_junk_is_stripped() {
        let c = code();
        for suffix in [")", ".", "),", "**", "/", "/.", "!", ">"] {
            let t = classify(&format!("synccrate://join/{c}?game=sims4{suffix}"), known);
            assert_eq!(t, Some(OpenTarget::Join { code: c.clone(), game_id: "sims4".into() }), "suffix {suffix:?}");
        }
        assert!(matches!(classify(&format!("<synccrate://join/{c}?game=sims4>"), known), Some(OpenTarget::Join { .. })));
        assert!(matches!(classify(&format!("synccrate://join/{c}/?game=sims4"), known), Some(OpenTarget::Join { .. })));
    }

    #[test]
    fn join_link_percent_encoded() {
        let c = code();
        let t = classify(&format!("synccrate://join/{}?game=sims%34", c.replace('-', "%2D")), known);
        assert_eq!(t, Some(OpenTarget::Join { code: c, game_id: "sims4".into() }));
    }

    #[test]
    fn join_link_rejects_bad_code_and_unknown_game() {
        let c = code();
        assert!(is_invalid(classify("synccrate://join/SC-NOPE-NOPE?game=sims4", known)));
        assert!(is_invalid(classify("synccrate://join/?game=sims4", known)));
        // One changed character breaks the checksum.
        let mut broken = c.clone();
        let last = broken.pop().unwrap();
        broken.push(if last == '0' { '1' } else { '0' });
        assert!(is_invalid(classify(&format!("synccrate://join/{broken}?game=sims4"), known)));
        assert!(is_invalid(classify(&format!("synccrate://join/{c}?game=notagame"), known)));
        assert!(is_invalid(classify(&format!("synccrate://join/{c}?game=../../etc"), |_| true)));
        assert!(is_invalid(classify(&format!("synccrate://join/{c}"), known)));
        assert!(is_invalid(classify(&format!("synccrate://join/{c}?game="), known)));
        assert!(is_invalid(classify(&format!("synccrate://join/{}?game=sims4", "A".repeat(300)), known)));
    }

    #[test]
    fn crew_link_valid_with_chat_junk_and_invalid_payloads() {
        let from = crate::crews::node_id_hex(&iroh::SecretKey::from_bytes(&[4; 32]).public());
        let crew = crate::crews::Crew {
            id: crate::crews::new_crew_id(),
            name: "Sunday Sims Crew".into(),
            name_updated_at: 0,
            games: vec!["sims4".into()],
            members: vec![],
            sets: Default::default(),
            last_host: None,
            created_at: 0,
        };
        let link = crate::crews::encode_invite(&crew, &from, "Host").unwrap();
        for raw in [link.clone(), format!("{link})."), format!("<{link}>"), format!("{link}/"), link.replacen("crew", "CREW", 1)] {
            match classify(&raw, known) {
                Some(OpenTarget::Crew(inv)) => {
                    assert_eq!(inv.id, crew.id);
                    assert!(matches!(resolve(OpenTarget::Crew(inv)), OpenIntent::Crew { .. }));
                }
                other => panic!("{raw}: {other:?}"),
            }
        }
        assert!(is_invalid(classify("synccrate://crew/", known)));
        assert!(is_invalid(classify("synccrate://crew/!!!", known)));
        // Unknown game: refused, with the game named.
        let mut other = crew.clone();
        other.games = vec!["notagame".into()];
        let link = crate::crews::encode_invite(&other, &from, "Host").unwrap();
        assert!(matches!(classify(&link, known), Some(OpenTarget::Invalid(r)) if r.contains("notagame")));
        let huge = format!("synccrate://crew/{}", "A".repeat(crate::crews::MAX_INVITE_BYTES + 10));
        assert!(is_invalid(classify(&huge, known)));
    }

    #[test]
    fn unknown_kind_and_oversized_input() {
        assert!(is_invalid(classify("synccrate://settings/reset", known)));
        assert!(is_invalid(classify("synccrate://", known)));
        let huge = format!("synccrate://pack/{}", "A".repeat(MAX_INPUT_BYTES));
        assert_eq!(classify(&huge, known), Some(OpenTarget::Invalid("That link is too long to be a SyncCrate link.".into())));
        // Not our scheme: ignored, not reported.
        assert_eq!(classify("https://example.com/x", known), None);
        assert_eq!(classify("--flag", known), None);
        assert_eq!(classify("C:\\Program Files\\SyncCrate\\synccrate.exe", known), None);
        assert_eq!(classify("", known), None);
        assert_eq!(classify(&format!("C:\\{}.scpack", "a".repeat(MAX_INPUT_BYTES)), known), None);
    }

    #[test]
    fn pack_link_valid_both_alphabets_and_junk() {
        let json = pack_json();
        let safe = URL_SAFE_NO_PAD.encode(&json);
        let std_padded = STANDARD.encode(&json);
        for link in [
            format!("synccrate://pack/{safe}"),
            format!("SYNCCRATE://Pack/{safe})."),
            format!("synccrate://pack/{std_padded}"),
            format!("synccrate://pack/{}", std_padded.replace('+', "%2B").replace('/', "%2F").replace('=', "%3D")),
        ] {
            let Some(OpenTarget::PackLink(p)) = classify(&link, known) else { panic!("not a pack link: {link}") };
            match resolve(OpenTarget::PackLink(p)) {
                OpenIntent::Pack { pack } => assert_eq!(pack.game_id, "sims4"),
                other => panic!("{link}: {other:?}"),
            }
        }
    }

    #[test]
    fn pack_link_invalid_payloads() {
        assert!(is_invalid(classify("synccrate://pack/", known)));
        let garbage = classify("synccrate://pack/!!!notbase64", known).unwrap();
        assert!(matches!(resolve(garbage), OpenIntent::Invalid { .. }));
        // Valid base64, but the pack escapes the game folder: the modpack
        // validator must still run.
        let mut p = crate::testutil::test_pack("sims4", &[("Mods/a.package", b"A")]);
        p.files[0].relative_path = "../../evil.dll".into();
        let link = format!("synccrate://pack/{}", URL_SAFE_NO_PAD.encode(serde_json::to_string(&p).unwrap()));
        let OpenIntent::Invalid { reason } = resolve(classify(&link, known).unwrap()) else { panic!("traversal accepted") };
        assert!(reason.contains("Invalid file path"), "{reason}");
        p.files[0].relative_path = "C:\\Windows\\x.dll".into();
        let link = format!("synccrate://pack/{}", URL_SAFE_NO_PAD.encode(serde_json::to_string(&p).unwrap()));
        assert!(matches!(resolve(classify(&link, known).unwrap()), OpenIntent::Invalid { .. }));
    }

    #[test]
    fn pack_paths() {
        assert_eq!(classify("C:\\Users\\me\\My Pack.SCPACK", known), Some(OpenTarget::PackFile("C:\\Users\\me\\My Pack.SCPACK".into())));
        assert_eq!(classify("\"/home/me/pack.scpack\"", known), Some(OpenTarget::PackFile("/home/me/pack.scpack".into())));
        assert_eq!(classify(".scpack", known), None);
        assert_eq!(classify("/home/me/pack.scpack.exe", known), None);

        let dir = crate::testutil::temp_dir("open-intent-pack");
        let good = dir.join("good.scpack");
        std::fs::write(&good, pack_json()).unwrap();
        assert!(matches!(resolve(OpenTarget::PackFile(good)), OpenIntent::Pack { .. }));
        let bad = dir.join("bad.scpack");
        std::fs::write(&bad, "{not json").unwrap();
        assert!(matches!(resolve(OpenTarget::PackFile(bad)), OpenIntent::Invalid { .. }));
        assert!(matches!(resolve(OpenTarget::PackFile(dir.join("missing.scpack"))), OpenIntent::Invalid { .. }));
        // A directory named like a pack isn't a file.
        std::fs::create_dir_all(dir.join("folder.scpack")).unwrap();
        assert!(matches!(resolve(OpenTarget::PackFile(dir.join("folder.scpack"))), OpenIntent::Invalid { .. }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn percent_decode_edge_cases() {
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz%4"), "%zz%4");
        assert_eq!(percent_decode("%E2%9C%93"), "✓");
        assert_eq!(percent_decode("✓%✓%4✓"), "✓%✓%4✓");
        assert!(is_invalid(classify("synccrate://join/✓%✓?game=✓", known)));
    }
}
