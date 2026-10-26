//! Which files a command may send out of this computer, and where a Windows
//! shortcut points.
//!
//! "Email me the project report" sends a file to someone, possibly from a
//! phone, with nobody looking at the screen. So the file has to be one of the
//! user's own documents: inside their user folder, not hidden, not app data
//! (`AppData`, `~/Library`, where browser profiles and password stores live),
//! and not anything that reads as a key or a credential. A search that finds
//! only such a file says so instead of sending it.
//!
//! Windows' recent-files list holds shortcuts (`report.docx.lnk` in
//! `AppData\...\Recent`), so the rules apply to the file a shortcut points at,
//! read by [`shortcut_target`] from the shortcut's own bytes.

use std::fs;
use std::path::{Component, Path, PathBuf};

/// Gmail's limit on what one email carries as attachments.
pub(crate) const MAX_ATTACHMENT_BYTES: u64 = 25 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    /// A test's stand-in for the user's folder ([`set_test_home`]).
    static TEST_HOME: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// Make [`home_dir`] answer `home` on this thread, for a test whose files
/// live in a temporary folder (which itself sits in `AppData`).
#[cfg(test)]
pub(crate) fn set_test_home(home: &Path) {
    TEST_HOME.with(|cell| *cell.borrow_mut() = Some(home.to_path_buf()));
}

/// The user's own folder: `USERPROFILE` on Windows, `HOME` elsewhere.
pub(crate) fn home_dir() -> Option<PathBuf> {
    #[cfg(test)]
    if let Some(home) = TEST_HOME.with(|cell| cell.borrow().clone()) {
        return Some(home);
    }
    let var = if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    };
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
}

/// Folders under the home folder that hold an app's data rather than the
/// user's documents (browser profiles, password stores, keychains). Compared
/// case-insensitively against every folder on the way to a file.
const PRIVATE_DIRS: &[&str] = &[
    "appdata",
    "application data",
    "local settings",
    "library",
    "keychains",
    "ntuser",
];

/// Extensions of keys, certificates, keystores and password databases.
const SECRET_EXTENSIONS: &[&str] = &[
    "pem",
    "key",
    "pfx",
    "p12",
    "ppk",
    "kdbx",
    "kdb",
    "keychain",
    "keychain-db",
    "jks",
    "keystore",
    "gpg",
    "pgp",
    "ovpn",
    "rdp",
    "cred",
    "credentials",
];

/// Words that, in a file's name, mark it as a secret.
const SECRET_NAME_WORDS: &[&str] = &[
    "password",
    "passwords",
    "passwd",
    "credential",
    "credentials",
    "secret",
    "secrets",
    "private key",
    "privatekey",
    "recovery code",
    "recovery codes",
    "seed phrase",
    "wallet.dat",
];

/// SSH and similar key file names, which have no extension.
const KEY_FILE_PREFIXES: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519"];

/// Why `path` must not be sent out, or `None` when it may be. `home` is the
/// user's own folder; `path` is the file itself (a shortcut already followed).
pub(crate) fn send_refusal(path: &Path, home: &Path) -> Option<&'static str> {
    let Some(inside) = relative_to(path, home) else {
        return Some("it isn't in your user folder");
    };
    let folders: Vec<String> = inside
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|c| match c {
                    Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if folders.iter().any(|f| f.starts_with('.')) {
        return Some("it is in a hidden folder");
    }
    if folders.iter().any(|f| PRIVATE_DIRS.contains(&f.as_str())) {
        return Some("it belongs to an app, not to your documents");
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.starts_with('.') {
        return Some("it is a hidden file");
    }
    if KEY_FILE_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return Some("it looks like a key");
    }
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if SECRET_EXTENSIONS.contains(&extension.as_str()) {
        return Some("it looks like a key, a certificate or a password store");
    }
    let spaced = name.replace(['_', '-', '.'], " ");
    if SECRET_NAME_WORDS
        .iter()
        .any(|word| name.contains(word) || spaced.contains(word))
    {
        return Some("its name says it holds passwords or secrets");
    }
    if is_hidden_or_system(path) {
        return Some("it is a hidden or system file");
    }
    None
}

/// `path` below `home`, when it is inside it: compared after resolving both
/// (so `..` and links cannot step out), and case-insensitively on Windows.
fn relative_to(path: &Path, home: &Path) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let home = fs::canonicalize(home).ok()?;
    if cfg!(target_os = "windows") {
        let p = path.to_string_lossy().to_lowercase();
        let h = home.to_string_lossy().to_lowercase();
        let h = h.trim_end_matches(['\\', '/']);
        let rest = p.strip_prefix(h)?;
        if !(rest.is_empty() || rest.starts_with(['\\', '/'])) {
            return None;
        }
        let rest = &path.to_string_lossy()[h.len()..];
        Some(PathBuf::from(rest.trim_start_matches(['\\', '/'])))
    } else {
        path.strip_prefix(&home).ok().map(Path::to_path_buf)
    }
}

#[cfg(target_os = "windows")]
fn is_hidden_or_system(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    fs::metadata(path).is_ok_and(|meta| meta.file_attributes() & (HIDDEN | SYSTEM) != 0)
}

#[cfg(not(target_os = "windows"))]
fn is_hidden_or_system(_path: &Path) -> bool {
    false
}

/// Whether `path` is a Windows shortcut.
pub(crate) fn is_shortcut(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
}

/// The file a Windows shortcut points at, read from the shortcut itself
/// ([`parse_shortcut`]). `None` when it cannot be read or points nowhere local.
pub(crate) fn shortcut_target(path: &Path) -> Option<PathBuf> {
    let bytes = fs::read(path).ok()?;
    parse_shortcut(&bytes)
}

/// The local target path in a Shell Link (`.lnk`) file, per [MS-SHLLINK]:
/// the header, an optional item-ID list to skip, then the LinkInfo block,
/// whose local base path (Unicode when present, else the ANSI one) is the
/// target. Network and item-ID-only shortcuts answer `None`.
pub(crate) fn parse_shortcut(bytes: &[u8]) -> Option<PathBuf> {
    const HEADER_SIZE: usize = 0x4C;
    const HAS_ID_LIST: u32 = 0x1;
    const HAS_LINK_INFO: u32 = 0x2;
    const VOLUME_ID_AND_LOCAL_BASE_PATH: u32 = 0x1;

    let u16_at = |at: usize| -> Option<u16> {
        Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
    };
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    };

    if u32_at(0)? as usize != HEADER_SIZE {
        return None;
    }
    let flags = u32_at(0x14)?;
    let mut at = HEADER_SIZE;
    if flags & HAS_ID_LIST != 0 {
        at += 2 + u16_at(at)? as usize;
    }
    if flags & HAS_LINK_INFO == 0 {
        return None;
    }
    let info = at;
    let header_size = u32_at(info + 4)? as usize;
    let info_flags = u32_at(info + 8)?;
    if info_flags & VOLUME_ID_AND_LOCAL_BASE_PATH == 0 {
        return None;
    }
    if header_size >= 0x24 {
        let offset = u32_at(info + 0x1C)? as usize;
        if offset != 0 {
            let start = info + offset;
            let mut units = Vec::new();
            let mut i = start;
            loop {
                let unit = u16_at(i)?;
                if unit == 0 {
                    break;
                }
                units.push(unit);
                i += 2;
            }
            let text = String::from_utf16(&units).ok()?;
            return (!text.is_empty()).then(|| PathBuf::from(text));
        }
    }
    let offset = u32_at(info + 0x10)? as usize;
    let start = info + offset;
    let end = start + bytes.get(start..)?.iter().position(|&b| b == 0)?;
    let raw = bytes.get(start..end)?;
    // The ANSI path is in the system code page; ASCII is all that can be
    // read without it, and anything else answers `None` rather than a
    // garbled path.
    if !raw.is_ascii() || raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(String::from_utf8_lossy(raw).into_owned()))
}

/// The file `path` stands for: a shortcut's target, or itself.
pub(crate) fn followed(path: &Path) -> Option<PathBuf> {
    if is_shortcut(path) {
        shortcut_target(path)
    } else {
        Some(path.to_path_buf())
    }
}

/// Whether a found path (a file, or a shortcut to one) may be sent out.
pub(crate) fn may_send(path: &Path, home: &Path) -> bool {
    followed(path)
        .filter(|target| target.is_file())
        .is_some_and(|target| send_refusal(&target, home).is_none())
}

/// "3.4 MB": a size as people read it.
pub(crate) fn readable_size(bytes: u64) -> String {
    let mb = bytes as f64 / (1024.0 * 1024.0);
    if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn touch(dir: &Path, rel: &str) -> PathBuf {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "x").unwrap();
        path
    }

    #[test]
    fn documents_in_the_user_folder_may_be_sent() {
        let home = home();
        for rel in [
            "Documents/Project report.pdf",
            "Downloads/invoice-2026.pdf",
            "Desktop/notes.txt",
            "Documents/Work/Q3/budget.xlsx",
        ] {
            let path = touch(home.path(), rel);
            assert_eq!(send_refusal(&path, home.path()), None, "{rel}");
        }
    }

    #[test]
    fn keys_secrets_hidden_and_app_data_are_never_sent() {
        let home = home();
        for rel in [
            ".ssh/id_rsa",
            "Documents/id_ed25519",
            ".aws/credentials",
            "AppData/Local/BraveSoftware/Login Data",
            "AppData/Roaming/notes.txt",
            "Library/Keychains/login.keychain-db",
            "Documents/server.pem",
            "Documents/cert.pfx",
            "Documents/Passwords.kdbx",
            "Documents/my passwords.txt",
            "Desktop/bank_credentials.xlsx",
            "Documents/.env",
            "Documents/secret-plans.docx",
        ] {
            let path = touch(home.path(), rel);
            assert!(send_refusal(&path, home.path()).is_some(), "{rel}");
        }
    }

    #[test]
    fn nothing_outside_the_user_folder_is_sent() {
        let home = home();
        let elsewhere = tempfile::tempdir().unwrap();
        let path = touch(elsewhere.path(), "report.pdf");
        assert!(send_refusal(&path, home.path()).is_some());
        // `..` cannot step out of it either.
        let sneaky = home
            .path()
            .join("..")
            .join(elsewhere.path().file_name().unwrap());
        let sneaky = sneaky.join("report.pdf");
        assert!(send_refusal(&sneaky, home.path()).is_some());
    }

    #[test]
    fn a_shortcut_is_judged_by_its_target() {
        let home = home();
        let target = touch(home.path(), "Documents/report.pdf");
        let link = home
            .path()
            .join("AppData/Roaming/Microsoft/Windows/Recent/report.pdf.lnk");
        fs::create_dir_all(link.parent().unwrap()).unwrap();
        fs::write(&link, shortcut_bytes(&target.to_string_lossy(), true)).unwrap();
        assert_eq!(followed(&link), Some(target.clone()));
        assert!(may_send(&link, home.path()));
        // A shortcut to a key is refused like the key.
        let key = touch(home.path(), ".ssh/id_rsa");
        fs::write(&link, shortcut_bytes(&key.to_string_lossy(), false)).unwrap();
        assert!(!may_send(&link, home.path()));
    }

    #[test]
    fn shortcuts_that_point_nowhere_local_read_as_none() {
        assert_eq!(parse_shortcut(&[]), None);
        assert_eq!(parse_shortcut(&[0u8; 0x4C]), None);
        let mut no_info = vec![0u8; 0x4C];
        no_info[0] = 0x4C;
        assert_eq!(parse_shortcut(&no_info), None);
    }

    #[test]
    fn sizes_read_as_people_say_them() {
        assert_eq!(readable_size(3_565_158), "3.4 MB");
        assert_eq!(readable_size(30 * 1024 * 1024), "30 MB");
    }

    /// A minimal Shell Link: header, a small item-ID list, and a LinkInfo
    /// with the local base path, Unicode (`unicode`) or ANSI.
    fn shortcut_bytes(target: &str, unicode: bool) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x4C];
        bytes[0..4].copy_from_slice(&0x4Cu32.to_le_bytes());
        bytes[0x14..0x18].copy_from_slice(&(0x1u32 | 0x2u32).to_le_bytes());
        // Item-ID list: 4 bytes of content, then its terminator.
        bytes.extend_from_slice(&6u16.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 6]);
        let info_start = bytes.len();
        let header_size: u32 = if unicode { 0x24 } else { 0x1C };
        let ansi: Vec<u8> = target.bytes().chain([0]).collect();
        let wide: Vec<u8> = target
            .encode_utf16()
            .chain([0])
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let ansi_offset = header_size;
        let wide_offset = ansi_offset + ansi.len() as u32;
        let mut info = vec![0u8; header_size as usize];
        info[4..8].copy_from_slice(&header_size.to_le_bytes());
        info[8..12].copy_from_slice(&1u32.to_le_bytes());
        info[0x10..0x14].copy_from_slice(&ansi_offset.to_le_bytes());
        if unicode {
            info[0x1C..0x20].copy_from_slice(&wide_offset.to_le_bytes());
        }
        info.extend_from_slice(&ansi);
        if unicode {
            info.extend_from_slice(&wide);
        }
        let size = info.len() as u32;
        info[0..4].copy_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&info);
        let _ = info_start;
        bytes
    }
}
