use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level registry file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameRegistry {
    pub version: u32,
    pub games: Vec<GameDefinition>,
}

/// Complete definition of a supported game.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDefinition {
    pub id: String,
    pub label: String,
    pub family: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    /// Hex color for dynamic accent theming (e.g., "#1fb87e").
    #[serde(default)]
    pub primary_color: String,
    /// Steam app id, used to fetch store artwork (`commands::art`). `None` for
    /// games that aren't sold on Steam (WoW, Minecraft, ...).
    #[serde(default)]
    pub steam_app_id: Option<u32>,
    /// Steam app whose Workshop items are this game's mods, when they live in
    /// Steam's workshop folder rather than the game folder (tModLoader:
    /// 1281930). Only used to tell the user why those mods don't show up;
    /// SyncCrate doesn't sync Workshop content.
    #[serde(default)]
    pub steam_workshop_app_id: Option<u32>,
    /// The mod loader this game's mods need (BepInEx, SMAPI), so the
    /// compatibility check can flag mods that can't load (`crate::compat`).
    #[serde(default)]
    pub mod_loader: Option<ModLoader>,
    /// Official publisher-hosted art for non-Steam games, by kind (`cover`,
    /// `header`, `hero`); missing kinds fall back to the others.
    #[serde(default)]
    pub art_urls: HashMap<String, String>,
    /// Genre tags for the game browser filters; must come from `GENRES`.
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub auto_detect: bool,
    #[serde(default)]
    pub detection: Option<DetectionConfig>,
    #[serde(default)]
    pub validation: Option<ValidationConfig>,
    pub content_types: Vec<ContentType>,
    #[serde(default)]
    pub dangerous_script_extensions: Vec<String>,
    /// Key into the hardcoded pack registry (e.g., "sims4", "sims3", "sims2").
    #[serde(default)]
    pub packs: Option<String>,
    /// Old Game enum variant name for migration from pre-registry config.
    #[serde(default)]
    pub legacy_id: Option<String>,
    #[serde(default)]
    pub version_detection: Option<VersionDetection>,
    #[serde(default)]
    pub path_correction: Option<PathCorrection>,
    /// Executable names of the running game (e.g. `TS4_x64.exe`), used to warn
    /// before syncing while the game has files open. Case-insensitive.
    #[serde(default)]
    pub process_names: Vec<String>,
    /// How `toggle_mod` disables a file: `"folder"` (default) moves it into
    /// `<mods>/_Disabled/`; `"rename"` appends `.disabled` in place. Games that
    /// load content from nested subfolders (The Sims 3/4) need `"rename"`, or
    /// "disabled" mods keep loading. BepInEx games also use `"rename"` (it
    /// loads `*.dll` from every plugins subfolder). `"none"` hides the toggle
    /// where neither works (SMAPI / KSP load every subfolder and mods are
    /// folders, so renaming single files would half-break a mod).
    #[serde(default)]
    pub disable_method: Option<String>,
    /// Offer the duplicate finder (first content type only). Opt-in because
    /// identical files at different paths are normal for many games (WoW
    /// addons' shared `Libs/LibStub.lua`, BepInEx shared DLLs, Minecraft
    /// region files): "delete all extras" there broke working setups.
    #[serde(default)]
    pub duplicate_finder: bool,
    /// Files (relative to the game path) deleted after a sync that received
    /// files, e.g. Sims 4's `localthumbcache.package`, which must be cleared
    /// after mods change.
    #[serde(default)]
    pub post_sync_delete: Vec<String>,
    /// Extra names (besides `label`) matched against uninstall / Epic / GOG
    /// entries when deciding whether the game is actually installed, e.g.
    /// "World of Warcraft" for `wow_retail`. See `game_install`.
    #[serde(default)]
    pub install_names: Vec<String>,
    /// Install evidence checked by `game_install`: a path relative to the game
    /// folder (`Wow.exe`), an absolute path (`%ProgramFiles(x86)%/...`, `~`),
    /// or a registry dir `HKLM\Key::Value` whose directory must exist.
    #[serde(default)]
    pub install_markers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    #[serde(default)]
    pub strategies: Vec<DetectionStrategy>,
    /// If non-empty, a candidate path only matches when at least one of these
    /// relative paths exists inside it (e.g. `ReShade.ini` in the Sims 4 `Bin`
    /// folder), so add-ons like ReShade/GShade are only detected when installed.
    #[serde(default)]
    pub require_any: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DetectionStrategy {
    /// Check paths relative to the user's Documents directory.
    #[serde(rename = "documents_relative")]
    DocumentsRelative {
        base: String,
        folders: Vec<String>,
    },
    /// Check absolute paths, per-platform.
    #[serde(rename = "absolute_paths")]
    AbsolutePaths {
        #[serde(default)]
        paths: PlatformPaths,
    },
    /// Check folders relative to every Steam library's `steamapps/common`
    /// directory (parsed from `libraryfolders.vdf`, so games installed on
    /// secondary drives are found too).
    #[serde(rename = "steam_library")]
    SteamLibrary {
        folders: Vec<String>,
    },
    /// Read an install directory from the Windows registry (e.g. the EA App's
    /// `HKLM\SOFTWARE\Maxis\The Sims 4` → `Install Dir`) and append `subpath`.
    /// No-op on other platforms.
    #[serde(rename = "windows_registry")]
    WindowsRegistry {
        keys: Vec<String>,
        value: String,
        #[serde(default)]
        subpath: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlatformPaths {
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub macos: Vec<String>,
    #[serde(default)]
    pub linux: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// At least one of these directories must exist for the path to be valid.
    #[serde(default)]
    pub check_dirs: Vec<String>,
    /// Directories to create automatically on set_game_path.
    #[serde(default)]
    pub auto_create_dirs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentType {
    pub id: String,
    pub label: String,
    pub folder: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Default file_type string for files in this folder.
    pub file_type: String,
    /// Map specific extensions to different file_type values within this folder.
    #[serde(default)]
    pub classify_by_extension: HashMap<String, String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default = "default_true")]
    pub syncable: bool,
    /// Scan subfolders (default). `false` only looks at files directly in
    /// `folder`, which is the only case where `folder` may be `"."`.
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// Only include files whose first 64 KB contain this text (e.g. ReShade
    /// presets contain `Techniques=`), so loose files are matched safely.
    #[serde(default)]
    pub must_contain: Option<String>,
    /// File names (case-insensitive) never included, e.g. `ReShade.ini`.
    #[serde(default)]
    pub exclude_files: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionDetection {
    pub file: String,
    #[serde(default = "default_read_text")]
    pub method: String,
}

fn default_read_text() -> String {
    "read_text_file".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathCorrection {
    #[serde(default)]
    pub known_subfolders: Vec<String>,
    #[serde(default)]
    pub nested_corrections: HashMap<String, u32>,
}

/// Allowed `genres` values. The frontend has matching labels
/// (`GENRE_LABELS` in `src/lib/games.ts`); add new ones in both places.
// Only the registry test reads it; the frontend owns filtering.
#[cfg_attr(not(test), allow(dead_code))]
pub const GENRES: &[&str] = &[
    "action", "automation", "city-builder", "horror", "life-sim", "mmo", "party", "platformer",
    "racing", "rhythm", "roguelike", "rpg", "sandbox", "shooter", "simulation", "strategy", "survival",
];

/// Load the game registry from the embedded JSON.
pub fn load_registry() -> GameRegistry {
    let json = include_str!("game_registry.json");
    serde_json::from_str(json).expect("Invalid embedded game_registry.json")
}

/// Build a lookup map from game ID to definition.
pub fn build_registry_map(registry: &GameRegistry) -> HashMap<String, GameDefinition> {
    registry.games.iter().map(|g| (g.id.clone(), g.clone())).collect()
}

/// Build a lookup map from legacy ID to new game ID for migration.
pub fn build_legacy_map(registry: &GameRegistry) -> HashMap<String, String> {
    registry.games.iter()
        .filter_map(|g| g.legacy_id.as_ref().map(|lid| (lid.clone(), g.id.clone())))
        .collect()
}

/// A mod loader: when `content_type` has files but none of `any_of`
/// (relative to the game folder) exists, the mods won't load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModLoader {
    pub name: String,
    pub content_type: String,
    pub any_of: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_loaders_point_at_real_content_types_and_plain_paths() {
        let games = load_registry().games;
        assert!(games.iter().filter(|g| g.mod_loader.is_some()).count() >= 2);
        for g in &games {
            let Some(l) = &g.mod_loader else { continue };
            assert!(g.content_types.iter().any(|c| c.id == l.content_type), "{}: mod_loader content type {}", g.id, l.content_type);
            assert!(!l.any_of.is_empty(), "{}: mod_loader needs files", g.id);
            for p in &l.any_of {
                assert!(!p.contains("..") && !p.starts_with('/') && !p.contains(':'), "{}: {}", g.id, p);
            }
            assert!(l.url.as_deref().map_or(true, |u| u.starts_with("https://")), "{}", g.id);
        }
    }

    #[test]
    fn every_game_has_known_genres() {
        for g in load_registry().games {
            assert!(!g.genres.is_empty(), "{} has no genres", g.id);
            for genre in &g.genres {
                assert!(GENRES.contains(&genre.as_str()), "{}: unknown genre {genre}", g.id);
            }
        }
    }

    #[test]
    fn test_embedded_registry_deserializes() {
        // Panics via expect() if the embedded JSON is malformed or doesn't match
        // the structs — guards against a bad game_registry.json edit.
        let registry = load_registry();
        assert!(!registry.games.is_empty());
        // IDs must be unique.
        let mut ids: Vec<&str> = registry.games.iter().map(|g| g.id.as_str()).collect();
        let count = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count, "duplicate game ids in registry");
    }

    #[test]
    fn test_sims4_reshade_entry() {
        let registry = load_registry();
        let rs = registry
            .games
            .iter()
            .find(|g| g.id == "sims4-reshade")
            .expect("sims4-reshade entry missing");
        assert_eq!(rs.family, "sims");
        // Uses absolute-path detection (Bin folder lives in the game install dir).
        let detection = rs.detection.as_ref().expect("detection missing");
        assert!(detection
            .strategies
            .iter()
            .any(|s| matches!(s, DetectionStrategy::AbsolutePaths { .. })));
        // Presets + shaders content types, both syncable.
        let ct_ids: Vec<&str> = rs.content_types.iter().map(|c| c.id.as_str()).collect();
        assert!(ct_ids.contains(&"reshade_presets"));
        assert!(ct_ids.contains(&"reshade_shaders"));
        assert!(rs.content_types.iter().all(|c| c.syncable));
    }

    #[test]
    fn test_sims4_gshade_entry_and_install_markers() {
        let registry = load_registry();
        for (id, marker, cts) in [
            ("sims4-reshade", "ReShade.ini", ["reshade_presets", "reshade_shaders"]),
            ("sims4-gshade", "GShade.ini", ["gshade_presets", "gshade_shaders"]),
        ] {
            let g = registry.games.iter().find(|g| g.id == id).unwrap_or_else(|| panic!("{} missing", id));
            let det = g.detection.as_ref().expect("detection missing");
            // Only detected when the add-on is actually installed in Bin.
            assert!(det.require_any.iter().any(|r| r == marker), "{} lacks {} marker", id, marker);
            assert!(det.strategies.iter().any(|s| matches!(s, DetectionStrategy::SteamLibrary { .. })));
            assert!(det.strategies.iter().any(|s| matches!(s, DetectionStrategy::WindowsRegistry { .. })));
            let ids: Vec<&str> = g.content_types.iter().map(|c| c.id.as_str()).collect();
            assert_eq!(&ids[..2], &cts[..]);
            // A "." folder is only allowed for non-recursive (loose-file) types.
            assert!(g.content_types.iter().all(|c| !c.folder.is_empty() && (c.folder != "." || !c.recursive)));
            // The machine-specific config must never be synced.
            for c in g.content_types.iter().filter(|c| c.folder == ".") {
                assert!(c.exclude_files.iter().any(|f| f.eq_ignore_ascii_case(marker)));
                assert!(c.must_contain.is_some());
            }
        }
    }

    /// Normalized content folder ("./" and case stripped, "/" separators).
    fn norm_folder(f: &str) -> String {
        f.replace('\\', "/").trim_matches('/').to_lowercase()
    }

    /// True if `inner`'s files would also be picked up by scanning `outer`.
    fn folder_covers(outer: &ContentType, inner: &ContentType) -> bool {
        let (o, i) = (norm_folder(&outer.folder), norm_folder(&inner.folder));
        if o == i {
            return true;
        }
        // "." non-recursive only sees loose files in the game folder itself.
        let o_prefix = if o == "." { String::new() } else { format!("{}/", o) };
        outer.recursive && (o == "." || i.starts_with(&o_prefix))
    }

    #[test]
    fn content_type_folders_do_not_overlap() {
        // One file must belong to one content type: overlapping folders made KSP
        // list saves/ships twice and gave it two sets of permissions.
        for g in load_registry().games {
            for (a_i, a) in g.content_types.iter().enumerate() {
                for (b_i, b) in g.content_types.iter().enumerate() {
                    if a_i != b_i {
                        assert!(!folder_covers(a, b), "{}: {} covers {}", g.id, a.id, b.id);
                    }
                }
            }
        }
    }

    #[test]
    fn content_folders_are_relative_to_the_game_folder() {
        // Scans, backups and the watcher join `folder` onto the game path, so
        // 7 Days to Die's "%APPDATA%/7DaysToDie/Saves" never existed and its
        // saves silently never synced.
        for g in load_registry().games {
            for ct in &g.content_types {
                let f = &ct.folder;
                assert!(
                    !f.contains('%') && !f.contains(':') && !f.starts_with('/') && !f.starts_with('~') && !f.split(['/', '\\']).any(|s| s == ".."),
                    "{}: content folder {:?} must be relative to the game folder",
                    g.id,
                    f
                );
            }
        }
    }

    #[test]
    fn folder_cover_rules() {
        let ct = |folder: &str, recursive: bool| -> ContentType {
            serde_json::from_value(serde_json::json!({
                "id": folder, "label": "x", "folder": folder, "recursive": recursive, "file_type": "Mod"
            }))
            .unwrap()
        };
        assert!(folder_covers(&ct("saves", true), &ct("saves/ships", true)));
        assert!(folder_covers(&ct("Saves", true), &ct("saves", true)));
        assert!(!folder_covers(&ct("saves", false), &ct("saves/ships", true)));
        assert!(!folder_covers(&ct("save", true), &ct("saves", true)));
        assert!(!folder_covers(&ct(".", false), &ct("presets", true)));
    }

    #[test]
    fn toggle_and_duplicate_flags() {
        let registry = load_registry();
        let get = |id: &str| registry.games.iter().find(|g| g.id == id).unwrap_or_else(|| panic!("{id}"));
        for id in ["valheim", "lethal_company", "risk_of_rain_2", "among_us", "repo", "v_rising", "dyson_sphere_program"] {
            assert_eq!(get(id).disable_method.as_deref(), Some("rename"), "{id}");
        }
        // Every BepInEx game, including ones added later.
        for g in registry.games.iter().filter(|g| g.content_types.iter().any(|c| c.folder.starts_with("BepInEx/plugins"))) {
            assert_eq!(g.disable_method.as_deref(), Some("rename"), "{} uses BepInEx", g.id);
        }
        for id in ["stardew_valley", "kerbal_space_program"] {
            assert_eq!(get(id).disable_method.as_deref(), Some("none"), "{id}");
        }
        for g in &registry.games {
            assert!(
                matches!(g.disable_method.as_deref(), None | Some("folder") | Some("rename") | Some("none")),
                "{}: unknown disable_method", g.id
            );
        }
        for id in ["sims4", "sims3", "sims2"] {
            assert!(get(id).duplicate_finder, "{id}");
        }
        for id in ["wow_retail", "minecraft_java", "valheim", "skyrim_se"] {
            assert!(!get(id).duplicate_finder, "{id}");
        }
    }

    #[test]
    fn registry_includes_recent_game_additions() {
        let registry = load_registry();
        let ids: Vec<_> = registry.games.iter().map(|g| g.id.as_str()).collect();

        assert!(ids.contains(&"project_zomboid"));
        assert!(ids.contains(&"skyrim_se"));
        assert!(ids.contains(&"bannerlord"));
    }

    #[test]
    fn registry_includes_expanded_game_catalog() {
        let registry = load_registry();
        assert!(
            registry.games.len() >= 100,
            "expected at least 100 games, found {}",
            registry.games.len()
        );

        let ids: Vec<_> = registry.games.iter().map(|g| g.id.as_str()).collect();
        for id in [
            "baldurs_gate_3",
            "fallout4",
            "cyberpunk2077",
            "witcher3",
            "stellaris",
            "civilization_6",
            "lethal_company",
            "vintage_story",
            "gta5",
            "among_us",
            "balatro",
            "tabletop_simulator",
            "dragon_age_origins",
        ] {
            assert!(ids.contains(&id), "missing game id {id}");
        }

        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate game ids in registry");

        let sims4 = registry.games.iter().find(|g| g.id == "sims4").expect("sims4");
        assert_eq!(sims4.disable_method.as_deref(), Some("rename"));
        assert!(sims4.post_sync_delete.iter().any(|f| f == "localthumbcache.package"));
        let sims3 = registry.games.iter().find(|g| g.id == "sims3").expect("sims3");
        assert_eq!(sims3.disable_method.as_deref(), Some("rename"));

        let with_procs = registry
            .games
            .iter()
            .filter(|g| g.process_names.iter().any(|p| !p.trim().is_empty()))
            .count();
        assert!(with_procs >= 60, "only {with_procs} games have process_names");

        for game in &registry.games {
            assert!(!game.content_types.is_empty(), "{} has no content types", game.id);
            for ct in &game.content_types {
                assert!(!ct.folder.trim().is_empty(), "{}/{} has empty folder", game.id, ct.id);
                assert!(!ct.file_type.trim().is_empty(), "{}/{} has empty file_type", game.id, ct.id);
            }
            // Manual-path-only entries (e.g. private WoW servers) have no detection.
            if game.auto_detect {
                let strategies = game
                    .detection
                    .as_ref()
                    .map(|d| d.strategies.len())
                    .unwrap_or(0);
                assert!(strategies > 0, "{} auto-detects but has no detection strategy", game.id);
            }
        }
    }
}

/// Resolve a game ID, accepting both new IDs and legacy enum variant names.
pub fn resolve_game_id(
    id: &str,
    registry_map: &HashMap<String, GameDefinition>,
    legacy_map: &HashMap<String, String>,
) -> Option<String> {
    if registry_map.contains_key(id) {
        return Some(id.to_string());
    }
    legacy_map.get(id).cloned()
}
