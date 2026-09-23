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
    /// Hex color for dynamic accent theming (e.g., "#1ea84b").
    #[serde(default)]
    pub primary_color: String,
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
    /// "disabled" mods keep loading.
    #[serde(default)]
    pub disable_method: Option<String>,
    /// Files (relative to the game path) deleted after a sync that received
    /// files, e.g. Sims 4's `localthumbcache.package`, which must be cleared
    /// after mods change.
    #[serde(default)]
    pub post_sync_delete: Vec<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

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
