//! `open_file`: open a document the user names ("open my resume", "the budget
//! spreadsheet") with its default application.
//!
//! Where to look is the hard part. The platform's recent-files list is the best
//! evidence of which "resume" the user means, so it is searched first and wins
//! a tie. Behind it is a bounded walk of the folders people keep files in
//! (Documents, Downloads, Desktop, home), because a file the user has not
//! opened lately is still one they can name. The walk is bounded on purpose,
//! by depth and by the number of entries read, since it runs while the user
//! waits and a home folder can hold a million files: a name that is buried
//! deeper than that is a question for search, not for a voice command.
//!
//! What the walk skips is as deliberate as where it goes: hidden folders,
//! dependency and cache trees (`node_modules`, `AppData`, `~/Library`), app
//! bundles, Office lock files, and symlinked folders, which can loop or lead
//! off the disk.
//!
//! **It opens documents, never programs.** Opening a file with its default
//! app runs an `.exe`, an installer or a script (a `.js` or `.vbs` under
//! Windows Script Host, a `.command` in Terminal), and Downloads is exactly
//! where an `invoice.pdf.exe` attachment lands; a fuzzy match on "open the
//! invoice" must not start it. So anything runnable ([`is_program`], read
//! through a shortcut to its target's name) is dropped before scoring, and
//! when only programs match, the user is pointed at `open_app` instead.
//! Starting a program is that tool's job, and it only launches installed apps.

use serde_json::{json, Value};
use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use crate::command_mode::fuzzy::{self, DEFAULT_LEAD, DEFAULT_MIN_SCORE};
use crate::command_mode::registry::{arg_str, Risk, ToolCtx, ToolOutcome, ToolSpec, ALL_PLATFORMS};

pub static SPEC: ToolSpec = ToolSpec {
    name: "open_file",
    description: "Open a file on this computer by name with its default app (\"open my resume\", \
        \"open the budget spreadsheet\"), or one of the Documents, Downloads and Desktop folders \
        themselves (\"open my downloads folder\"). Searches recent files and those folders. Asks \
        when several files match.",
    schema,
    risk: Risk::Safe,
    platforms: ALL_PLATFORMS,
    keywords: &[
        "open", "file", "document", "spreadsheet", "presentation", "pdf", "photo", "picture",
        "video", "folder", "resume",
    ],
    handler,
    on_confirm: None,
};

/// How many folder levels below each root the walk enters.
const MAX_DEPTH: usize = 3;
/// How many directory entries (files and folders alike) the walk reads in all.
const MAX_ENTRIES: usize = 4000;
/// At most this many files are offered in a question.
const MAX_CHOICES: usize = 5;

/// What the user hears when the only files that match are programs.
const PROGRAM_REFUSAL: &str =
    "I only open documents this way. To start a program, say open <app name>.";

/// Extensions of files that run something when opened, compared
/// case-insensitively against the name a file is known by ([`named`], so a
/// shortcut `setup.exe.lnk` counts as `exe`). Windows executables, installers
/// and packages; script hosts and shells everywhere; macOS apps, installers
/// and disk images; Linux launchers and packages. Windows' own `.lnk` is not
/// here: a Recent item is a shortcut to a document, and one whose target is a
/// program carries that program's extension in its name.
const PROGRAM_EXTENSIONS: &[&str] = &[
    // Windows
    "exe", "com", "scr", "pif", "cpl", "msc", "msi", "msp", "mst", "msix", "msixbundle", "appx",
    "appxbundle", "appinstaller", "application", "appref-ms", "gadget", "inf", "reg", "url", "scf",
    "settingcontent-ms", "hta", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf",
    "wsh", "jar",
    // macOS
    "app", "pkg", "mpkg", "dmg", "command", "tool", "terminal", "workflow", "scpt",
    "applescript",
    // Shells and interpreters
    "sh", "bash", "zsh", "fish", "csh", "ksh", "py", "pyw", "pl", "rb",
    // Linux
    "run", "bin", "appimage", "deb", "rpm", "desktop",
];

fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name": {
                "type": "string",
                "minLength": 1,
                "maxLength": 260,
                "description": "The file's name as the user said it; the extension may be left out, e.g. \"resume\" or \"Q3 budget\"."
            },
            "kind": {
                "type": "string",
                "enum": ["document", "spreadsheet", "presentation", "pdf", "image", "video", "audio", "any"],
                "description": "The kind of file when the user said it (\"the budget spreadsheet\"); otherwise omit or use any."
            }
        },
        "required": ["name"],
        "additionalProperties": false
    })
}

fn handler(ctx: &ToolCtx, args: Value) -> ToolOutcome {
    let Some(name) = arg_str(&args, "name") else {
        return ToolOutcome::failed("Which file should I open?", false);
    };
    // "Open my Downloads folder": the folder itself, which the walk below
    // never offers (it lists the files in it).
    if let Some(folder) = known_folder_named(name, &ctx.effects.known_folders()) {
        return match ctx.effects.open_path(&folder) {
            Ok(()) => ToolOutcome::done(
                "openingFile",
                "Opening {{file}}.",
                &[("file", &file_name(&folder))],
            ),
            Err(e) => ToolOutcome::failed(e, false),
        };
    }
    let kind = Kind::parse(arg_str(&args, "kind"));
    let query = clean_file_query(name);

    let recent: Vec<Candidate> = ctx
        .effects
        .recent_files()
        .into_iter()
        .filter(|path| path.is_file())
        .map(|path| Candidate { path, recent: true })
        .collect();
    let recent_documents: Vec<Candidate> = recent
        .iter()
        .filter(|candidate| !is_program(&candidate.path))
        .cloned()
        .collect();

    // A single strong match among recent files cannot be beaten by anything
    // the walk finds (recent files win ties), so the disk is left alone.
    let choice = match choose(&query, kind, &recent_documents) {
        Choice::One(candidate, score) if score >= 1.0 - DEFAULT_LEAD => {
            Choice::One(candidate, score)
        }
        _ => {
            let walked = walk_files(&ctx.effects.known_folders(), MAX_DEPTH, MAX_ENTRIES);
            log::debug!(
                "open_file: {} recent files, {} found in known folders",
                recent.len(),
                walked.len()
            );
            // Programs never compete with documents; they only decide what
            // to say when nothing else matched.
            let (programs, documents): (Vec<Candidate>, Vec<Candidate>) = merge(recent, walked)
                .into_iter()
                .partition(|candidate| is_program(&candidate.path));
            match choose(&query, kind, &documents) {
                Choice::Nothing if choose(&query, kind, &programs) != Choice::Nothing => {
                    log::info!("open_file: only programs matched; not opening any");
                    return ToolOutcome::failed(PROGRAM_REFUSAL, false);
                }
                choice => choice,
            }
        }
    };

    match choice {
        Choice::One(candidate, _) => open(ctx, &candidate.path),
        Choice::Several(options) => ToolOutcome::ask(
            "Which file?",
            dedupe(options.iter().map(|c| choice_label(&c.path)).collect()),
        ),
        Choice::Nothing => {
            ToolOutcome::failed(format!("I couldn't find a file called {name}."), false)
        }
    }
}

/// The known folder (Documents, Downloads, Desktop, home) a spoken name means
/// on its own: "Downloads", "my downloads folder", "the desktop". Only the
/// whole name: "the budget in downloads" is a file, and is left to the walk.
fn known_folder_named(spoken: &str, folders: &[PathBuf]) -> Option<PathBuf> {
    const FILLER: &[&str] = &["my", "the", "folder", "directory", "dir", "files"];
    let words: Vec<String> = spoken
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .filter(|word| !FILLER.contains(&word.as_str()))
        .collect();
    let [word] = words.as_slice() else {
        return None;
    };
    let singular = word.trim_end_matches('s');
    folders
        .iter()
        .find(|folder| {
            folder
                .file_name()
                .map(|name| name.to_string_lossy().to_lowercase())
                .is_some_and(|name| name == *word || name.trim_end_matches('s') == singular)
        })
        .cloned()
}

fn open(ctx: &ToolCtx, path: &Path) -> ToolOutcome {
    // `handler` never picks a program; this keeps any later route to `open`
    // from opening one either.
    if is_program(path) {
        return ToolOutcome::failed(PROGRAM_REFUSAL, false);
    }
    match ctx.effects.open_path(path) {
        Ok(()) => ToolOutcome::done("openingFile", "Opening {{file}}.", &[("file", &file_name(path))]),
        Err(e) => {
            log::warn!("open_file: opening a file failed: {e}");
            // The file exists; what failed is its default app, which clicking
            // around would not fix.
            ToolOutcome::failed(e, false)
        }
    }
}

/// The kind of file the user asked for, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
    Image,
    Video,
    Audio,
    Any,
}

impl Kind {
    fn parse(value: Option<&str>) -> Self {
        match value.map(str::to_ascii_lowercase).as_deref() {
            Some("document") => Kind::Document,
            Some("spreadsheet") => Kind::Spreadsheet,
            Some("presentation") => Kind::Presentation,
            Some("pdf") => Kind::Pdf,
            Some("image") => Kind::Image,
            Some("video") => Kind::Video,
            Some("audio") => Kind::Audio,
            _ => Kind::Any,
        }
    }

    /// Lowercase extensions of this kind; `None` for any file.
    fn extensions(self) -> Option<&'static [&'static str]> {
        let list: &'static [&'static str] = match self {
            Kind::Document => &[
                "doc", "docx", "docm", "odt", "rtf", "txt", "md", "pages", "wpd",
            ],
            Kind::Spreadsheet => &["xls", "xlsx", "xlsm", "xlsb", "ods", "csv", "tsv", "numbers"],
            Kind::Presentation => &["ppt", "pptx", "pptm", "odp", "key"],
            Kind::Pdf => &["pdf"],
            Kind::Image => &[
                "jpg", "jpeg", "png", "gif", "bmp", "webp", "heic", "heif", "tif", "tiff", "svg",
            ],
            Kind::Video => &["mp4", "mov", "m4v", "avi", "mkv", "webm", "wmv"],
            Kind::Audio => &["mp3", "wav", "m4a", "flac", "aac", "ogg", "opus", "wma"],
            Kind::Any => return None,
        };
        Some(list)
    }

    fn matches(self, path: &Path) -> bool {
        let Some(extensions) = self.extensions() else {
            return true;
        };
        named(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.iter().any(|known| known.eq_ignore_ascii_case(ext)))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    path: PathBuf,
    /// From the platform's recent-files list rather than the folder walk.
    recent: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Choice {
    /// The file, and how well it matched.
    One(Candidate, f64),
    Several(Vec<Candidate>),
    Nothing,
}

/// Recent files first, then what the walk found, each path once.
fn merge(recent: Vec<Candidate>, walked: Vec<PathBuf>) -> Vec<Candidate> {
    let mut seen = HashSet::new();
    recent
        .into_iter()
        .chain(walked.into_iter().map(|path| Candidate { path, recent: false }))
        .filter(|candidate| seen.insert(path_key(&candidate.path)))
        .collect()
}

/// Choose the file `query` names among `candidates`, which are in preference
/// order (recent first, most recent first).
///
/// The fuzzy rule with one twist: among the candidates within the lead of the
/// best, a single recent file wins, because the file the user opened last week
/// is the "resume" they mean far more often than the one from 2019.
fn choose(query: &str, kind: Kind, candidates: &[Candidate]) -> Choice {
    let mut scored: Vec<(f64, usize)> = candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| kind.matches(&candidate.path))
        .map(|(index, candidate)| (file_score(query, &candidate.path), index))
        .filter(|(score, _)| *score >= DEFAULT_MIN_SCORE)
        .collect();
    // Best first; equal scores keep preference order.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let Some(&(top, best)) = scored.first() else {
        return Choice::Nothing;
    };
    let close: Vec<(f64, usize)> = scored
        .iter()
        .copied()
        .take_while(|(score, _)| top - score < DEFAULT_LEAD)
        .collect();
    if close.len() == 1 {
        return Choice::One(candidates[best].clone(), top);
    }
    let recent: Vec<&(f64, usize)> = close
        .iter()
        .filter(|(_, index)| candidates[*index].recent)
        .collect();
    if let [(score, index)] = recent.as_slice() {
        return Choice::One(candidates[*index].clone(), *score);
    }
    Choice::Several(
        close
            .iter()
            .take(MAX_CHOICES)
            .map(|(_, index)| candidates[*index].clone())
            .collect(),
    )
}

/// How well `query` names the file at `path`: the best of its stem ("Resume
/// 2024") and its full name ("Resume 2024.pdf"). The label a question shows
/// ("Resume 2024.pdf (Documents)") counts only when the query is that label
/// exactly, so an answer echoing a choice picks it. Scored fuzzily, the label
/// made the folder's name part of every file's name: "open taxes" picked the
/// one file in a folder called Taxes, whatever it was called.
fn file_score(query: &str, path: &Path) -> f64 {
    let stem = named(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let echoed = if fuzzy::score(query, &choice_label(path)) >= 1.0 {
        1.0
    } else {
        0.0
    };
    [stem, file_name(path)]
        .iter()
        .map(|name| fuzzy::score(query, name))
        .fold(echoed, f64::max)
}

/// Whether opening `path` would run something rather than show it: a program,
/// installer, script or launcher by extension ([`PROGRAM_EXTENSIONS`], read
/// through a shortcut to its target's name), or, on Unix, an extensionless
/// file marked executable.
fn is_program(path: &Path) -> bool {
    match named(path).extension().and_then(|ext| ext.to_str()) {
        Some(ext) => PROGRAM_EXTENSIONS
            .iter()
            .any(|program| program.eq_ignore_ascii_case(ext)),
        None => is_executable(path),
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    false
}

/// The path a file is known by. A Windows Recent item is a shortcut named
/// after its target ("report.docx.lnk"), so its name, extension and kind are
/// read without the `.lnk`; the shortcut itself is still what gets opened,
/// which opens the target.
fn named(path: &Path) -> PathBuf {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("lnk") => path.with_extension(""),
        _ => path.to_path_buf(),
    }
}

fn file_name(path: &Path) -> String {
    named(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// "Resume.pdf (Documents)": the name, plus the folder it is in, which is what
/// tells two files of one name apart.
fn choice_label(path: &Path) -> String {
    let name = file_name(path);
    match path
        .parent()
        .and_then(Path::file_name)
        .map(|s| s.to_string_lossy().into_owned())
    {
        Some(parent) if !parent.is_empty() => format!("{name} ({parent})"),
        _ => name,
    }
}

fn dedupe(labels: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    labels
        .into_iter()
        .filter(|label| seen.insert(label.clone()))
        .collect()
}

/// "my resume" → "resume", "the budget file" → "budget". A name made only of
/// such words is left as it was.
fn clean_file_query(spoken: &str) -> String {
    const LEADING: &[&str] = &["my", "the", "a", "an", "that", "this"];
    const TRAILING: &[&str] = &["file", "document"];

    let is_one_of = |word: &str, list: &[&str]| list.iter().any(|w| w.eq_ignore_ascii_case(word));
    let words: Vec<&str> = spoken.split_whitespace().collect();
    let mut start = 0;
    let mut end = words.len();
    while end - start > 1 && is_one_of(words[start], LEADING) {
        start += 1;
    }
    while end - start > 1 && is_one_of(words[end - 1], TRAILING) {
        end -= 1;
    }
    words[start..end].join(" ")
}

/// Two spellings of one path compare equal: case-insensitively on Windows,
/// where the file system is.
fn path_key(path: &Path) -> String {
    let key = path.to_string_lossy().into_owned();
    if cfg!(target_os = "windows") {
        key.to_lowercase()
    } else {
        key
    }
}

/// Every regular file under `roots`, breadth first, so shallow files are found
/// before the entry budget runs out on one deep tree.
///
/// A root is depth 0 and its subfolders depth 1; folders deeper than
/// `max_depth` are not entered. `max_entries` caps the directory entries read,
/// files and folders alike, across all roots. Hidden entries, the folders in
/// [`SKIPPED_DIRS`] and lock files are skipped, a root reached twice (home and
/// home/Documents) is read once, and a symlinked folder is never entered,
/// though a symlink to a file is returned.
fn walk_files(roots: &[PathBuf], max_depth: usize, max_entries: usize) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut queue: VecDeque<(PathBuf, usize)> =
        roots.iter().map(|root| (root.clone(), 0)).collect();
    let mut visited = HashSet::new();
    let mut read = 0usize;

    while let Some((dir, depth)) = queue.pop_front() {
        if !visited.insert(path_key(&dir)) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if read >= max_entries {
                return files;
            }
            read += 1;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_hidden(&entry, &name) {
                continue;
            }
            // `DirEntry::file_type` does not follow links, so a symlinked
            // folder (or a Windows junction) is a symlink here, never a dir.
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                if depth < max_depth && !is_skipped_dir(&name) {
                    queue.push_back((entry.path(), depth + 1));
                }
            } else if is_lock_file(&name) {
                continue;
            } else if file_type.is_file() {
                files.push(entry.path());
            } else if file_type.is_symlink() {
                let path = entry.path();
                if fs::metadata(&path).is_ok_and(|meta| meta.is_file()) {
                    files.push(path);
                }
            }
        }
    }
    files
}

/// Folders that hold an app's or a tool's files rather than the user's, and
/// can hold more entries than the whole budget. Compared case-insensitively.
const SKIPPED_DIRS: &[&str] = &[
    "node_modules",
    "appdata",
    "application data",
    "local settings",
    "library",
    "__pycache__",
    "site-packages",
    "venv",
    "$recycle.bin",
    "system volume information",
];

/// A folder in [`SKIPPED_DIRS`], or an app bundle (`Foo.app`), which is a
/// program on macOS and full of its own executables inside.
fn is_skipped_dir(name: &str) -> bool {
    SKIPPED_DIRS.iter().any(|skipped| skipped.eq_ignore_ascii_case(name))
        || Path::new(name)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

/// Office's "~$Report.docx" and LibreOffice's ".~lock.Report.docx#" are
/// lock files beside an open document, never something to open.
fn is_lock_file(name: &str) -> bool {
    name.starts_with("~$") || name.starts_with(".~lock.")
}

/// Dot-files everywhere, plus the hidden attribute on Windows.
fn is_hidden(entry: &fs::DirEntry, name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        // On Windows `DirEntry::metadata` comes from the directory listing
        // itself and does not follow links, so this costs no extra I/O.
        if entry
            .metadata()
            .is_ok_and(|meta| meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        {
            return true;
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = entry;
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_mode::contacts::ContactBook;
    use crate::command_mode::context::CommandContext;
    use crate::command_mode::effects::fake::FakeEffects;
    use crate::command_mode::registry::{validate_args, Platform};
    use chrono::{FixedOffset, TimeZone};
    use std::sync::Arc;

    fn ctx_with(fake: Arc<FakeEffects>) -> ToolCtx {
        let now = FixedOffset::east_opt(0)
            .unwrap()
            .with_ymd_and_hms(2026, 10, 22, 9, 15, 0)
            .unwrap();
        ToolCtx {
            settings: crate::settings::get_default_settings(),
            context: CommandContext::at(now, None, Platform::Windows, None, None),
            effects: fake,
            contacts: Arc::new(ContactBook::new(Vec::new())),
            platform: Platform::Windows,
            dry_run: false,
        }
    }

    /// Create `relative` (and its folders) under `root`, returning its path.
    fn touch(root: &Path, relative: &str) -> PathBuf {
        // Joined part by part, so the path is spelled exactly as the walk
        // spells it (backslashes on Windows) and the recorded actions compare.
        let path = relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"x").unwrap();
        path
    }

    fn run(recent: Vec<PathBuf>, folders: Vec<PathBuf>, args: Value) -> (ToolOutcome, Vec<String>) {
        let fake = Arc::new(FakeEffects {
            recent,
            folders,
            ..Default::default()
        });
        let outcome = handler(&ctx_with(fake.clone()), args);
        (outcome, fake.actions())
    }

    fn opened(path: &Path) -> Vec<String> {
        vec![format!("open_path {}", path.display())]
    }

    /// "Open my Downloads folder" opens the folder, which the walk never
    /// lists; a file named with the folder in it is still searched for.
    #[test]
    fn a_known_folder_said_by_name_is_opened_itself() {
        let folders = vec![
            PathBuf::from(r"C:\Users\u\Documents"),
            PathBuf::from(r"C:\Users\u\Downloads"),
        ];
        for spoken in ["Downloads", "my downloads folder", "the download folder"] {
            assert_eq!(
                known_folder_named(spoken, &folders),
                Some(folders[1].clone()),
                "{spoken}"
            );
        }
        assert_eq!(known_folder_named("budget in downloads", &folders), None);
        assert_eq!(known_folder_named("resume", &folders), None);

        let (outcome, actions) = run(vec![], folders.clone(), json!({"name": "my documents"}));
        assert_eq!(actions, opened(&folders[0]));
        assert!(matches!(outcome, ToolOutcome::Done { .. }));
    }

    #[test]
    fn a_unique_name_in_a_known_folder_is_opened() {
        let home = tempfile::tempdir().unwrap();
        let resume = touch(home.path(), "Documents/Resume.pdf");
        touch(home.path(), "Documents/Budget 2026.xlsx");
        let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": "my resume"}));
        assert_eq!(actions, opened(&resume));
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.key, "commandMode.feedback.openingFile");
        assert_eq!(feedback.render_en(), "Opening Resume.pdf.");
    }

    /// Two files of one name in different folders are a question whose
    /// choices say which folder each is in.
    #[test]
    fn two_files_of_one_name_are_a_question_naming_their_folders() {
        let home = tempfile::tempdir().unwrap();
        touch(home.path(), "Documents/Resume.pdf");
        touch(home.path(), "Downloads/Resume.pdf");
        let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": "resume"}));
        assert!(actions.is_empty());
        let ToolOutcome::AskUser { question, mut choices } = outcome else {
            panic!("expected a question, got {outcome:?}");
        };
        assert_eq!(question, "Which file?");
        choices.sort();
        assert_eq!(
            choices,
            vec![
                "Resume.pdf (Documents)".to_string(),
                "Resume.pdf (Downloads)".to_string()
            ]
        );
    }

    /// Echoing one of the question's choices back settles it.
    #[test]
    fn answering_with_a_choice_opens_that_file() {
        let home = tempfile::tempdir().unwrap();
        touch(home.path(), "Documents/Resume.pdf");
        let wanted = touch(home.path(), "Downloads/Resume.pdf");
        let (_, actions) = run(
            vec![],
            vec![home.path().into()],
            json!({"name": "Resume.pdf (Downloads)"}),
        );
        assert_eq!(actions, opened(&wanted));
    }

    #[test]
    fn nothing_close_fails_without_escalating() {
        let home = tempfile::tempdir().unwrap();
        touch(home.path(), "Documents/Budget.xlsx");
        let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": "tax return"}));
        assert!(actions.is_empty());
        assert_eq!(
            outcome,
            ToolOutcome::failed("I couldn't find a file called tax return.", false)
        );
    }

    /// A recent file wins a tie with one the walk found.
    #[test]
    fn a_recent_file_beats_an_equally_good_old_one() {
        let home = tempfile::tempdir().unwrap();
        // "resume" is a partial match for both (0.95), so the walk runs and
        // the tie is real.
        touch(home.path(), "Archive/Resume final.docx");
        let recent = touch(home.path(), "Work/Resume final.docx");
        let (outcome, actions) = run(
            vec![recent.clone()],
            vec![home.path().into()],
            json!({"name": "resume"}),
        );
        assert_eq!(actions, opened(&recent), "{outcome:?}");
    }

    /// A Windows Recent item is a shortcut named after its target. It is found
    /// by the target's name and kind, and the shortcut is what gets opened.
    #[test]
    fn a_recent_shortcut_is_known_by_its_target() {
        let home = tempfile::tempdir().unwrap();
        let shortcut = touch(home.path(), "Recent/Q3 budget.xlsx.lnk");
        let (outcome, actions) = run(
            vec![shortcut.clone()],
            vec![],
            json!({"name": "Q3 budget", "kind": "spreadsheet"}),
        );
        assert_eq!(actions, opened(&shortcut), "{outcome:?}");
        let ToolOutcome::Done { feedback } = outcome else {
            panic!("expected done, got {outcome:?}");
        };
        assert_eq!(feedback.render_en(), "Opening Q3 budget.xlsx.");
    }

    /// Two recent files are no tie-breaker for each other.
    #[test]
    fn two_equally_good_recent_files_are_still_a_question() {
        let home = tempfile::tempdir().unwrap();
        let a = touch(home.path(), "A/Notes.txt");
        let b = touch(home.path(), "B/Notes.txt");
        let (outcome, _) = run(vec![a, b], vec![], json!({"name": "notes"}));
        assert!(matches!(outcome, ToolOutcome::AskUser { .. }), "{outcome:?}");
    }

    /// A recent entry whose file is gone is not offered.
    #[test]
    fn a_deleted_recent_file_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        let (outcome, actions) = run(
            vec![home.path().join("Gone.pdf")],
            vec![],
            json!({"name": "gone"}),
        );
        assert!(actions.is_empty());
        assert!(matches!(outcome, ToolOutcome::Failed { .. }));
    }

    /// "The budget spreadsheet" must not open the budget PDF.
    #[test]
    fn the_kind_filters_by_extension() {
        let home = tempfile::tempdir().unwrap();
        touch(home.path(), "Budget.pdf");
        let sheet = touch(home.path(), "Budget.xlsx");
        let (outcome, _) = run(vec![], vec![home.path().into()], json!({"name": "budget"}));
        assert!(matches!(outcome, ToolOutcome::AskUser { .. }), "{outcome:?}");
        let (_, actions) = run(
            vec![],
            vec![home.path().into()],
            json!({"name": "budget", "kind": "spreadsheet"}),
        );
        assert_eq!(actions, opened(&sheet));
        let (outcome, _) = run(
            vec![],
            vec![home.path().into()],
            json!({"name": "budget", "kind": "video"}),
        );
        assert!(matches!(outcome, ToolOutcome::Failed { .. }));
    }

    #[test]
    fn kinds_match_extensions_case_insensitively() {
        assert!(Kind::Image.matches(Path::new("Photo.JPG")));
        assert!(Kind::Pdf.matches(Path::new("a/b.pdf")));
        assert!(!Kind::Pdf.matches(Path::new("noextension")));
        assert!(Kind::Any.matches(Path::new("noextension")));
        assert_eq!(Kind::parse(Some("Spreadsheet")), Kind::Spreadsheet);
        assert_eq!(Kind::parse(None), Kind::Any);
    }

    #[test]
    fn the_walk_stops_at_the_depth_limit() {
        let root = tempfile::tempdir().unwrap();
        let shallow = touch(root.path(), "top.txt");
        let three = touch(root.path(), "a/b/c/three.txt");
        let four = touch(root.path(), "a/b/c/d/four.txt");
        let found = walk_files(&[root.path().into()], 3, 10_000);
        assert!(found.contains(&shallow));
        assert!(found.contains(&three));
        assert!(!found.contains(&four));
        let found = walk_files(&[root.path().into()], 0, 10_000);
        assert_eq!(found, vec![shallow]);
    }

    /// The budget counts every entry read, folders included, across roots.
    #[test]
    fn the_walk_stops_at_the_entry_limit() {
        let root = tempfile::tempdir().unwrap();
        for i in 0..50 {
            touch(root.path(), &format!("f{i:02}.txt"));
        }
        let other = tempfile::tempdir().unwrap();
        touch(other.path(), "late.txt");
        let found = walk_files(&[root.path().into(), other.path().into()], 3, 10);
        assert_eq!(found.len(), 10);
        assert!(!found.iter().any(|p| p.ends_with("late.txt")));
        let found = walk_files(&[root.path().into(), other.path().into()], 3, 51);
        assert_eq!(found.len(), 51);
    }

    #[test]
    fn the_walk_skips_hidden_dependency_and_lock_entries() {
        let root = tempfile::tempdir().unwrap();
        let kept = touch(root.path(), "Projects/plan.md");
        touch(root.path(), ".git/plan.md");
        touch(root.path(), ".config/plan.md");
        touch(root.path(), "Projects/node_modules/plan.md");
        touch(root.path(), "AppData/Roaming/plan.md");
        touch(root.path(), ".hidden-plan.md");
        touch(root.path(), "Projects/~$plan.docx");
        let found = walk_files(&[root.path().into()], 3, 10_000);
        assert_eq!(found, vec![kept]);
    }

    /// Home and home/Documents both being roots must not list a file twice.
    #[test]
    fn overlapping_roots_list_each_file_once() {
        let home = tempfile::tempdir().unwrap();
        let resume = touch(home.path(), "Documents/Resume.pdf");
        let (_, actions) = run(
            vec![],
            vec![home.path().into(), home.path().join("Documents")],
            json!({"name": "resume"}),
        );
        assert_eq!(actions, opened(&resume));
        // The same file in the recent list and the walk is one candidate too.
        let (_, actions) = run(
            vec![resume.clone()],
            vec![home.path().into()],
            json!({"name": "resume"}),
        );
        assert_eq!(actions, opened(&resume));
    }

    /// A symlinked folder can loop back on itself or lead to another disk.
    #[cfg(unix)]
    #[test]
    fn the_walk_never_enters_a_symlinked_folder() {
        let root = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        touch(elsewhere.path(), "secret.txt");
        let target = touch(elsewhere.path(), "linked.txt");
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("link")).unwrap();
        std::os::unix::fs::symlink(&target, root.path().join("file-link.txt")).unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("loop")).unwrap();
        let found = walk_files(&[root.path().into()], 3, 10_000);
        assert_eq!(found, vec![root.path().join("file-link.txt")]);
    }

    /// "Open the invoice" with `invoice.pdf.exe` in Downloads: the double
    /// extension does not make it a document, and nothing runs. A real
    /// document beside it is still opened.
    #[test]
    fn programs_and_scripts_are_never_opened() {
        let home = tempfile::tempdir().unwrap();
        touch(home.path(), "Downloads/invoice.pdf.exe");
        touch(home.path(), "Downloads/invoice.js");
        touch(home.path(), "Desktop/setup.exe");
        touch(home.path(), "Desktop/setup.MSI");
        touch(home.path(), "Downloads/run me.command");
        for name in ["invoice", "invoice.pdf", "setup", "run me"] {
            let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": name}));
            assert!(actions.is_empty(), "{name}: {actions:?}");
            assert_eq!(outcome, ToolOutcome::failed(PROGRAM_REFUSAL, false), "{name}");
        }

        let pdf = touch(home.path(), "Documents/invoice.pdf");
        let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": "invoice"}));
        assert_eq!(actions, opened(&pdf), "{outcome:?}");
    }

    /// A Recent shortcut to a program is a program; one to a document is a
    /// document, whatever its target's extension says it is not.
    #[test]
    fn a_shortcut_is_judged_by_its_target() {
        assert!(is_program(Path::new("Recent/setup.exe.lnk")));
        assert!(is_program(Path::new("tool.PS1")));
        assert!(is_program(Path::new("Foo.app")));
        assert!(!is_program(Path::new("Recent/Q3 budget.xlsx.lnk")));
        assert!(!is_program(Path::new("Resume.pdf")));
        assert!(!is_program(Path::new("Recent/Some folder.lnk")));
        let home = tempfile::tempdir().unwrap();
        let shortcut = touch(home.path(), "Recent/setup.exe.lnk");
        let (outcome, actions) = run(vec![shortcut], vec![], json!({"name": "setup"}));
        assert!(actions.is_empty(), "{actions:?}");
        assert_eq!(outcome, ToolOutcome::failed(PROGRAM_REFUSAL, false));
    }

    /// An app bundle is a folder of executables, never walked into.
    #[test]
    fn the_walk_does_not_enter_app_bundles() {
        let root = tempfile::tempdir().unwrap();
        let kept = touch(root.path(), "notes.txt");
        touch(root.path(), "Tool.app/Contents/Info.plist");
        let found = walk_files(&[root.path().into()], 3, 10_000);
        assert_eq!(found, vec![kept]);
    }

    /// An extensionless file with an execute bit runs when opened on Unix.
    #[cfg(unix)]
    #[test]
    fn an_executable_without_an_extension_is_a_program() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let script = touch(root.path(), "invoice");
        assert!(!is_program(&script));
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_program(&script));
    }

    /// The folder a file is in is not part of its name: "taxes" does not pick
    /// the one file in a folder called Taxes, but echoing that file's choice
    /// label still does.
    #[test]
    fn a_folder_name_alone_does_not_match_the_files_in_it() {
        let home = tempfile::tempdir().unwrap();
        let ret = touch(home.path(), "Taxes/2024 return.pdf");
        let (outcome, actions) = run(vec![], vec![home.path().into()], json!({"name": "taxes"}));
        assert!(actions.is_empty(), "{actions:?}");
        assert!(matches!(outcome, ToolOutcome::Failed { .. }), "{outcome:?}");

        let (_, actions) = run(
            vec![],
            vec![home.path().into()],
            json!({"name": "2024 return.pdf (Taxes)"}),
        );
        assert_eq!(actions, opened(&ret));
    }

    #[test]
    fn filler_words_around_a_file_name_are_dropped() {
        assert_eq!(clean_file_query("my resume"), "resume");
        assert_eq!(clean_file_query("the budget file"), "budget");
        assert_eq!(clean_file_query("my"), "my");
    }

    #[test]
    fn the_schema_accepts_a_name_and_a_known_kind_only() {
        assert!(validate_args(&SPEC, &json!({"name": "resume"})).is_ok());
        assert!(validate_args(&SPEC, &json!({"name": "resume", "kind": "pdf"})).is_ok());
        assert!(validate_args(&SPEC, &json!({"name": "resume", "kind": "folder"})).is_err());
        assert!(validate_args(&SPEC, &json!({"kind": "pdf"})).is_err());
    }
}
