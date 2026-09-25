//! Tauri commands for crews (`crate::crews` has the model, merge rules and
//! trust notes). Nothing here syncs on its own: "catch up" is the frontend
//! running the ordinary pack flows (`compute_pack_sync_plan`,
//! `apply_pack_exact`) with the crew set, after the user clicks.
use crate::commands::modpack::PackComparison;
use crate::crews::{self, Crew, CrewInvite, CrewMember, CrewSet};
use crate::state::{AppState, PeerInfo, SessionInfo};
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::Mutex;

fn my_node(state: &AppState) -> Result<String, String> {
    state.local_node_id.clone().ok_or_else(|| "This install has no network id yet.".to_string())
}

fn me(state: &AppState, my_name: &str, now: u64) -> Result<CrewMember, String> {
    Ok(CrewMember {
        node_id: my_node(state)?,
        name: crews::clean_name(my_name).unwrap_or_else(|| "Me".into()),
        updated_at: now,
        removed: false,
        last_seen: now,
    })
}

fn crew_mut<'a>(state: &'a mut AppState, id: &str) -> Result<&'a mut Crew, String> {
    state.crews.get_mut(id).ok_or_else(|| "That crew no longer exists.".to_string())
}

fn is_known_game(state: &AppState) -> impl Fn(&str) -> bool + '_ {
    move |g| state.game_registry.games.iter().any(|d| d.id == g)
}

#[tauri::command]
pub async fn list_crews(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<Crew>, String> {
    Ok(state.lock().await.crews.crews.clone())
}

#[tauri::command]
pub async fn get_local_node_id(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<String, String> {
    my_node(&*state.lock().await)
}

#[tauri::command]
pub async fn create_crew(state: tauri::State<'_, Arc<Mutex<AppState>>>, name: String, my_name: String) -> Result<Crew, String> {
    create_crew_inner(state.inner(), &name, &my_name).await
}

pub(crate) async fn create_crew_inner(state: &Arc<Mutex<AppState>>, name: &str, my_name: &str) -> Result<Crew, String> {
    let name = crews::clean_name(name).ok_or("Give the crew a name.")?;
    let mut s = state.lock().await;
    if s.crews.crews.len() >= crews::MAX_CREWS {
        return Err(format!("You can be in at most {} crews.", crews::MAX_CREWS));
    }
    let now = crate::utils::timestamp_now();
    let crew = Crew {
        id: crews::new_crew_id(),
        name,
        name_updated_at: now,
        games: vec![s.active_game.clone()],
        members: vec![me(&s, my_name, now)?],
        sets: Default::default(),
        last_host: None,
        created_at: now,
    };
    s.crews.crews.push(crew.clone());
    crews::persist(&s);
    Ok(crew)
}

#[tauri::command]
pub async fn rename_crew(state: tauri::State<'_, Arc<Mutex<AppState>>>, id: String, name: String) -> Result<Crew, String> {
    let name = crews::clean_name(&name).ok_or("Give the crew a name.")?;
    let mut s = state.lock().await;
    let crew = crew_mut(&mut s, &id)?;
    crew.name = name;
    crew.name_updated_at = crate::utils::timestamp_now().max(crew.name_updated_at.saturating_add(1));
    let crew = crew.clone();
    crews::persist(&s);
    Ok(crew)
}

/// Leaving is local: it forgets the crew on this PC. Other members keep
/// their copies (there's no server to tell).
#[tauri::command]
pub async fn leave_crew(state: tauri::State<'_, Arc<Mutex<AppState>>>, id: String) -> Result<(), String> {
    let mut s = state.lock().await;
    s.crews.crews.retain(|c| c.id != id);
    crews::persist(&s);
    Ok(())
}

#[tauri::command]
pub async fn crew_invite_link(state: tauri::State<'_, Arc<Mutex<AppState>>>, id: String, my_name: String) -> Result<String, String> {
    let s = state.lock().await;
    let crew = s.crews.get(&id).ok_or("That crew no longer exists.")?;
    crews::encode_invite(crew, &my_node(&s)?, &my_name)
}

/// Parse pasted text (a full link, even with chat punctuation around it).
#[tauri::command]
pub async fn preview_crew_invite(state: tauri::State<'_, Arc<Mutex<AppState>>>, text: String) -> Result<CrewInvite, String> {
    let s = state.lock().await;
    let known = is_known_game(&s);
    match crate::commands::open_intent::classify(&text, &known) {
        Some(crate::commands::open_intent::OpenTarget::Crew(invite)) => Ok(invite),
        Some(crate::commands::open_intent::OpenTarget::Invalid(reason)) => Err(reason),
        _ => Err("That isn't a crew invite link (they start with synccrate://crew/).".to_string()),
    }
}

#[tauri::command]
pub async fn join_crew(state: tauri::State<'_, Arc<Mutex<AppState>>>, invite: CrewInvite, my_name: String) -> Result<Crew, String> {
    join_crew_inner(state.inner(), invite, &my_name).await
}

/// Adds the crew (or, if we're already in it, just learns the inviter).
/// Joining never connects or syncs.
pub(crate) async fn join_crew_inner(state: &Arc<Mutex<AppState>>, invite: CrewInvite, my_name: &str) -> Result<Crew, String> {
    let mut s = state.lock().await;
    let invite = crews::validate_invite(invite, is_known_game(&s))?;
    let now = crate::utils::timestamp_now();
    let me = me(&s, my_name, now)?;
    let crew = if let Some(existing) = s.crews.get(&invite.id) {
        // Already in it: an invite doesn't change who's a member. Anyone who
        // ever saw the crew id could otherwise forge an "invite" that adds
        // their own node id under a friend's name.
        existing.clone()
    } else {
        if s.crews.crews.len() >= crews::MAX_CREWS {
            return Err(format!("You can be in at most {} crews.", crews::MAX_CREWS));
        }
        let crew = crews::crew_from_invite(&invite, me, now);
        s.crews.crews.push(crew.clone());
        crew
    };
    crews::persist(&s);
    Ok(crew)
}

#[tauri::command]
pub async fn set_crew_member_removed(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    crew_id: String,
    node_id: String,
    removed: bool,
) -> Result<Crew, String> {
    let mut s = state.lock().await;
    if removed && s.local_node_id.as_deref() == Some(node_id.as_str()) {
        return Err("To remove yourself, leave the crew instead.".to_string());
    }
    let crew = crew_mut(&mut s, &crew_id)?;
    crews::set_member_removed(crew, &node_id, removed, crate::utils::timestamp_now())?;
    let crew = crew.clone();
    crews::persist(&s);
    Ok(crew)
}

/// Snapshot the active game's folder as the crew set (a normal `ModPack`,
/// restricted to `content_types` when given). Spreads to members the next
/// time they connect to a session this PC hosts.
#[tauri::command]
pub async fn publish_crew_set(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    crew_id: String,
    content_types: Option<Vec<String>>,
    my_name: String,
) -> Result<CrewSet, String> {
    publish_crew_set_inner(state.inner(), &crew_id, content_types, &my_name).await
}

pub(crate) async fn publish_crew_set_inner(
    state: &Arc<Mutex<AppState>>,
    crew_id: &str,
    content_types: Option<Vec<String>>,
    my_name: &str,
) -> Result<CrewSet, String> {
    let crew_name = {
        let s = state.lock().await;
        s.crews.get(crew_id).ok_or("That crew no longer exists.")?.name.clone()
    };
    let pack = crate::commands::modpack::create_pack_inner(state, None, content_types, None, crew_name, String::new(), false).await?;
    if pack.files.len() > crews::MAX_SET_FILES {
        return Err(format!("A crew set can list at most {} files.", crews::MAX_SET_FILES));
    }
    let mut s = state.lock().await;
    let publisher = crews::clean_name(my_name).unwrap_or_default();
    let crew = crew_mut(&mut s, crew_id)?;
    let set = crews::publish_set(crew, pack, &publisher, crate::utils::timestamp_now()).clone();
    crews::persist(&s);
    Ok(set)
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CrewStatus {
    pub game_id: String,
    /// The crew has a set for the active game.
    pub has_set: bool,
    pub behind: usize,
    pub comparison: Option<PackComparison>,
}

/// "You're N files behind the crew" for the active game, via the ordinary
/// pack compare (a hashed scan of the local folder; no host needed).
#[tauri::command]
pub async fn crew_status(state: tauri::State<'_, Arc<Mutex<AppState>>>, crew_id: String) -> Result<CrewStatus, String> {
    crew_status_inner(state.inner(), &crew_id).await
}

pub(crate) async fn crew_status_inner(state: &Arc<Mutex<AppState>>, crew_id: &str) -> Result<CrewStatus, String> {
    let (game, set) = {
        let s = state.lock().await;
        let crew = s.crews.get(crew_id).ok_or("That crew no longer exists.")?;
        (s.active_game.clone(), crew.sets.get(&s.active_game).cloned())
    };
    let Some(set) = set else {
        return Ok(CrewStatus { game_id: game, ..Default::default() });
    };
    let cmp = crate::commands::modpack::compare_pack_inner(state, set.pack).await?;
    Ok(CrewStatus { game_id: game, has_set: true, behind: crews::behind(&cmp), comparison: Some(cmp) })
}

#[derive(Debug, Clone, Serialize)]
pub struct CrewLanHost {
    pub crew_id: String,
    pub node_id: String,
    pub peer: PeerInfo,
}

/// A LAN discovery scan, filtered to hosts that are members of one of our
/// crews ("hosting now on your network"). Presence over the internet isn't
/// detected: that would mean dialling every member.
#[tauri::command]
pub async fn scan_crew_hosts(state: tauri::State<'_, Arc<Mutex<AppState>>>) -> Result<Vec<CrewLanHost>, String> {
    let peers = crate::network::discovery::scan_for_hosts().await?;
    let mut s = state.lock().await;
    let found = crew_lan_hosts(&s.crews.crews, s.local_node_id.as_deref(), &peers);
    // Kept so `connect_crew` can use fresh LAN addresses.
    s.discovered_peers = peers;
    Ok(found)
}

pub(crate) fn crew_lan_hosts(crews: &[Crew], me: Option<&str>, peers: &[PeerInfo]) -> Vec<CrewLanHost> {
    let mut out = Vec::new();
    for crew in crews {
        for p in peers {
            let Some(node) = p.node_id.as_deref() else { continue };
            if Some(node) == me {
                continue;
            }
            if crew.members.iter().any(|m| m.node_id == node && !m.removed) {
                out.push(CrewLanHost { crew_id: crew.id.clone(), node_id: node.to_string(), peer: p.clone() });
            }
        }
    }
    out
}

/// Where to dial a crew member: fresh LAN addresses from the last scan win,
/// then the addresses the last session with them used; the node id is
/// always dialled over the internet too (`connect_best` races both).
pub(crate) fn connect_target(
    crew: &Crew,
    node_id: &str,
    discovered: &[PeerInfo],
) -> Result<(Vec<String>, u16, iroh::EndpointId, String), String> {
    let member = crew.members.iter().find(|m| m.node_id == node_id).ok_or("That person isn't in this crew.")?;
    if member.removed {
        return Err(format!("{} was removed from this crew.", member.name));
    }
    let id = crews::parse_node_id(node_id).ok_or("That member has an invalid id.")?;
    if let Some(p) = discovered.iter().find(|p| p.node_id.as_deref() == Some(node_id) && p.port >= 1024) {
        let addrs = if p.addresses.is_empty() { vec![p.ip.clone()] } else { p.addresses.clone() };
        return Ok((addrs, p.port, id, member.name.clone()));
    }
    if let Some(h) = crew.last_host.as_ref().filter(|h| h.node_id == node_id && h.port >= 1024) {
        let addrs: Vec<String> = h.addresses.iter().filter(|a| a.parse::<std::net::IpAddr>().is_ok()).cloned().collect();
        return Ok((addrs, h.port, id, member.name.clone()));
    }
    // Internet only; the port is unused without addresses.
    Ok((Vec::new(), 9847, id, member.name.clone()))
}

/// One-click reconnect: join a crew member's session without a code. The
/// usual handshake runs, so the PIN (`pin`) and wrong-game checks apply
/// exactly as with a join code.
#[tauri::command]
pub async fn connect_crew(
    state: tauri::State<'_, Arc<Mutex<AppState>>>,
    app: tauri::AppHandle,
    crew_id: String,
    node_id: Option<String>,
    name: String,
    pin: Option<String>,
) -> Result<SessionInfo, String> {
    let (addresses, port, id, label) = {
        let s = state.lock().await;
        let crew = s.crews.get(&crew_id).ok_or("That crew no longer exists.")?;
        let node = node_id
            .or_else(|| crew.last_host.as_ref().map(|h| h.node_id.clone()))
            .ok_or("Nobody has hosted this crew for you yet. Join once with a code, or ask a member to host.")?;
        if s.local_node_id.as_deref() == Some(node.as_str()) {
            return Err("That's you. Host a session and the crew can join you.".to_string());
        }
        connect_target(crew, &node, &s.discovered_peers)?
    };
    let pin = pin.filter(|p| !p.trim().is_empty());
    // Internet (iroh) only: it proves the host really has this node id. A LAN
    // address comes from an unauthenticated broadcast anyone can fake, so it
    // would let a stranger pose as a crew member. On a LAN without internet,
    // join with the host's code instead.
    let _ = addresses;
    crate::commands::session::start_direct_connection(state.inner(), app, Vec::new(), port, Some(id), true, name, pin, label).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(n: u8) -> String {
        crews::node_id_hex(&iroh::SecretKey::from_bytes(&[n; 32]).public())
    }

    fn crew() -> Crew {
        let m = |n: u8, name: &str| CrewMember { node_id: node(n), name: name.into(), updated_at: 1, removed: false, last_seen: 0 };
        Crew {
            id: crews::new_crew_id(),
            name: "C".into(),
            name_updated_at: 0,
            games: vec!["sims4".into()],
            members: vec![m(1, "Host"), m(2, "Ann"), CrewMember { removed: true, ..m(3, "Gone") }],
            sets: Default::default(),
            last_host: Some(crews::CrewHost {
                node_id: node(1),
                name: "Host".into(),
                addresses: vec!["192.168.1.5".into(), "not-an-ip".into()],
                port: 9847,
                game_id: "sims4".into(),
                at: 1,
            }),
            created_at: 0,
        }
    }

    fn peer(node_id: Option<String>, ip: &str, port: u16) -> PeerInfo {
        PeerInfo {
            id: "p".into(),
            name: "H".into(),
            ip: ip.into(),
            port,
            mod_count: 0,
            version: "0.6.0".into(),
            pin_required: false,
            game_info: None,
            game_id: Some("sims4".into()),
            addresses: vec![ip.into()],
            node_id,
        }
    }

    #[test]
    fn connect_target_prefers_fresh_lan_then_last_host_then_internet() {
        let c = crew();
        let lan = [peer(Some(node(1)), "10.0.0.9", 9850)];
        let (addrs, port, id, _) = connect_target(&c, &node(1), &lan).unwrap();
        assert_eq!((addrs, port), (vec!["10.0.0.9".to_string()], 9850));
        assert_eq!(crews::node_id_hex(&id), node(1));

        let (addrs, port, _, _) = connect_target(&c, &node(1), &[]).unwrap();
        assert_eq!((addrs, port), (vec!["192.168.1.5".to_string()], 9847), "junk addresses are dropped");

        let (addrs, _, id, name) = connect_target(&c, &node(2), &[]).unwrap();
        assert!(addrs.is_empty(), "no known address: internet only");
        assert_eq!(crews::node_id_hex(&id), node(2));
        assert_eq!(name, "Ann");

        assert!(connect_target(&c, &node(3), &[]).unwrap_err().contains("removed"));
        assert!(connect_target(&c, &node(9), &[]).is_err());
    }

    #[test]
    fn lan_hosts_are_members_only_and_never_me() {
        let c = crew();
        let peers = [
            peer(Some(node(1)), "10.0.0.1", 9847),
            peer(Some(node(3)), "10.0.0.3", 9847), // removed
            peer(Some(node(7)), "10.0.0.7", 9847), // stranger
            peer(None, "10.0.0.8", 9847),          // pre-0.6 host
            peer(Some(node(2)), "10.0.0.2", 9847), // me
        ];
        let found = crew_lan_hosts(&[c], Some(&node(2)), &peers);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].node_id, node(1));
    }
}
