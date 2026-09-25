//! Crews: a remembered friend group ("Sunday Sims Crew") — who's in it, which
//! games it plays, and the crew's canonical mod set per game (a `ModPack`, no
//! second format). Stored locally per user (`<config>/synccrate/crews.json`);
//! there is no server, so crew data only moves during a normal session, from
//! host to client, inside the optional `crews` fields of Hello/Welcome.
//!
//! Trust model: a crew is an address book plus a shared file list. Membership
//! grants nothing — the PIN and the wrong-game checks gate every session
//! exactly as before, and nothing here writes files or syncs. Node ids are
//! only authenticated over iroh (the QUIC handshake proves the key); a node id
//! claimed over LAN TCP is display data, never access control. "Remove" is a
//! last-writer-wins tombstone that spreads as members connect; it's advisory
//! (the removed person keeps their copy), changing the PIN is what locks
//! someone out.
//!
//! Everything arriving from a peer or an invite link is untrusted and goes
//! through `validate_welcome` / `decode_invite` before it touches the store.
use crate::state::ModPack;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;
pub const INVITE_VERSION: u32 = 1;
pub const MAX_CREWS: usize = 32;
pub const MAX_MEMBERS: usize = 64;
pub const MAX_GAMES: usize = 16;
const MAX_NAME_CHARS: usize = 64;
pub const INVITE_PREFIX: &str = "synccrate://crew/";
/// Invites carry ids and names only; a few hundred bytes in practice.
pub const MAX_INVITE_BYTES: usize = 2048;
/// A crew set rides inside Welcome, whose frame is capped at 10 MB
/// (`protocol::MAX_MESSAGE_SIZE`); `protocol::MAX_MANIFEST_FILES` keeps a set
/// comfortably under that.
pub const MAX_SET_FILES: usize = crate::network::protocol::MAX_MANIFEST_FILES;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrewMember {
    /// Hex of the member's iroh endpoint id (stable: `iroh_secret_key` is persisted).
    pub node_id: String,
    pub name: String,
    /// Last-writer-wins clock for `name`/`removed`.
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub removed: bool,
    #[serde(default)]
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewSet {
    pub pack: ModPack,
    pub version: u64,
    pub published_at: u64,
    #[serde(default)]
    pub publisher: String,
}

impl CrewSet {
    /// Newest wins: version, then publish time, then publisher (so two
    /// members who both published "v3" offline still converge).
    fn key(&self) -> (u64, u64, &str) {
        (self.version, self.published_at, self.publisher.as_str())
    }
}

/// The last host this crew was synced from, so "Reconnect" needs no code.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CrewHost {
    pub node_id: String,
    pub name: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub game_id: String,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Crew {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_updated_at: u64,
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub members: Vec<CrewMember>,
    /// Canonical set per game id.
    #[serde(default)]
    pub sets: BTreeMap<String, CrewSet>,
    #[serde(default)]
    pub last_host: Option<CrewHost>,
    #[serde(default)]
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewStore {
    pub format_version: u32,
    #[serde(default)]
    pub crews: Vec<Crew>,
}

impl Default for CrewStore {
    fn default() -> Self {
        Self { format_version: FORMAT_VERSION, crews: Vec::new() }
    }
}

impl CrewStore {
    pub fn get(&self, id: &str) -> Option<&Crew> {
        self.crews.iter().find(|c| c.id == id)
    }
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Crew> {
        self.crews.iter_mut().find(|c| c.id == id)
    }
}

// ---------------------------------------------------------------------------
// Wire types (optional fields of Hello / Welcome)

/// Client → host: "I'm in this crew, and this is the set I already have for
/// the game I'm joining with".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CrewHello {
    pub id: String,
    #[serde(default)]
    pub set_version: u64,
    #[serde(default)]
    pub set_published_at: u64,
}

/// Host → client, only for crews the client named.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrewWelcome {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_updated_at: u64,
    #[serde(default)]
    pub games: Vec<String>,
    #[serde(default)]
    pub members: Vec<CrewMember>,
    /// Only when the host's set for this session's game is newer than the
    /// client's (keeps a reconnect Welcome small).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set: Option<CrewSet>,
}

// ---------------------------------------------------------------------------
// Validation (untrusted input)

pub fn is_valid_crew_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn node_id_hex(id: &iroh::EndpointId) -> String {
    hex::encode(id.as_bytes())
}

pub fn parse_node_id(id: &str) -> Option<iroh::EndpointId> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let bytes: [u8; 32] = hex::decode(id).ok()?.try_into().ok()?;
    iroh::EndpointId::from_bytes(&bytes).ok()
}

pub fn is_valid_node_id(id: &str) -> bool {
    parse_node_id(id).is_some()
}

/// Strip control characters, trim, cap length. `None` if nothing is left.
pub fn clean_name(name: &str) -> Option<String> {
    let cleaned: String = name.chars().filter(|c| !c.is_control() && !crate::chat::is_bidi_control(*c)).collect();
    let trimmed: String = cleaned.trim().chars().take(MAX_NAME_CHARS).collect();
    let trimmed = trimmed.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn is_plausible_game_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn new_crew_id() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

/// Validate and normalise a host's Welcome entry. `is_known_game` rejects
/// games this app version doesn't have (an unknown id could never be synced).
const MAX_CLOCK_SKEW_SECS: u64 = 24 * 3600;
/// Real sets are published a handful of times; anything near u64::MAX is an
/// attempt to win every merge (and overflow the next publish).
const MAX_SET_VERSION: u64 = 1_000_000;

pub fn validate_welcome(mut w: CrewWelcome, session_game: &str, is_known_game: impl Fn(&str) -> bool) -> Result<CrewWelcome, String> {
    if !is_valid_crew_id(&w.id) {
        return Err("invalid crew id".into());
    }
    w.name = clean_name(&w.name).ok_or("crew has no name")?;
    if w.games.len() > MAX_GAMES || w.members.len() > MAX_MEMBERS {
        return Err("crew is too large".into());
    }
    w.games.retain(|g| is_plausible_game_id(g) && is_known_game(g));
    w.games.dedup();
    // Clocks from a host are clamped to "now + a day": a u64::MAX timestamp
    // would win every later last-writer-wins merge and freeze a name or a
    // removal forever.
    let latest = crate::utils::timestamp_now().saturating_add(MAX_CLOCK_SKEW_SECS);
    w.name_updated_at = w.name_updated_at.min(latest);
    for m in &mut w.members {
        if !is_valid_node_id(&m.node_id) {
            return Err("invalid member id".into());
        }
        m.name = clean_name(&m.name).unwrap_or_else(|| "Unknown".into());
        m.updated_at = m.updated_at.min(latest);
        m.last_seen = m.last_seen.min(latest);
    }
    if w.set.as_ref().is_some_and(|s| s.version > MAX_SET_VERSION) {
        return Err("crew set version is out of range".into());
    }
    if let Some(set) = &mut w.set {
        set.published_at = set.published_at.min(latest);
    }
    if let Some(set) = &w.set {
        crate::commands::modpack::validate_pack(&set.pack)?;
        if set.pack.game_id != session_game {
            return Err("crew set is for a different game than this session".into());
        }
        if set.pack.files.len() > MAX_SET_FILES {
            return Err("crew set is too large".into());
        }
    }
    if let Some(set) = &mut w.set {
        set.publisher = clean_name(&set.publisher).unwrap_or_default();
    }
    Ok(w)
}

// ---------------------------------------------------------------------------
// Merge rules

/// Upsert by node id, last writer wins on `name`/`removed`; `last_seen`
/// only moves forward. New members are dropped past `MAX_MEMBERS`. Returns
/// whether anything changed.
pub fn merge_member(members: &mut Vec<CrewMember>, incoming: CrewMember) -> bool {
    if let Some(m) = members.iter_mut().find(|m| m.node_id == incoming.node_id) {
        let mut changed = false;
        if incoming.updated_at > m.updated_at {
            m.name = incoming.name;
            m.removed = incoming.removed;
            m.updated_at = incoming.updated_at;
            changed = true;
        }
        if incoming.last_seen > m.last_seen {
            m.last_seen = incoming.last_seen;
            changed = true;
        }
        return changed;
    }
    if members.len() >= MAX_MEMBERS {
        return false;
    }
    members.push(incoming);
    true
}

/// Whether `incoming` should replace `current`.
pub fn set_is_newer(incoming: &CrewSet, current: Option<&CrewSet>) -> bool {
    current.map_or(true, |c| incoming.key() > c.key())
}

fn add_game(games: &mut Vec<String>, game: &str) -> bool {
    if game.is_empty() || games.iter().any(|g| g == game) || games.len() >= MAX_GAMES {
        return false;
    }
    games.push(game.to_string());
    true
}

/// The client's announcement for `crew` when joining with `game` selected.
pub fn hello_for(crew: &Crew, game: &str) -> CrewHello {
    let set = crew.sets.get(game);
    CrewHello {
        id: crew.id.clone(),
        set_version: set.map_or(0, |s| s.version),
        set_published_at: set.map_or(0, |s| s.published_at),
    }
}

/// Host side: record the joining member and build the Welcome entry.
/// `None` when the member was removed from the crew — the session itself
/// still goes ahead (the PIN decided that), they just get no crew data.
pub fn host_answer(crew: &mut Crew, hello: &CrewHello, member: Option<(String, String)>, game: &str, now: u64) -> (Option<CrewWelcome>, bool) {
    let mut changed = false;
    if let Some((node_id, name)) = member {
        match crew.members.iter_mut().find(|m| m.node_id == node_id) {
            Some(m) if m.removed => return (None, false),
            Some(m) => {
                if m.name != name {
                    m.name = name;
                    m.updated_at = now.max(m.updated_at.saturating_add(1));
                }
                m.last_seen = now;
                changed = true;
            }
            None => {
                changed = merge_member(
                    &mut crew.members,
                    CrewMember { node_id, name, updated_at: now, removed: false, last_seen: now },
                );
            }
        }
    }
    let theirs = (hello.set_version, hello.set_published_at);
    let set = crew
        .sets
        .get(game)
        .filter(|s| (s.version, s.published_at) > theirs)
        .cloned();
    let welcome = CrewWelcome {
        id: crew.id.clone(),
        name: crew.name.clone(),
        name_updated_at: crew.name_updated_at,
        games: crew.games.clone(),
        members: crew.members.clone(),
        set,
    };
    (Some(welcome), changed)
}

/// Client side: merge a validated Welcome entry into our copy of the crew.
pub fn client_apply(crew: &mut Crew, w: CrewWelcome, host: Option<CrewHost>) -> bool {
    let mut changed = false;
    if w.name_updated_at > crew.name_updated_at {
        crew.name = w.name;
        crew.name_updated_at = w.name_updated_at;
        changed = true;
    }
    for g in &w.games {
        changed |= add_game(&mut crew.games, g);
    }
    for m in w.members {
        changed |= merge_member(&mut crew.members, m);
    }
    if let Some(set) = w.set {
        let game = set.pack.game_id.clone();
        if set_is_newer(&set, crew.sets.get(&game)) {
            add_game(&mut crew.games, &game);
            crew.sets.insert(game, set);
            changed = true;
        }
    }
    if let Some(host) = host {
        let seen = CrewMember { node_id: host.node_id.clone(), name: host.name.clone(), updated_at: 0, removed: false, last_seen: host.at };
        merge_member(&mut crew.members, seen);
        crew.last_host = Some(host);
        changed = true;
    }
    changed
}

/// Local "publish as crew set": bump past whatever we've seen.
pub fn publish_set<'a>(crew: &'a mut Crew, pack: ModPack, publisher: &str, now: u64) -> &'a CrewSet {
    let game = pack.game_id.clone();
    let version = crew.sets.get(&game).map_or(0, |s| s.version).saturating_add(1);
    add_game(&mut crew.games, &game);
    crew.sets.insert(game.clone(), CrewSet { pack, version, published_at: now, publisher: publisher.to_string() });
    &crew.sets[&game]
}

/// Mark a member removed (or restore them). The tombstone's clock must beat
/// every copy of the member entry out there, so it's at least `now`.
pub fn set_member_removed(crew: &mut Crew, node_id: &str, removed: bool, now: u64) -> Result<(), String> {
    let m = crew.members.iter_mut().find(|m| m.node_id == node_id).ok_or("Not a member of this crew")?;
    m.removed = removed;
    m.updated_at = now.max(m.updated_at.saturating_add(1));
    Ok(())
}

// ---------------------------------------------------------------------------
// Handshake glue (called from `network::transfer` with the AppState lock held;
// everything here is in-memory work plus one small file write)

/// The client's Hello entries: every crew it's in (capped), keyed to the
/// game it's joining with. Hosts that aren't in a crew just ignore them.
/// Only crews that `host` (a node id iroh proved) is a current member of:
/// crew ids sent to any host leaked to spoofed hosts and strangers' codes.
pub fn hellos_for_host(state: &crate::state::AppState, host: &str) -> Vec<CrewHello> {
    state
        .crews
        .crews
        .iter()
        .filter(|c| is_active_member(c, host))
        .take(MAX_CREWS)
        .map(|c| hello_for(c, &state.active_game))
        .collect()
}

pub fn is_active_member(crew: &Crew, node_id: &str) -> bool {
    crew.members.iter().any(|m| m.node_id == node_id && !m.removed)
}

/// Host side. `member` is (node id, display name): the authenticated id
/// over iroh, the claimed one over TCP.
pub fn host_handshake(state: &mut crate::state::AppState, hellos: &[CrewHello], member: Option<(String, String)>) -> Vec<CrewWelcome> {
    let game = state.active_game.clone();
    let now = crate::utils::timestamp_now();
    let mut out = Vec::new();
    let mut changed = false;
    for h in hellos.iter().take(MAX_CREWS) {
        if let Some(crew) = state.crews.get_mut(&h.id) {
            let (w, c) = host_answer(crew, h, member.clone(), &game, now);
            changed |= c;
            out.extend(w);
        }
    }
    // A Welcome over the frame cap would kill the whole session; the set is
    // the only big part, so drop sets first (the client just stays behind
    // and sees that on the crew page).
    let size = serde_json::to_vec(&out).map(|v| v.len()).unwrap_or(usize::MAX);
    if size > crate::network::protocol::MAX_CREW_WELCOME_BYTES {
        log::warn!("Crew set too large for the handshake ({size} bytes); sending crew info without it");
        for w in &mut out {
            w.set = None;
        }
    }
    if changed {
        persist(state);
    }
    out
}

/// Client side: merge the host's answers into crews we're in (a Welcome can't
/// add a crew we never joined). Invalid entries are skipped with a log line,
/// never fatal: the sync session itself doesn't depend on crew data.
pub fn client_handshake(state: &mut crate::state::AppState, welcomes: Vec<CrewWelcome>, host: CrewHost) -> bool {
    let game = state.active_game.clone();
    let registry = crate::registry::build_registry_map(&state.game_registry);
    let mut changed = false;
    for w in welcomes.into_iter().take(MAX_CREWS) {
        let id = w.id.clone();
        let w = match validate_welcome(w, &game, |g| registry.contains_key(g)) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("Ignoring crew data from host: {e}");
                continue;
            }
        };
        // Only a current member (by the id iroh proved) speaks for a crew.
        if let Some(crew) = state.crews.get_mut(&id).filter(|c| is_active_member(c, &host.node_id)) {
            changed |= client_apply(crew, w, Some(host.clone()));
        }
    }
    if changed {
        persist(state);
    }
    changed
}

// ---------------------------------------------------------------------------
// Invites

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrewInvite {
    pub v: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub games: Vec<String>,
    pub from_node: String,
    pub from_name: String,
}

/// No PIN and no join code: invites get forwarded, so they carry nothing
/// that lets anyone into a session.
pub fn encode_invite(crew: &Crew, from_node: &str, from_name: &str) -> Result<String, String> {
    let invite = CrewInvite {
        v: INVITE_VERSION,
        id: crew.id.clone(),
        name: crew.name.clone(),
        games: crew.games.clone(),
        from_node: from_node.to_string(),
        from_name: clean_name(from_name).unwrap_or_else(|| "A friend".into()),
    };
    let json = serde_json::to_vec(&invite).map_err(|e| e.to_string())?;
    Ok(format!("{INVITE_PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
}

/// Decode the base64 payload of a `synccrate://crew/` link (already
/// stripped of scheme, percent-encoding and trailing chat junk by
/// `open_intent::classify`).
pub fn decode_invite(payload: &str, is_known_game: impl Fn(&str) -> bool) -> Result<CrewInvite, String> {
    let payload = payload.trim();
    if payload.is_empty() {
        return Err("That crew invite is empty — it may have been cut off.".into());
    }
    if payload.len() > MAX_INVITE_BYTES {
        return Err("That crew invite is too long to be real.".into());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| "That crew invite is damaged — ask for a fresh link.".to_string())?;
    let inv: CrewInvite =
        serde_json::from_slice(&bytes).map_err(|_| "That crew invite is damaged — ask for a fresh link.".to_string())?;
    validate_invite(inv, is_known_game)
}

/// Also used on an invite handed back by the frontend (`join_crew`).
pub fn validate_invite(mut inv: CrewInvite, is_known_game: impl Fn(&str) -> bool) -> Result<CrewInvite, String> {
    if inv.v == 0 || inv.v > INVITE_VERSION {
        return Err("This crew invite needs a newer version of SyncCrate.".into());
    }
    if !is_valid_crew_id(&inv.id) {
        return Err("That crew invite has an invalid crew id.".into());
    }
    if !is_valid_node_id(&inv.from_node) {
        return Err("That crew invite has an invalid member id.".into());
    }
    inv.name = clean_name(&inv.name).ok_or("That crew invite has no crew name.")?;
    inv.from_name = clean_name(&inv.from_name).unwrap_or_else(|| "A friend".into());
    if inv.games.len() > MAX_GAMES {
        return Err("That crew invite lists too many games.".into());
    }
    if let Some(g) = inv.games.iter().find(|g| !is_plausible_game_id(g) || !is_known_game(g)) {
        return Err(format!("That crew plays a game this version of SyncCrate doesn't know ({g}). Update SyncCrate and try again."));
    }
    Ok(inv)
}

/// A new local crew from an invite: just the inviter and ourselves.
pub fn crew_from_invite(inv: &CrewInvite, me: CrewMember, now: u64) -> Crew {
    let mut members = vec![CrewMember {
        node_id: inv.from_node.clone(),
        name: inv.from_name.clone(),
        updated_at: 0,
        removed: false,
        last_seen: 0,
    }];
    if me.node_id != inv.from_node {
        members.push(me);
    }
    Crew {
        id: inv.id.clone(),
        name: inv.name.clone(),
        name_updated_at: 0,
        games: inv.games.clone(),
        members,
        sets: BTreeMap::new(),
        last_host: None,
        created_at: now,
    }
}

// ---------------------------------------------------------------------------
// Storage

pub fn store_path() -> PathBuf {
    let dir = crate::utils::config_root().join("synccrate");
    std::fs::create_dir_all(&dir).ok();
    dir.join("crews.json")
}

/// Missing file → empty store. A file from a newer app (or a corrupt one)
/// is an error, so the caller can avoid overwriting it.
pub fn load_store(path: &Path) -> Result<CrewStore, String> {
    let raw = match std::fs::read(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(CrewStore::default()),
        Err(e) => return Err(format!("Could not read crews: {e}")),
    };
    let store: CrewStore = serde_json::from_slice(&raw).map_err(|e| format!("Crews file is damaged: {e}"))?;
    if store.format_version == 0 || store.format_version > FORMAT_VERSION {
        return Err(format!(
            "Crews were saved by a newer SyncCrate (format {}); update to use them.",
            store.format_version
        ));
    }
    Ok(store)
}

/// Write-then-rename so a crash mid-write can't leave a half file.
pub fn save_store(path: &Path, store: &CrewStore) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data).map_err(|e| format!("Could not save crews: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Could not save crews: {e}"))
}

/// Persist the in-memory store if this `AppState` has a backing file (tests
/// and a store that failed to load run without one).
pub fn persist(state: &crate::state::AppState) {
    if let Some(path) = &state.crews_path {
        if let Err(e) = save_store(path, &state.crews) {
            log::warn!("{e}");
        }
    }
}

/// "You're N files behind the crew": what "get missing" + "apply exactly"
/// would still change, per the ordinary pack compare.
pub fn behind(cmp: &crate::commands::modpack::PackComparison) -> usize {
    cmp.missing.len() + cmp.different.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_clocks_and_versions_are_clamped() {
        let c = crew();
        let far = u64::MAX;
        let mut w = CrewWelcome { id: c.id.clone(), name: "C".into(), name_updated_at: far, games: vec![], members: vec![], set: None };
        w.members.push(CrewMember { node_id: node(2), name: "Ann".into(), updated_at: far, removed: true, last_seen: far });
        let v = validate_welcome(w.clone(), "sims4", known).unwrap();
        let limit = crate::utils::timestamp_now() + MAX_CLOCK_SKEW_SECS + 5;
        assert!(v.name_updated_at <= limit && v.members[0].updated_at <= limit && v.members[0].last_seen <= limit);
        // A u64::MAX set version would win every merge and overflow the next publish.
        let mut p = crate::testutil::test_pack("sims4", &[("Mods/a.package", b"A")]);
        p.name = "S".into();
        w.set = Some(CrewSet { pack: p, version: far, published_at: 1, publisher: "H".into() });
        assert!(validate_welcome(w, "sims4", known).is_err());
        // And publishing past a huge local version saturates instead of wrapping to 0.
        let mut c = crew();
        c.sets.insert("sims4".into(), CrewSet { pack: pack("sims4", 1), version: u64::MAX, published_at: 1, publisher: "X".into() });
        assert_eq!(publish_set(&mut c, pack("sims4", 1), "Me", 2).version, u64::MAX);
    }

    #[test]
    fn bidi_overrides_are_stripped_from_names() {
        assert_eq!(clean_name("Ann\u{202e}gpj.exe").as_deref(), Some("Anngpj.exe"));
    }

    pub(crate) fn node(n: u8) -> String {
        // Real ed25519 public keys (from deterministic secret keys), since
        // `parse_node_id` rejects bytes that aren't a valid curve point.
        node_id_hex(&iroh::SecretKey::from_bytes(&[n; 32]).public())
    }

    fn pack(game: &str, n: usize) -> ModPack {
        let mut p = crate::testutil::test_pack(game, &[]);
        p.files = (0..n)
            .map(|i| crate::state::PackFile { relative_path: format!("Mods/{i}.package"), size: 1, hash: "a".repeat(64) })
            .collect();
        p
    }

    fn crew() -> Crew {
        Crew {
            id: new_crew_id(),
            name: "Sunday Sims Crew".into(),
            name_updated_at: 1,
            games: vec!["sims4".into()],
            members: vec![CrewMember { node_id: node(1), name: "Host".into(), updated_at: 1, removed: false, last_seen: 0 }],
            sets: BTreeMap::new(),
            last_host: None,
            created_at: 1,
        }
    }

    fn known(g: &str) -> bool {
        g == "sims4" || g == "ets2"
    }

    #[test]
    fn store_round_trips_through_disk() {
        let dir = crate::testutil::temp_dir("crews-store");
        let path = dir.join("crews.json");
        assert!(load_store(&path).unwrap().crews.is_empty(), "missing file is an empty store");
        let mut store = CrewStore::default();
        let mut c = crew();
        publish_set(&mut c, pack("sims4", 2), "Host", 5);
        store.crews.push(c.clone());
        save_store(&path, &store).unwrap();
        let back = load_store(&path).unwrap();
        assert_eq!(back.crews.len(), 1);
        assert_eq!(back.crews[0].id, c.id);
        assert_eq!(back.crews[0].sets["sims4"].version, 1);
        assert_eq!(back.crews[0].sets["sims4"].pack.files.len(), 2);
        assert!(!dir.join("crews.json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_from_a_newer_app_or_damaged_is_an_error_not_empty() {
        let dir = crate::testutil::temp_dir("crews-future");
        let path = dir.join("crews.json");
        std::fs::write(&path, r#"{"format_version":2,"crews":[]}"#).unwrap();
        assert!(load_store(&path).unwrap_err().contains("newer"));
        std::fs::write(&path, "{not json").unwrap();
        assert!(load_store(&path).is_err());
        // Unknown extra fields from a newer minor version are fine.
        std::fs::write(&path, r#"{"format_version":1,"crews":[],"future":true}"#).unwrap();
        assert!(load_store(&path).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invite_round_trips() {
        let c = crew();
        let link = encode_invite(&c, &node(1), "Host").unwrap();
        let payload = link.strip_prefix(INVITE_PREFIX).unwrap();
        let inv = decode_invite(payload, known).unwrap();
        assert_eq!(inv.id, c.id);
        assert_eq!(inv.name, "Sunday Sims Crew");
        assert_eq!(inv.games, vec!["sims4".to_string()]);
        assert_eq!(inv.from_node, node(1));
        assert!(!link.contains("pin"), "an invite must never carry a PIN");
    }

    fn encode_raw(v: serde_json::Value) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&v).unwrap())
    }

    #[test]
    fn invite_rejects_untrusted_garbage() {
        let good = serde_json::json!({"v":1,"id":"0123456789abcdef0123456789abcdef","name":"Crew","games":["sims4"],"from_node":node(1),"from_name":"A"});
        assert!(decode_invite(&encode_raw(good.clone()), known).is_ok());

        let with = |k: &str, v: serde_json::Value| {
            let mut j = good.clone();
            j[k] = v;
            encode_raw(j)
        };
        assert!(decode_invite("", known).is_err());
        assert!(decode_invite(&"A".repeat(MAX_INVITE_BYTES + 1), known).unwrap_err().contains("too long"));
        assert!(decode_invite("!!!not-base64", known).is_err());
        assert!(decode_invite(&with("v", 2.into()), known).unwrap_err().contains("newer"));
        assert!(decode_invite(&with("id", "../../etc".into()), known).unwrap_err().contains("crew id"));
        assert!(decode_invite(&with("id", "0123456789ABCDEF0123456789ABCDEF".into()), known).is_err());
        assert!(decode_invite(&with("from_node", "zz".into()), known).unwrap_err().contains("member id"));
        assert!(decode_invite(&with("from_node", node(1).to_uppercase().into()), known).is_err());
        assert!(decode_invite(&with("from_node", node(1)[..63].into()), known).is_err());
        assert!(decode_invite(&with("name", "\u{0007}  ".into()), known).unwrap_err().contains("name"));
        assert!(decode_invite(&with("games", serde_json::json!(["notagame"])), known).unwrap_err().contains("notagame"));
        assert!(decode_invite(&with("games", serde_json::json!(["Sims4; rm"])), known).is_err());
        let names = with("name", "x".repeat(500).into());
        assert_eq!(decode_invite(&names, known).unwrap().name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn member_merge_is_last_writer_wins() {
        let mut members = vec![CrewMember { node_id: node(2), name: "Ann".into(), updated_at: 10, removed: false, last_seen: 3 }];
        // Older rename loses, but a newer last_seen still moves forward.
        let older = CrewMember { node_id: node(2), name: "Old".into(), updated_at: 5, removed: false, last_seen: 9 };
        assert!(merge_member(&mut members, older));
        assert_eq!(members[0].name, "Ann");
        assert_eq!(members[0].last_seen, 9);
        // A newer tombstone wins.
        let removed = CrewMember { node_id: node(2), name: "Ann".into(), updated_at: 11, removed: true, last_seen: 0 };
        assert!(merge_member(&mut members, removed));
        assert!(members[0].removed);
        // Re-adding with an older clock can't resurrect them.
        let stale = CrewMember { node_id: node(2), name: "Ann".into(), updated_at: 10, removed: false, last_seen: 0 };
        assert!(!merge_member(&mut members, stale));
        assert!(members[0].removed);
        // A new member is added; the cap holds.
        assert!(merge_member(&mut members, CrewMember { node_id: node(3), name: "Bo".into(), updated_at: 1, removed: false, last_seen: 0 }));
        assert_eq!(members.len(), 2);
        while members.len() < MAX_MEMBERS {
            let n = members.len() as u8 + 10;
            members.push(CrewMember { node_id: node(n), name: "x".into(), updated_at: 0, removed: false, last_seen: 0 });
        }
        assert!(!merge_member(&mut members, CrewMember { node_id: node(200), name: "late".into(), updated_at: 1, removed: false, last_seen: 0 }));
        assert_eq!(members.len(), MAX_MEMBERS);
    }

    #[test]
    fn remove_and_restore_beat_every_earlier_copy() {
        let mut c = crew();
        c.members[0].updated_at = 100; // a clock from a PC whose time runs ahead
        set_member_removed(&mut c, &node(1), true, 50).unwrap();
        assert!(c.members[0].removed);
        assert!(c.members[0].updated_at > 100);
        set_member_removed(&mut c, &node(1), false, 50).unwrap();
        assert!(!c.members[0].removed);
        assert!(set_member_removed(&mut c, &node(9), true, 1).is_err());
    }

    #[test]
    fn host_records_member_and_sends_set_only_when_newer() {
        let mut c = crew();
        publish_set(&mut c, pack("sims4", 3), "Host", 100);
        let fresh = CrewHello { id: c.id.clone(), set_version: 0, set_published_at: 0 };
        let (w, changed) = host_answer(&mut c, &fresh, Some((node(2), "Ann".into())), "sims4", 200);
        let w = w.unwrap();
        assert!(changed);
        assert_eq!(w.set.as_ref().unwrap().pack.files.len(), 3);
        assert!(c.members.iter().any(|m| m.node_id == node(2) && m.last_seen == 200));

        let current = hello_for(&c, "sims4");
        let (w, _) = host_answer(&mut c, &current, Some((node(2), "Ann".into())), "sims4", 300);
        assert!(w.unwrap().set.is_none(), "an up-to-date member gets no set");
        // No set for a game the crew hasn't published.
        let (w, _) = host_answer(&mut c, &fresh, None, "ets2", 300);
        assert!(w.unwrap().set.is_none());

        // A removed member gets no crew data (the session itself is the PIN's call).
        set_member_removed(&mut c, &node(2), true, 400).unwrap();
        let (w, _) = host_answer(&mut c, &fresh, Some((node(2), "Ann".into())), "sims4", 500);
        assert!(w.is_none());
    }

    #[test]
    fn client_adopts_newer_set_members_and_last_host() {
        let mut host = crew();
        publish_set(&mut host, pack("sims4", 2), "Host", 100);
        let mut mine = host.clone();
        mine.sets.clear();
        mine.members.clear();
        let (w, _) = host_answer(&mut host, &hello_for(&mine, "sims4"), Some((node(2), "Ann".into())), "sims4", 150);
        let w = validate_welcome(w.unwrap(), "sims4", known).unwrap();
        let last = CrewHost { node_id: node(1), name: "Host".into(), addresses: vec!["10.0.0.2".into()], port: 9847, game_id: "sims4".into(), at: 150 };
        assert!(client_apply(&mut mine, w, Some(last.clone())));
        assert_eq!(mine.sets["sims4"].version, 1);
        assert_eq!(mine.last_host, Some(last));
        assert!(mine.members.iter().any(|m| m.node_id == node(1)));
        assert!(mine.members.iter().any(|m| m.node_id == node(2)));

        // An older set never replaces a newer one.
        let mut newer = mine.sets["sims4"].clone();
        newer.version = 5;
        mine.sets.insert("sims4".into(), newer);
        let stale = CrewWelcome { set: Some(host.sets["sims4"].clone()), ..w_empty(&host) };
        client_apply(&mut mine, stale, None);
        assert_eq!(mine.sets["sims4"].version, 5);
    }

    fn w_empty(c: &Crew) -> CrewWelcome {
        CrewWelcome { id: c.id.clone(), name: c.name.clone(), name_updated_at: 0, games: vec![], members: vec![], set: None }
    }

    #[test]
    fn welcome_validation_rejects_bad_peer_data() {
        let c = crew();
        let ok = || w_empty(&c);
        assert!(validate_welcome(ok(), "sims4", known).is_ok());
        assert!(validate_welcome(CrewWelcome { id: "x".into(), ..ok() }, "sims4", known).is_err());
        let bad_member = CrewMember { node_id: "nope".into(), name: "x".into(), updated_at: 0, removed: false, last_seen: 0 };
        assert!(validate_welcome(CrewWelcome { members: vec![bad_member], ..ok() }, "sims4", known).is_err());
        // Unknown games are dropped rather than stored.
        let w = validate_welcome(CrewWelcome { games: vec!["sims4".into(), "nope".into()], ..ok() }, "sims4", known).unwrap();
        assert_eq!(w.games, vec!["sims4".to_string()]);
        // A set for another game than the session, or with a traversal path, is refused.
        let set = |p: ModPack| Some(CrewSet { pack: p, version: 1, published_at: 1, publisher: "H".into() });
        assert!(validate_welcome(CrewWelcome { set: set(pack("ets2", 1)), ..ok() }, "sims4", known).is_err());
        let mut evil = pack("sims4", 1);
        evil.files[0].relative_path = "../../Windows/evil.dll".into();
        assert!(validate_welcome(CrewWelcome { set: set(evil), ..ok() }, "sims4", known).is_err());
        assert!(validate_welcome(CrewWelcome { set: set(pack("sims4", 1)), ..ok() }, "sims4", known).is_ok());
    }

    #[test]
    fn publish_bumps_version_past_what_was_seen() {
        let mut c = crew();
        c.sets.insert("sims4".into(), CrewSet { pack: pack("sims4", 1), version: 7, published_at: 1, publisher: "X".into() });
        let s = publish_set(&mut c, pack("sims4", 2), "Me", 10);
        assert_eq!(s.version, 8);
        publish_set(&mut c, pack("ets2", 1), "Me", 10);
        assert!(c.games.contains(&"ets2".to_string()));
        assert!(set_is_newer(&c.sets["sims4"], None));
    }

    #[test]
    fn behind_uses_the_pack_compare() {
        let mut c = crew();
        let p = crate::testutil::test_pack("sims4", &[("Mods/a.package", b"A"), ("Mods/b.package", b"B"), ("Mods/c.package", b"C")]);
        publish_set(&mut c, p, "Host", 1);
        let mut local = crate::state::FileManifest::default();
        let file = |path: &str, content: &[u8]| crate::state::FileInfo {
            relative_path: path.into(),
            size: content.len() as u64,
            hash: crate::testutil::sha256_hex(content),
            modified: 0,
            file_type: "CustomContent".into(),
        };
        local.files.insert("Mods/a.package".into(), file("Mods/a.package", b"A"));
        local.files.insert("Mods/b.package".into(), file("Mods/b.package", b"CHANGED"));
        let cmp = crate::commands::modpack::compare_pack_to_local(&c.sets["sims4"].pack, &local, &[]);
        assert_eq!(behind(&cmp), 2, "one missing + one different");
    }
}
