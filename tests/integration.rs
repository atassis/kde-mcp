// Single test-binary wrapper for all Phase 0 integration tests.
// Each module file under tests/integration/<name>.rs becomes a `mod` entry
// here. Cargo compiles this whole tree into one binary, which keeps build
// times down vs. one binary per file.

mod integration {
    pub mod chord_parse;
    pub mod cli_parse;
    pub mod clipboard_offline;
    pub mod keymap;
    pub mod lease;
    pub mod lockfile;
    pub mod observe_offline;
    pub mod text_editor;
    pub mod tools_list;
}
