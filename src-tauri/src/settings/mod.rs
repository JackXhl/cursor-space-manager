//! User preferences and build identity.
//!
//! Preferences are user-editable and versioned. Build identity (version, build
//! number, target) comes from compile-time metadata so a settings file can
//! never be used to misrepresent which build is running.

use crate::error::{AppError, AppResult};
use crate::util::now_millis;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bumped whenever the shape of `Settings` changes incompatibly.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SizeUnit {
    #[default]
    Auto,
    Megabytes,
    Gigabytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub theme: ThemeMode,
    /// BCP 47 tag. Only `zh-CN` ships today; the field exists so stored
    /// preferences survive the addition of more locales.
    pub language: String,
    pub scan_on_start: bool,
    pub size_unit: SizeUnit,
    pub default_target_root: Option<PathBuf>,
    /// Root ids the user never wants scanned.
    pub excluded_root_ids: Vec<String>,
    pub check_updates_automatically: bool,
    pub update_channel: UpdateChannel,
    /// Keep renamed-aside originals after a successful migration. Turning this
    /// off is offered but never the default.
    pub retain_backups: bool,
    pub reduce_motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme: ThemeMode::System,
            language: "zh-CN".to_string(),
            scan_on_start: true,
            size_unit: SizeUnit::Auto,
            default_target_root: None,
            excluded_root_ids: Vec::new(),
            check_updates_automatically: true,
            update_channel: UpdateChannel::Stable,
            retain_backups: true,
            reduce_motion: false,
        }
    }
}

/// Read-only facts about the running binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub name: String,
    pub version: String,
    pub build_profile: String,
    pub target_os: String,
    pub target_arch: String,
    pub license: String,
    pub schema_version: u32,
}

impl BuildInfo {
    pub fn current() -> Self {
        Self {
            name: "Cursor 空间迁移工具".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            }
            .to_string(),
            target_os: std::env::consts::OS.to_string(),
            target_arch: std::env::consts::ARCH.to_string(),
            license: "MIT".to_string(),
            schema_version: SETTINGS_SCHEMA_VERSION,
        }
    }
}

/// Reported alongside settings when a stored file could not be used, so the UI
/// can tell the user their preferences were reset instead of silently losing them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsLoad {
    pub settings: Settings,
    pub recovered: bool,
    pub message: Option<String>,
}

pub struct SettingsStore {
    path: PathBuf,
    cache: RwLock<Settings>,
}

impl SettingsStore {
    pub fn load(path: &Path) -> SettingsLoad {
        let fallback = |message: Option<String>| SettingsLoad {
            settings: Settings::default(),
            recovered: message.is_some(),
            message,
        };

        let Ok(text) = std::fs::read_to_string(path) else {
            // No file yet is the normal first-run case, not a recovery.
            return fallback(None);
        };

        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) if settings.schema_version == SETTINGS_SCHEMA_VERSION => SettingsLoad {
                settings,
                recovered: false,
                message: None,
            },
            Ok(settings) => {
                // A future or older schema: keep what still parses, reset the rest.
                let mut migrated = Settings {
                    schema_version: SETTINGS_SCHEMA_VERSION,
                    ..settings
                };
                if migrated.language.is_empty() {
                    migrated.language = "zh-CN".to_string();
                }
                SettingsLoad {
                    settings: migrated,
                    recovered: true,
                    message: Some("设置已经按新版本更新过了".to_string()),
                }
            }
            Err(_error) => {
                // Preserve the unreadable file so nothing is destroyed silently.
                let backup = path.with_extension(format!("corrupt-{}.json", now_millis()));
                let _ = std::fs::rename(path, &backup);
                fallback(Some(format!(
                    "设置文件读不出来，已恢复成最初的样子。原来的文件保存在 {}",
                    backup.display()
                )))
            }
        }
    }

    pub fn new(path: PathBuf) -> (Self, SettingsLoad) {
        let load = Self::load(&path);
        let store = Self {
            path,
            cache: RwLock::new(load.settings.clone()),
        };
        (store, load)
    }

    pub fn get(&self) -> Settings {
        self.cache.read().clone()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes to a sibling temp file and renames over the original, so an
    /// interrupted save leaves the previous settings intact rather than a
    /// half-written file.
    pub fn save(&self, settings: Settings) -> AppResult<Settings> {
        let mut settings = settings;
        settings.schema_version = SETTINGS_SCHEMA_VERSION;
        if settings.language.trim().is_empty() {
            settings.language = "zh-CN".to_string();
        }
        if let Some(target) = &settings.default_target_root {
            if target.as_os_str().is_empty() {
                settings.default_target_root = None;
            }
        }

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&settings)?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, json.as_bytes())?;
        std::fs::rename(&temporary, &self.path).map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            AppError::Io(error)
        })?;

        *self.cache.write() = settings.clone();
        Ok(settings)
    }

    pub fn reset(&self) -> AppResult<Settings> {
        self.save(Settings::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_conservative() {
        let settings = Settings::default();
        assert!(
            settings.retain_backups,
            "backups must be kept unless asked otherwise"
        );
        assert!(settings.scan_on_start);
        assert_eq!(settings.theme, ThemeMode::System);
        assert!(settings.default_target_root.is_none());
    }

    #[test]
    fn settings_survive_a_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (store, load) = SettingsStore::new(path.clone());
        assert!(!load.recovered);

        let mut settings = store.get();
        settings.theme = ThemeMode::Dark;
        settings.size_unit = SizeUnit::Gigabytes;
        settings.default_target_root = Some(PathBuf::from("/data/cursor"));
        store.save(settings.clone()).unwrap();

        let (reloaded, load) = SettingsStore::new(path);
        assert!(!load.recovered);
        assert_eq!(reloaded.get().theme, ThemeMode::Dark);
        assert_eq!(reloaded.get().size_unit, SizeUnit::Gigabytes);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_defaults_and_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{ this is not json").unwrap();

        let load = SettingsStore::load(&path);
        assert!(load.recovered);
        assert!(load.message.is_some());
        assert_eq!(load.settings, Settings::default());

        let preserved: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(preserved.len(), 1, "the unreadable file must be kept");
    }

    #[test]
    fn a_mismatched_schema_version_is_migrated_not_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let stored = serde_json::json!({
            "schemaVersion": 999,
            "theme": "dark",
            "language": "zh-CN",
            "scanOnStart": false,
            "sizeUnit": "auto",
            "defaultTargetRoot": null,
            "excludedRootIds": [],
            "checkUpdatesAutomatically": true,
            "updateChannel": "stable",
            "retainBackups": true,
            "reduceMotion": false
        });
        std::fs::write(&path, serde_json::to_vec(&stored).unwrap()).unwrap();

        let load = SettingsStore::load(&path);
        assert!(load.recovered);
        assert_eq!(load.settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(
            load.settings.theme,
            ThemeMode::Dark,
            "valid fields are kept"
        );
        assert!(!load.settings.scan_on_start);
    }

    #[test]
    fn build_info_is_not_user_controlled() {
        let info = BuildInfo::current();
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert!(!info.target_os.is_empty());
    }

    #[test]
    fn saving_never_leaves_a_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = SettingsStore::new(dir.path().join("settings.json"));
        store.save(Settings::default()).unwrap();

        let temp_files: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(temp_files.is_empty());
    }
}
