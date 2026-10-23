//! Command-mode tools, one per file in this directory.
//!
//! Each file exports `pub static SPEC: ToolSpec`; `build.rs`
//! (`generate_command_tools`) declares every file here as a module and lists
//! its `SPEC` in [`ALL`], sorted by file name. To add a tool, add a file.
//! Shared helpers live outside this directory (`fuzzy.rs`, `registry.rs`), so
//! nothing here is mistaken for a tool.

include!(concat!(env!("OUT_DIR"), "/command_tools.rs"));
