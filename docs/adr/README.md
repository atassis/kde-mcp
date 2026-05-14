# Architecture Decision Records

Each ADR captures one non-obvious decision at the moment it was made, with the reasoning frozen. Useful for: catching myself revisiting a settled question, onboarding contributors, looking back to ask "did the assumption that justified this still hold?"

Format: 1-2 pages each. Sections: Status · Context · Decision · Consequences · Alternatives considered · Date.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| 0001 | Use Rust (not Python, not TypeScript) | Accepted |
| 0002 | Scope: KDE Plasma 6 / Wayland only | Accepted |
| 0003 | Mode per process; no runtime swap | Accepted |
| 0004 | Response-as-instruction over prompt-prose | Accepted |
| 0005 | Accessibility-tree first; pixels as fallback | Accepted |
| 0006 | 15 tools split read/write; no Anthropic-style omni-tool | Accepted |
| 0007 | Adopt Anthropic computer-use safety architecture (policy gate) | Accepted |
| 0008 | Visual prompt injection defence is host-side, not server-side  | Accepted |
| 0009 | Phase 3–6 implementation shape (per-call EIS, SIGUSR1 cancel, …) | Accepted (§2 superseded by 0010) |
| 0010 | Cancel UX: system tray + Meta+Esc hotkey + dispatch hooks | Accepted |

## When to add an ADR

When you make a choice that has a clear alternative someone might reasonably propose later. Not for every implementation detail — for choices about the shape of the system.
