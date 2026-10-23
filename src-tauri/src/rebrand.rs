//! Carrying an install over from the app's previous name, Zynveliq.
//!
//! The app identifier changed from `com.zynveliq.desktop` to
//! `com.talvio.desktop`, and every per-user directory Tauri derives is named
//! after it: settings, history, models and recordings (app data), the WebView
//! profile with its localStorage (app local data on Windows and Linux,
//! `~/Library/WebKit/<id>` on macOS), and the logs. Left alone, an update
//! would start as a fresh install with none of them.
//!
//! [`carry_over_data_dirs`] runs once per launch, before the Tauri builder
//! resolves any path or takes the single-instance lock. For each directory
//! whose new location does not exist yet while the old one does, the old one
//! is renamed into place; if the rename fails (another volume, a file held
//! open), it is copied into a staging directory beside the destination and
//! the staging directory is renamed into place only when the copy finished,
//! so a half-copied directory never passes for a carried-over one. The copy
//! leaves the old directory where it is. Portable installs keep their data
//! beside the executable and are skipped.
//!
//! The keychain (`secret_store`) and autostart ([`remove_legacy_autostart`])
//! carry over separately, at their own first use.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The app identifier before the rename.
pub const LEGACY_IDENTIFIER: &str = "com.zynveliq.desktop";
/// The app identifier now; matches `identifier` in `tauri.conf.json`.
pub const IDENTIFIER: &str = "com.talvio.desktop";
/// The product name before the rename, which autostart entries were named
/// after.
pub const LEGACY_PRODUCT_NAME: &str = "Zynveliq";

/// Prefix of the environment variables the app reads.
pub const ENV_PREFIX: &str = "TALVIO_";
/// Prefix they had under the previous name.
pub const LEGACY_ENV_PREFIX: &str = "ZYNVELIQ_";

/// An environment variable a user sets by hand (`NO_GTK_LAYER_SHELL`,
/// `ALLOW_WAYLAND`, `KEEP_VULKAN_IMPLICIT_LAYERS`, `METAL_RESIDENCY`), by its
/// name without the prefix. `TALVIO_<suffix>` wins; `ZYNVELIQ_<suffix>`, the
/// name it had before the rename, still works, so a setting someone put in a
/// shell profile or a desktop file keeps working after the update.
pub fn user_env_var_os(suffix: &str) -> Option<std::ffi::OsString> {
    pick_env(suffix, |name| std::env::var_os(name))
}

/// [`user_env_var_os`] as UTF-8; a value that is not UTF-8 reads as unset.
pub fn user_env_var(suffix: &str) -> Option<String> {
    user_env_var_os(suffix).and_then(|value| value.into_string().ok())
}

fn pick_env<T>(suffix: &str, read: impl Fn(&str) -> Option<T>) -> Option<T> {
    read(&format!("{ENV_PREFIX}{suffix}")).or_else(|| read(&format!("{LEGACY_ENV_PREFIX}{suffix}")))
}

/// One directory to carry over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirPair {
    /// What it holds, for the log.
    pub what: &'static str,
    pub old: PathBuf,
    pub new: PathBuf,
}

/// What happened to one [`DirPair`].
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The old directory does not exist: nothing to carry.
    NothingToCarry,
    /// The new directory already exists: carried before, or a fresh install.
    AlreadyThere,
    /// Renamed into place.
    Moved,
    /// Copied into place; the old directory was left where it was.
    Copied,
    /// Neither worked. The app starts without the old data.
    Failed(String),
}

/// The base directories Tauri derives app paths from, as the `dirs` crate
/// reports them (Tauri's own path resolver uses the same crate).
#[derive(Debug, Clone, Default)]
pub struct BaseDirs {
    /// `dirs::data_dir()`: `%APPDATA%`, `~/Library/Application Support`,
    /// `$XDG_DATA_HOME`.
    pub data: Option<PathBuf>,
    /// `dirs::data_local_dir()`: `%LOCALAPPDATA%`, the same as `data` on
    /// macOS and Linux.
    pub data_local: Option<PathBuf>,
    /// `dirs::home_dir()`, for the macOS log and WebKit directories.
    pub home: Option<PathBuf>,
}

impl BaseDirs {
    fn current() -> Self {
        Self {
            data: dirs::data_dir(),
            data_local: dirs::data_local_dir(),
            home: dirs::home_dir(),
        }
    }
}

/// Which family of path rules applies. A parameter so every platform's rules
/// are testable on any platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRules {
    /// Windows and Linux: logs under app local data, WebView profile in app
    /// local data (Tauri forces it there on both).
    Default,
    /// macOS: logs under `~/Library/Logs/<id>`, WebKit data under
    /// `~/Library/WebKit/<id>`.
    MacOs,
}

impl PathRules {
    fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Default
        }
    }
}

/// Every directory to carry from `old_id` to `new_id`, following the rules
/// of Tauri's path resolver (`app_data_dir`, `app_local_data_dir`,
/// `app_log_dir`) and of where the WebView keeps its profile.
///
/// A directory that sits inside another one in the list is left out, since
/// carrying the outer one carries it (Windows and Linux keep the logs inside
/// app local data, and Linux's data and local data are one directory).
pub fn dir_pairs(base: &BaseDirs, rules: PathRules, old_id: &str, new_id: &str) -> Vec<DirPair> {
    let mut pairs = Vec::new();
    let mut push = |what: &'static str, root: Option<&PathBuf>, tail: &[&str]| {
        if let Some(root) = root {
            let build = |id: &str| {
                let mut path = root.join(id);
                for part in tail {
                    path = path.join(part);
                }
                path
            };
            pairs.push(DirPair {
                what,
                old: build(old_id),
                new: build(new_id),
            });
        }
    };
    push("app data", base.data.as_ref(), &[]);
    push("app local data", base.data_local.as_ref(), &[]);
    match rules {
        PathRules::Default => {
            push("logs", base.data_local.as_ref(), &["logs"]);
        }
        PathRules::MacOs => {
            let logs = base.home.as_ref().map(|h| h.join("Library").join("Logs"));
            push("logs", logs.as_ref(), &[]);
            let webkit = base.home.as_ref().map(|h| h.join("Library").join("WebKit"));
            push("WebView data", webkit.as_ref(), &[]);
        }
    }
    // Drop exact duplicates and anything nested inside another entry.
    let mut kept: Vec<DirPair> = Vec::new();
    for pair in pairs {
        let covered = kept
            .iter()
            .any(|k| pair.old == k.old || pair.old.starts_with(&k.old));
        if !covered {
            kept.retain(|k| !k.old.starts_with(&pair.old));
            kept.push(pair);
        }
    }
    kept
}

/// Carry one directory over, renaming with `rename` (a parameter so a failed
/// rename is testable).
fn carry_dir_using(
    old: &Path,
    new: &Path,
    rename: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Outcome {
    if new.exists() {
        return Outcome::AlreadyThere;
    }
    if !old.is_dir() {
        return Outcome::NothingToCarry;
    }
    if let Some(parent) = new.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return Outcome::Failed(format!("could not create {}: {e}", parent.display()));
        }
    }
    let rename_error = match rename(old, new) {
        Ok(()) => return Outcome::Moved,
        Err(e) => e,
    };
    let staging = staging_path(new);
    let _ = std::fs::remove_dir_all(&staging);
    let copied = copy_dir_recursive(old, &staging).and_then(|()| rename(&staging, new));
    match copied {
        Ok(()) => Outcome::Copied,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Outcome::Failed(format!("rename failed ({rename_error}), copy failed ({e})"))
        }
    }
}

/// Where a copy is assembled before it is renamed into place.
fn staging_path(new: &Path) -> PathBuf {
    let mut name = new
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".carrying");
    new.with_file_name(name)
}

/// Copy a directory tree. Symbolic links are skipped rather than followed, so
/// a link loop or a link to somewhere large cannot run away.
fn copy_dir_recursive(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_symlink() {
            continue;
        } else if kind.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Carry every pair over, in order, and describe what happened.
pub fn carry_pairs(pairs: &[DirPair]) -> Vec<(DirPair, Outcome)> {
    pairs
        .iter()
        .map(|pair| {
            let outcome = carry_dir_using(&pair.old, &pair.new, |a, b| std::fs::rename(a, b));
            (pair.clone(), outcome)
        })
        .collect()
}

/// Lines describing what [`carry_over_data_dirs`] did, kept until the logger
/// exists ([`log_report`]).
static REPORT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Carry the Zynveliq directories over to Talvio's. Call once at startup,
/// after `portable::init` and before the Tauri builder. Never fails: a
/// directory that cannot be carried is reported and left alone.
pub fn carry_over_data_dirs() {
    if crate::portable::is_portable() {
        return;
    }
    let pairs = dir_pairs(
        &BaseDirs::current(),
        PathRules::current(),
        LEGACY_IDENTIFIER,
        IDENTIFIER,
    );
    let mut report = Vec::new();
    for (pair, outcome) in carry_pairs(&pairs) {
        let line = match outcome {
            Outcome::NothingToCarry | Outcome::AlreadyThere => continue,
            Outcome::Moved => format!(
                "moved {} from {} to {}",
                pair.what,
                pair.old.display(),
                pair.new.display()
            ),
            Outcome::Copied => format!(
                "copied {} from {} to {} (the old directory was left in place)",
                pair.what,
                pair.old.display(),
                pair.new.display()
            ),
            Outcome::Failed(why) => format!(
                "could not carry {} from {} to {}: {why}",
                pair.what,
                pair.old.display(),
                pair.new.display()
            ),
        };
        eprintln!("[rebrand] {line}");
        report.push(line);
    }
    if let Ok(mut stored) = REPORT.lock() {
        stored.extend(report);
    }
}

/// Write what [`carry_over_data_dirs`] did to the app log. Call once the log
/// plugin is up.
pub fn log_report() {
    let lines = REPORT
        .lock()
        .map(|mut lines| std::mem::take(&mut *lines))
        .unwrap_or_default();
    for line in lines {
        if line.starts_with("could not") {
            log::warn!("Carrying over from Zynveliq: {line}");
        } else {
            log::info!("Carrying over from Zynveliq: {line}");
        }
    }
}

/// Remove an autostart entry created under the old product name (the HKCU
/// `Run` value on Windows, the LaunchAgent plist on macOS, the XDG autostart
/// `.desktop` file on Linux). It points at the Zynveliq executable, which an
/// update replaces, so it can only start nothing or the wrong thing. The
/// caller re-applies the user's autostart choice under the new name.
#[cfg(desktop)]
pub fn remove_legacy_autostart() {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| LEGACY_PRODUCT_NAME.to_string());
    let legacy = auto_launch::AutoLaunchBuilder::new()
        .set_app_name(LEGACY_PRODUCT_NAME)
        .set_app_path(&exe)
        // The plugin is set up with `MacosLauncher::LaunchAgent`.
        .set_use_launch_agent(true)
        .build();
    let Ok(legacy) = legacy else {
        return;
    };
    if legacy.is_enabled().unwrap_or(false) {
        match legacy.disable() {
            Ok(()) => log::info!("Removed the old Zynveliq autostart entry"),
            Err(e) => log::warn!("Could not remove the old Zynveliq autostart entry: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(root: &Path) -> BaseDirs {
        BaseDirs {
            data: Some(root.join("Roaming")),
            data_local: Some(root.join("Local")),
            home: Some(root.join("home")),
        }
    }

    #[test]
    fn the_new_env_name_wins_and_the_old_one_still_works() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            pick_env(
                "ALLOW_WAYLAND",
                env(&[
                    ("TALVIO_ALLOW_WAYLAND", "new"),
                    ("ZYNVELIQ_ALLOW_WAYLAND", "old")
                ])
            ),
            Some("new".to_string())
        );
        assert_eq!(
            pick_env("ALLOW_WAYLAND", env(&[("ZYNVELIQ_ALLOW_WAYLAND", "old")])),
            Some("old".to_string())
        );
        assert_eq!(pick_env("ALLOW_WAYLAND", env(&[])), None);
    }

    #[test]
    fn identifiers_follow_the_rename() {
        assert_eq!(IDENTIFIER, "com.talvio.desktop");
        assert_eq!(LEGACY_IDENTIFIER, "com.zynveliq.desktop");
    }

    #[test]
    fn windows_style_pairs_fold_the_logs_into_local_data() {
        let root = Path::new("/r");
        let pairs = dir_pairs(&base(root), PathRules::Default, "old.id", "new.id");
        assert_eq!(
            pairs,
            vec![
                DirPair {
                    what: "app data",
                    old: root.join("Roaming").join("old.id"),
                    new: root.join("Roaming").join("new.id"),
                },
                DirPair {
                    what: "app local data",
                    old: root.join("Local").join("old.id"),
                    new: root.join("Local").join("new.id"),
                },
            ]
        );
    }

    #[test]
    fn linux_style_pairs_collapse_to_one_directory() {
        let share = PathBuf::from("/home/me/.local/share");
        let dirs = BaseDirs {
            data: Some(share.clone()),
            data_local: Some(share.clone()),
            home: Some(PathBuf::from("/home/me")),
        };
        let pairs = dir_pairs(&dirs, PathRules::Default, "old.id", "new.id");
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].old, share.join("old.id"));
        assert_eq!(pairs[0].new, share.join("new.id"));
    }

    #[test]
    fn macos_pairs_include_logs_and_webkit() {
        let home = PathBuf::from("/Users/me");
        let support = home.join("Library").join("Application Support");
        let dirs = BaseDirs {
            data: Some(support.clone()),
            data_local: Some(support.clone()),
            home: Some(home.clone()),
        };
        let pairs = dir_pairs(&dirs, PathRules::MacOs, "old.id", "new.id");
        let olds: Vec<PathBuf> = pairs.iter().map(|p| p.old.clone()).collect();
        assert_eq!(
            olds,
            vec![
                support.join("old.id"),
                home.join("Library").join("Logs").join("old.id"),
                home.join("Library").join("WebKit").join("old.id"),
            ]
        );
    }

    #[test]
    fn a_missing_base_directory_is_skipped() {
        let pairs = dir_pairs(&BaseDirs::default(), PathRules::MacOs, "a", "b");
        assert!(pairs.is_empty());
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_old_directory_is_renamed_into_place() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("com.zynveliq.desktop");
        let new = tmp.path().join("com.talvio.desktop");
        write(&old.join("settings_store.json"), "{}");
        write(&old.join("recordings").join("a.wav"), "wav");

        let outcome = carry_dir_using(&old, &new, |a, b| std::fs::rename(a, b));
        assert_eq!(outcome, Outcome::Moved);
        assert!(!old.exists());
        assert_eq!(
            std::fs::read_to_string(new.join("recordings").join("a.wav")).unwrap(),
            "wav"
        );
    }

    #[test]
    fn an_existing_new_directory_is_never_touched() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let new = tmp.path().join("new");
        write(&old.join("settings_store.json"), "old");
        write(&new.join("settings_store.json"), "new");

        let outcome = carry_dir_using(&old, &new, |a, b| std::fs::rename(a, b));
        assert_eq!(outcome, Outcome::AlreadyThere);
        assert_eq!(
            std::fs::read_to_string(new.join("settings_store.json")).unwrap(),
            "new"
        );
        assert!(old.exists(), "the old data stays when nothing was carried");
    }

    #[test]
    fn nothing_to_carry_on_a_fresh_install() {
        let tmp = tempfile::tempdir().unwrap();
        let outcome = carry_dir_using(&tmp.path().join("old"), &tmp.path().join("new"), |a, b| {
            std::fs::rename(a, b)
        });
        assert_eq!(outcome, Outcome::NothingToCarry);
        assert!(!tmp.path().join("new").exists());
    }

    #[test]
    fn a_failed_rename_falls_back_to_a_copy_and_keeps_the_old_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let new = tmp.path().join("deeper").join("new");
        write(&old.join("history.db"), "rows");
        write(&old.join("models").join("m.gguf"), "weights");

        // Refuse to rename the old directory itself, as a cross-volume move
        // would; allow the staging directory's rename.
        let refuse_old = |a: &Path, b: &Path| {
            if a == old.as_path() {
                Err(io::Error::other("cross-device link"))
            } else {
                std::fs::rename(a, b)
            }
        };
        let outcome = carry_dir_using(&old, &new, refuse_old);
        assert_eq!(outcome, Outcome::Copied);
        assert_eq!(
            std::fs::read_to_string(new.join("history.db")).unwrap(),
            "rows"
        );
        assert_eq!(
            std::fs::read_to_string(new.join("models").join("m.gguf")).unwrap(),
            "weights"
        );
        assert!(old.join("history.db").exists());
        assert!(!staging_path(&new).exists());
    }

    #[test]
    fn a_failed_copy_leaves_no_half_copied_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("old");
        let new = tmp.path().join("new");
        write(&old.join("a.txt"), "a");

        let outcome = carry_dir_using(&old, &new, |_, _| Err(io::Error::other("no")));
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        assert!(!new.exists(), "a later launch must be able to try again");
        assert!(!staging_path(&new).exists());
        assert!(old.join("a.txt").exists());
    }

    #[test]
    fn carry_pairs_moves_every_pair() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = base(tmp.path());
        write(
            &tmp.path()
                .join("Roaming")
                .join("old.id")
                .join("settings_store.json"),
            "{}",
        );
        write(
            &tmp.path()
                .join("Local")
                .join("old.id")
                .join("logs")
                .join("zynveliq.log"),
            "log",
        );
        let pairs = dir_pairs(&dirs, PathRules::Default, "old.id", "new.id");
        let outcomes: Vec<Outcome> = carry_pairs(&pairs).into_iter().map(|(_, o)| o).collect();
        assert_eq!(outcomes, vec![Outcome::Moved, Outcome::Moved]);
        assert!(tmp
            .path()
            .join("Local")
            .join("new.id")
            .join("logs")
            .join("zynveliq.log")
            .exists());

        // A second launch finds everything in place.
        let again: Vec<Outcome> = carry_pairs(&pairs).into_iter().map(|(_, o)| o).collect();
        assert_eq!(again, vec![Outcome::AlreadyThere, Outcome::AlreadyThere]);
    }
}
