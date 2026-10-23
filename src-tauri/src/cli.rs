use clap::Parser;

#[derive(Parser, Debug, Clone, Default)]
#[command(name = "zynveliq", about = "Zynveliq - Speech to Text")]
pub struct CliArgs {
    /// Start with the main window hidden
    #[arg(long)]
    pub start_hidden: bool,

    /// Disable the system tray icon
    #[arg(long)]
    pub no_tray: bool,

    /// Toggle transcription on/off (sent to running instance)
    #[arg(long)]
    pub toggle_transcription: bool,

    /// Toggle transcription with post-processing on/off (sent to running instance)
    #[arg(long)]
    pub toggle_post_process: bool,

    /// Toggle the voice assistant on/off (sent to running instance)
    #[arg(long)]
    pub toggle_assistant: bool,

    /// Start a hands-free conversation with the assistant, or end the one in
    /// progress (sent to running instance)
    #[arg(long)]
    pub toggle_call: bool,

    /// Cancel the current operation (sent to running instance)
    #[arg(long)]
    pub cancel: bool,

    /// Run a command-mode request given as text, e.g. "open Spotify" (sent to
    /// running instance, or run once the app has started)
    #[arg(long, value_name = "TEXT")]
    pub run_command: Option<String>,

    /// With --run-command: show what the command would do without doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Enable debug mode with verbose logging
    #[arg(long)]
    pub debug: bool,

    /// List the transcribe.cpp compute devices (and backend availability) then
    /// exit, without launching the app. Used to verify a packaged build's
    /// bundled ggml backend libraries load and register a device on a machine
    /// with no dev toolchain / no Vulkan SDK (the Session 7 clean-machine gate).
    #[arg(long)]
    pub list_devices: bool,

    /// Internal: enumerate the compute devices, print them as one line of JSON,
    /// and exit; the machine-readable sibling of `--list-devices`, for scripts
    /// and audits. (The app used to spawn itself with it on Linux to list GPUs
    /// out of process; the transcription worker does that on every platform
    /// now.) Hidden because it is not a user-facing switch.
    ///
    /// The transcription worker's own flag (`--transcribe-worker`) is not
    /// declared here: `main` handles it before these arguments are parsed.
    #[arg(long, hide = true)]
    pub probe_devices: bool,
}

/// A `--run-command` request found in a forwarded command line: the text,
/// and whether `--dry-run` came with it.
///
/// The single-instance callback gets the second instance's raw argv, not
/// parsed arguments, so the text has to be dug out here. clap does it when it
/// can. It cannot on Windows when the text holds a `|`: tauri-plugin-single-
/// instance joins argv with `|` to send it and splits on `|` when it arrives
/// (2.4.0, `platform_impl/windows.rs`), so "a|b" arrives as two arguments and
/// clap rejects the second as an unexpected positional. The fallback puts the
/// pieces back together, up to the next flag this binary declares.
pub(crate) fn run_command_from_argv(args: &[String]) -> Option<(String, bool)> {
    if let Ok(parsed) = CliArgs::try_parse_from(args) {
        return parsed.run_command.map(|text| (text, parsed.dry_run));
    }

    let known = known_long_flags();
    let is_flag = |arg: &str| {
        arg.strip_prefix("--")
            .map(|name| {
                known
                    .iter()
                    .any(|k| k == name.split('=').next().unwrap_or(name))
            })
            .unwrap_or(false)
    };
    // argv[0] is the executable, never an argument.
    let rest = args.get(1..).unwrap_or_default();
    let start = rest
        .iter()
        .position(|a| a == "--run-command" || a.starts_with("--run-command="))?;

    let mut parts: Vec<String> = Vec::new();
    if let Some(value) = rest[start].strip_prefix("--run-command=") {
        parts.push(value.to_string());
    }
    let mut end = start + 1;
    while end < rest.len() && !is_flag(rest[end].as_str()) {
        parts.push(rest[end].clone());
        end += 1;
    }
    if parts.is_empty() {
        return None;
    }
    let dry_run = rest.iter().any(|a| a == "--dry-run");
    Some((parts.join("|"), dry_run))
}

/// Every `--long` name `CliArgs` declares, so the fallback parser stops at a
/// real flag rather than at any text that happens to start with dashes.
fn known_long_flags() -> Vec<String> {
    use clap::CommandFactory;
    CliArgs::command()
        .get_arguments()
        .filter_map(|arg| arg.get_long().map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::CliArgs;
    use clap::Parser;

    #[test]
    fn parses_toggle_assistant() {
        let args = CliArgs::try_parse_from(["zynveliq", "--toggle-assistant"])
            .expect("--toggle-assistant should be a supported background command");
        assert!(args.toggle_assistant);
    }

    /// The conversation shortcut needs a flag of its own, for desktop shortcuts
    /// on Linux (Wayland), where the app's own keys cannot be heard.
    #[test]
    fn parses_toggle_call() {
        let args = CliArgs::try_parse_from(["zynveliq", "--toggle-call"])
            .expect("--toggle-call should be a supported background command");
        assert!(args.toggle_call);
    }

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Both spellings clap accepts reach the command, with or without a dry run.
    #[test]
    fn parses_run_command_and_dry_run() {
        let a = CliArgs::try_parse_from([
            "zynveliq",
            "--run-command",
            "send an email to Ron",
            "--dry-run",
        ])
        .expect("--run-command should take a value");
        assert_eq!(a.run_command.as_deref(), Some("send an email to Ron"));
        assert!(a.dry_run);

        let b = CliArgs::try_parse_from(["zynveliq", "--run-command=open Spotify"])
            .expect("--run-command=TEXT should parse");
        assert_eq!(b.run_command.as_deref(), Some("open Spotify"));
        assert!(!b.dry_run);
    }

    /// The plain case of a forwarded command line goes through clap.
    #[test]
    fn a_forwarded_run_command_is_found() {
        let found = super::run_command_from_argv(&argv(&[
            r"C:\zynveliq.exe",
            "--run-command",
            "open Spotify",
            "--dry-run",
        ]));
        assert_eq!(found, Some(("open Spotify".to_string(), true)));
    }

    /// Windows splits a forwarded argv on `|`, so a command holding one
    /// arrives in pieces; it is put back together, and the flag after it is
    /// still read as a flag.
    #[test]
    fn a_command_split_on_a_pipe_is_rejoined() {
        let found = super::run_command_from_argv(&argv(&[
            "zynveliq.exe",
            "--run-command",
            "type a",
            "b",
            "--dry-run",
        ]));
        assert_eq!(found, Some(("type a|b".to_string(), true)));

        let equals =
            super::run_command_from_argv(&argv(&["zynveliq.exe", "--run-command=type a", "b"]));
        assert_eq!(equals, Some(("type a|b".to_string(), false)));
    }

    /// A forwarded command line without the flag, or with the flag but no
    /// text, is not a command; the other remote-control flags handle it.
    #[test]
    fn no_run_command_means_none() {
        assert_eq!(
            super::run_command_from_argv(&argv(&["zynveliq.exe", "--toggle-call"])),
            None
        );
        assert_eq!(
            super::run_command_from_argv(&argv(&["zynveliq.exe", "--run-command"])),
            None
        );
        assert_eq!(
            super::run_command_from_argv(&argv(&["zynveliq.exe", "--run-command", "--dry-run"])),
            None
        );
    }
}
