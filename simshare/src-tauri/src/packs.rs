use crate::state::{
    CompatibilityStatus, FileManifest, GameInfo, ModCompatibility, PackId, PackInfo, PackType,
    SimsGame,
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

pub fn get_pack_registry(game: &SimsGame) -> Vec<PackInfo> {
    let known = match game {
        SimsGame::Sims4 => sims4_packs(),
        SimsGame::Sims3 => sims3_packs(),
        SimsGame::Sims2 => sims2_packs(),
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

/// Detect game version from the user data directory.
pub fn detect_game_version(game: &SimsGame, game_path: &str) -> Option<String> {
    let filename = match game {
        SimsGame::Sims4 => "GameVersion.txt",
        SimsGame::Sims3 => "Sims3Version.txt",
        SimsGame::Sims2 => return None,
    };
    let version_file = std::path::Path::new(game_path).join(filename);
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

/// Detect installed packs using a fallback chain of heuristics.
pub fn detect_installed_packs(game: &SimsGame, game_path: &str) -> Vec<PackInfo> {
    let registry = get_pack_registry(game);
    let base = std::path::Path::new(game_path);

    // Strategy 1: Parse installedpacks.txt (Sims 4)
    if *game == SimsGame::Sims4 {
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
    let found: Vec<PackInfo> = registry
        .iter()
        .filter(|p| base.join(&p.id.code).exists())
        .cloned()
        .collect();
    if !found.is_empty() {
        return found;
    }

    Vec::new()
}

/// Full detection: version + packs.
pub fn detect_game_info(game: &SimsGame, game_path: &str) -> GameInfo {
    let game_version = detect_game_version(game, game_path);
    let installed_packs = detect_installed_packs(game, game_path);
    GameInfo {
        game_version,
        installed_packs,
    }
}

/// Guess which packs a mod might require based on filename heuristics.
pub fn guess_mod_required_packs(mod_path: &str, game: &SimsGame) -> Vec<PackId> {
    let registry = get_pack_registry(game);
    let filename_lower = mod_path.to_lowercase();
    let mut required = Vec::new();

    for pack in &registry {
        // Check for pack code in filename (e.g., EP01, GP05)
        if filename_lower.contains(&pack.id.code.to_lowercase()) {
            required.push(pack.id.clone());
            continue;
        }
        // Check for pack name keywords (only if name > 3 chars to avoid false positives)
        let name_lower = pack.name.to_lowercase();
        if name_lower.len() > 3 && filename_lower.contains(&name_lower) {
            required.push(pack.id.clone());
        }
    }

    required
}

/// Check mod compatibility against installed packs.
pub fn check_mod_compatibility(
    manifest: &FileManifest,
    game_info: &GameInfo,
    game: &SimsGame,
) -> Vec<ModCompatibility> {
    let installed_codes: std::collections::HashSet<&str> = game_info
        .installed_packs
        .iter()
        .map(|p| p.id.code.as_str())
        .collect();

    manifest
        .files
        .iter()
        .filter(|(_, info)| {
            matches!(
                info.file_type,
                crate::state::FileType::Mod | crate::state::FileType::CustomContent
            )
        })
        .filter_map(|(path, _)| {
            let required = guess_mod_required_packs(path, game);
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
