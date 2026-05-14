// Single test-binary wrapper for all Phase 0 integration tests.
// Each module file under tests/integration/<name>.rs becomes a `mod` entry
// here. Cargo compiles this whole tree into one binary, which keeps build
// times down vs. one binary per file.

mod integration {
    pub mod cli_parse;
    pub mod lockfile;
}
