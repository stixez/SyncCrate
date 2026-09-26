use crate::state::{
    CompatibilityStatus, FileManifest, GameInfo, ModCompatibility, PackId, PackInfo,
    PackType,
};

/// Known pack registry entry.
struct KnownPack {
    code: &'static str,
    name: &'static str,
    pack_type: PackType,
}

macro_rules! ep {
    ($code:expr, $name:expr) => {
        KnownPack { code: $code, name: $name, pack_type: PackType::ExpansionPack }
    };
}
macro_rules! gp {
    ($code:expr, $name:expr) => {
        KnownPack { code: $code, name: $name, pack_type: PackType::GamePack }
    };
}
macro_rules! sp {
    ($code:expr, $name:expr) => {
        KnownPack { code: $code, name: $name, pack_type: PackType::StuffPack }
    };
}
macro_rules! kit {
    ($code:expr, $name:expr) => {
        KnownPack { code: $code, name: $name, pack_type: PackType::Kit }
    };
}

fn sims4_packs() -> Vec<KnownPack> {
    vec![
        // Expansion Packs
        ep!("EP01", "Get to Work"),
        ep!("EP02", "Get Together"),
        ep!("EP03", "City Living"),
        ep!("EP04", "Cats & Dogs"),
        ep!("EP05", "Seasons"),
        ep!("EP06", "Get Famous"),
        ep!("EP07", "Island Living"),
        ep!("EP08", "Discover University"),
        ep!("EP09", "Eco Lifestyle"),
        ep!("EP10", "Snowy Escape"),
        ep!("EP11", "Cottage Living"),
        ep!("EP12", "High School Years"),
        ep!("EP13", "Growing Together"),
        ep!("EP14", "Horse Ranch"),
        ep!("EP15", "Lovestruck"),
        ep!("EP16", "Life & Death"),
        // Game Packs
        gp!("GP01", "Outdoor Retreat"),
        gp!("GP02", "Spa Day"),
        gp!("GP03", "Dine Out"),
        gp!("GP04", "Vampires"),
        gp!("GP05", "Parenthood"),
        gp!("GP06", "Jungle Adventure"),
        gp!("GP07", "StrangerVille"),
        gp!("GP08", "Realm of Magic"),
        gp!("GP09", "Journey to Batuu"),
        gp!("GP10", "Dream Home Decorator"),
        gp!("GP11", "My Wedding Stories"),
        gp!("GP12", "Werewolves"),
        // Stuff Packs
        sp!("SP01", "Luxury Party"),
        sp!("SP02", "Perfect Patio"),
        sp!("SP03", "Cool Kitchen"),
        sp!("SP04", "Spooky"),
        sp!("SP05", "Movie Hangout"),
        sp!("SP06", "Romantic Garden"),
        sp!("SP07", "Kids Room"),
        sp!("SP08", "Backyard"),
        sp!("SP09", "Vintage Glamour"),
        sp!("SP10", "Bowling Night"),
        sp!("SP11", "Fitness"),
        sp!("SP12", "Toddler"),
        sp!("SP13", "Laundry Day"),
        sp!("SP14", "My First Pet"),
        sp!("SP15", "Moschino"),
        sp!("SP16", "Tiny Living"),
        sp!("SP17", "Nifty Knitting"),
        sp!("SP18", "Paranormal"),
        sp!("SP19", "Courtyard Oasis"),
        sp!("SP20", "Fashion Street"),
        sp!("SP21", "Decor to the Max"),
        sp!("SP22", "Modern Menswear"),
        sp!("SP23", "Blooming Rooms"),
        sp!("SP24", "Carnaval Streetwear"),
        // Kits
        kit!("KT01", "Throwback Fit"),
        kit!("KT02", "Country Kitchen"),
        kit!("KT03", "Bust the Dust"),
        kit!("KT04", "Courtyard Oasis Kit"),
        kit!("KT05", "Industrial Loft"),
        kit!("KT06", "Fashion Street Kit"),
        kit!("KT07", "Incheon Arrivals"),
        kit!("KT08", "Blooming Rooms Kit"),
        kit!("KT09", "Little Campers"),
        kit!("KT10", "Moonlight Chic"),
        kit!("KT11", "Desert Luxe"),
        kit!("KT12", "Decor to the Max Kit"),
        kit!("KT13", "Carnaval Streetwear Kit"),
        kit!("KT14", "Pastel Pop"),
        kit!("KT15", "Everyday Clutter"),
        kit!("KT16", "Simtimates Collection"),
        kit!("KT17", "Bathroom Clutter"),
        kit!("KT18", "Greenhouse Haven"),
        kit!("KT19", "Basement Treasures"),
        kit!("KT20", "Grunge Revival"),
        kit!("KT21", "Book Nook"),
        kit!("KT22", "Poolside Splash"),
        kit!("KT23", "Cozy Bistro"),
        kit!("KT24", "Urban Homage"),
        kit!("KT25", "Modern Luxe"),
        kit!("KT26", "Castle Estate"),
        kit!("KT27", "Goth Galore"),
        kit!("KT28", "Glimmerbrook Gameday"),
        kit!("KT29", "Riviera Retreat"),
        kit!("KT30", "Sweet Slumber Party"),
    ]
}

fn sims3_packs() -> Vec<KnownPack> {
    vec![
        ep!("EP01", "World Adventures"),
        ep!("EP02", "Ambitions"),
        ep!("EP03", "Late Night"),
        ep!("EP04", "Generations"),
        ep!("EP05", "Pets"),
        ep!("EP06", "Showtime"),
        ep!("EP07", "Supernatural"),
        ep!("EP08", "Seasons"),
        ep!("EP09", "University Life"),
        ep!("EP10", "Island Paradise"),
        ep!("EP11", "Into the Future"),
        sp!("SP01", "High-End Loft"),
        sp!("SP02", "Fast Lane"),
        sp!("SP03", "Outdoor Living"),
        sp!("SP04", "Town Life"),
        sp!("SP05", "Master Suite"),
        sp!("SP06", "Katy Perry's Sweet Treats"),
        sp!("SP07", "Diesel"),
        sp!("SP08", "70s 80s & 90s"),
        sp!("SP09", "Movie"),
    ]
}

fn sims2_packs() -> Vec<KnownPack> {
    vec![
        ep!("EP01", "University"),
        ep!("EP02", "Nightlife"),
        ep!("EP03", "Open for Business"),
        ep!("EP04", "Pets"),
        ep!("EP05", "Seasons"),
        ep!("EP06", "Bon Voyage"),
        ep!("EP07", "FreeTime"),
        ep!("EP08", "Apartment Life"),
        sp!("SP01", "Family Fun"),
        sp!("SP02", "Glamour Life"),
        sp!("SP03", "Happy Holiday"),
        sp!("SP04", "Celebration!"),
        sp!("SP05", "H&M Fashion"),
        sp!("SP06", "Teen Style"),
        sp!("SP07", "Kitchen & Bath Interior Design"),
        sp!("SP08", "IKEA Home"),
        sp!("SP09", "Mansion & Garden"),
    ]
}

/// Get the pack registry for a game using its `packs` key from the game registry.
/// The `packs_key` comes from `GameDefinition.packs` (e.g., "sims4", "sims3", "sims2").
pub fn get_pack_registry(packs_key: &str) -> Vec<PackInfo> {
    let known = match packs_key {
        "sims4" => sims4_packs(),
        "sims3" => sims3_packs(),
        "sims2" => sims2_packs(),
        _ => return Vec::new(),
    };
    known
        .into_iter()
        .map(|k| PackInfo {
            id: PackId {
                code: k.code.to_string(),
                pack_type: k.pack_type,
            },
            name: k.name.to_string(),
        })
        .collect()
}

/// Maximum length for a game version string (e.g. "1.108.329.1030").
const MAX_VERSION_LEN: usize = 64;

/// Detect game version using the version_detection config from the registry.
pub fn detect_game_version(game_id: &str, game_path: &str) -> Option<String> {
    let registry = crate::registry::load_registry();
    let game_def = registry.games.iter().find(|g| g.id == game_id)?;
    let vd = game_def.version_detection.as_ref()?;

    let version_file = std::path::Path::new(game_path).join(&vd.file);
    std::fs::read_to_string(&version_file)
        .ok()
        .map(|s| {
            s.trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(MAX_VERSION_LEN)
                .collect::<String>()
        })
        .filter(|s| !s.is_empty())
}

/// The Sims 4 install folder, where the pack folders (EP01, GP01, ...) are:
/// the folder SyncCrate syncs is the one in Documents, which has none, so no
/// pack was ever detected. Found the way the ReShade entry finds Game\Bin.
pub(crate) fn sims4_install_dir() -> Option<std::path::PathBuf> {
    let reg = crate::registry::load_registry();
    let mut def = reg.games.iter().find(|g| g.id == "sims4-reshade")?.clone();
    // The ReShade entry only accepts a Game\Bin that has ReShade in it; the
    // install folder itself is wanted here, ReShade or not (without this,
    // packs and Play worked only for ReShade users).
    if let Some(d) = def.detection.as_mut() {
        d.require_any.clear();
    }
    let bin = crate::utils::detect_game_path_from_def(&def)?;
    std::path::Path::new(&bin).parent()?.parent().map(std::path::Path::to_path_buf)
}

/// Detect installed packs using a fallback chain of heuristics.
pub fn detect_installed_packs(packs_key: &str, game_path: &str) -> Vec<PackInfo> {
    let registry = get_pack_registry(packs_key);
    if registry.is_empty() {
        return Vec::new();
    }
    let base = std::path::Path::new(game_path);

    // Strategy 1: Parse installedpacks.txt (Sims 4)
    if packs_key == "sims4" {
        let installed_file = base.join("installedpacks.txt");
        if let Ok(content) = std::fs::read_to_string(&installed_file) {
            let content_lower = content.to_lowercase();
            let found: Vec<PackInfo> = registry
                .iter()
                .filter(|p| {
                    content.contains(&p.id.code)
                        || content_lower.contains(&p.name.to_lowercase())
                })
                .cloned()
                .collect();
            if !found.is_empty() {
                return found;
            }
        }
    }

    // Strategy 2: Scan Options.ini for pack references
    let options_file = base.join("Options.ini");
    if let Ok(content) = std::fs::read_to_string(&options_file) {
        let found: Vec<PackInfo> = registry
            .iter()
            .filter(|p| content.contains(&p.id.code))
            .cloned()
            .collect();
        if !found.is_empty() {
            return found;
        }
    }

    // Strategy 3: Check for pack subdirectories (EP01/, GP01/, etc.)
    let mut roots = vec![base.to_path_buf()];
    if packs_key == "sims4" {
        roots.extend(sims4_install_dir());
    }
    let found: Vec<PackInfo> = registry
        .iter()
        .filter(|p| roots.iter().any(|r| r.join(&p.id.code).is_dir()))
        .cloned()
        .collect();
    if !found.is_empty() {
        return found;
    }

    Vec::new()
}

/// Full detection: version + packs.
pub fn detect_game_info(game_id: &str, game_path: &str) -> GameInfo {
    let game_version = detect_game_version(game_id, game_path);

    let registry = crate::registry::load_registry();
    let packs_key = registry
        .games
        .iter()
        .find(|g| g.id == game_id)
        .and_then(|g| g.packs.as_deref())
        .unwrap_or("");

    let installed_packs = detect_installed_packs(packs_key, game_path);
    GameInfo {
        game_version,
        installed_packs,
    }
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// Guess which packs a mod might require based on filename heuristics.
/// Whole words only: substring matches flagged "deep01" as EP01 and any
/// "Toddler" folder as Toddler Stuff. Codes count anywhere in the path;
/// names only in the file name, and only multi-word ones ("Get to Work",
/// also run together as "GetToWork"): "Seasons" or "Pets" are everyday words
/// in mod names.
pub fn guess_mod_required_packs(mod_path: &str, registry: &[PackInfo]) -> Vec<PackId> {
    let path_words = words(mod_path);
    let file = mod_path.rsplit(['/', '\\']).next().unwrap_or(mod_path);
    let file_words = words(file);
    registry
        .iter()
        .filter(|pack| {
            let code = pack.id.code.to_lowercase();
            if path_words.iter().any(|w| *w == code) {
                return true;
            }
            let name = words(&pack.name);
            if name.len() < 2 {
                return false;
            }
            let joined = name.concat();
            file_words.iter().any(|w| *w == joined) || file_words.windows(name.len()).any(|w| w == name.as_slice())
        })
        .map(|p| p.id.clone())
        .collect()
}

/// Check mod compatibility against installed packs.
pub fn check_mod_compatibility(
    manifest: &FileManifest,
    game_info: &GameInfo,
    packs_key: &str,
) -> Vec<ModCompatibility> {
    // Nothing detected means we don't know what's installed (the Sims 4
    // Documents folder often has no pack list), not that nothing is: every
    // guessed mod was flagged "missing packs" on a PC that owns them all.
    if game_info.installed_packs.is_empty() {
        return Vec::new();
    }
    let installed_codes: std::collections::HashSet<&str> = game_info
        .installed_packs
        .iter()
        .map(|p| p.id.code.as_str())
        .collect();
    let registry = get_pack_registry(packs_key);

    manifest
        .files
        .iter()
        .filter(|(_, info)| {
            info.file_type == "Mod" || info.file_type == "CustomContent"
        })
        .filter_map(|(path, _)| {
            let required = guess_mod_required_packs(path, &registry);
            if required.is_empty() {
                return None;
            }
            let missing: Vec<PackId> = required
                .iter()
                .filter(|p| !installed_codes.contains(p.code.as_str()))
                .cloned()
                .collect();
            let status = if missing.is_empty() {
                CompatibilityStatus::Compatible
            } else {
                CompatibilityStatus::MissingPacks
            };
            Some(ModCompatibility {
                mod_path: path.clone(),
                required_packs: required,
                missing_packs: missing,
                status,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guessed(path: &str) -> Vec<String> {
        guess_mod_required_packs(path, &get_pack_registry("sims4")).into_iter().map(|p| p.code).collect()
    }

    #[test]
    fn pack_guesses_match_whole_words_only() {
        assert_eq!(guessed("Mods/EP01_career.package"), ["EP01"]);
        assert_eq!(guessed("Mods/GetToWork_Retail.package"), ["EP01"]);
        assert_eq!(guessed("Mods/get to work - retail.package"), ["EP01"]);
        assert!(guessed("Mods/deep01_sofa.package").is_empty(), "code inside another word");
        assert!(guessed("Mods/Toddler CC/hair.package").is_empty(), "single-word name in a folder");
        assert!(guessed("Mods/toddler_hair.package").is_empty(), "single-word name in the file");
    }

    #[test]
    fn no_detected_packs_means_no_missing_pack_warnings() {
        let mut manifest = FileManifest::default();
        manifest.files.insert(
            "Mods/EP01_career.package".into(),
            crate::state::FileInfo { relative_path: "Mods/EP01_career.package".into(), size: 1, hash: String::new(), modified: 0, file_type: "Mod".into() },
        );
        assert!(check_mod_compatibility(&manifest, &GameInfo::default(), "sims4").is_empty());
        let ep02 = get_pack_registry("sims4").into_iter().find(|p| p.id.code == "EP02").unwrap();
        let info = GameInfo { game_version: None, installed_packs: vec![ep02] };
        let r = check_mod_compatibility(&manifest, &info, "sims4");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].missing_packs[0].code, "EP01");
    }
}
