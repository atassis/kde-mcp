use kde_mcp::keymap::{Modifier, parse_chord};

#[test]
fn single_letter() {
    let c = parse_chord("s").unwrap();
    assert_eq!(c.modifiers, vec![]);
    assert_eq!(c.key, 31);
}

#[test]
fn one_modifier_plus_key() {
    let c = parse_chord("ctrl+s").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl]);
    assert_eq!(c.key, 31);
}

#[test]
fn two_modifiers() {
    let c = parse_chord("ctrl+shift+t").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
    assert_eq!(c.key, 20);
}

#[test]
fn named_key_with_modifier() {
    let c = parse_chord("alt+Return").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Alt]);
    assert_eq!(c.key, 28);
}

#[test]
fn super_alias_meta() {
    let c = parse_chord("super+l").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Super]);
    let c2 = parse_chord("meta+l").unwrap();
    assert_eq!(c2.modifiers, vec![Modifier::Super]);
    let c3 = parse_chord("win+l").unwrap();
    assert_eq!(c3.modifiers, vec![Modifier::Super]);
}

#[test]
fn whitespace_around_segments_is_tolerated() {
    let c = parse_chord(" ctrl + shift + t ").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
}

#[test]
fn case_insensitive_modifiers() {
    let c = parse_chord("CTRL+SHIFT+T").unwrap();
    assert_eq!(c.modifiers, vec![Modifier::Ctrl, Modifier::Shift]);
}

#[test]
fn duplicate_modifier_errors() {
    assert!(parse_chord("ctrl+ctrl+s").is_err());
}

#[test]
fn unknown_modifier_errors() {
    let err = parse_chord("hyper+s").unwrap_err();
    assert_eq!(err.code(), "unsupported_region");
}

#[test]
fn unknown_key_errors() {
    let err = parse_chord("ctrl+notakey").unwrap_err();
    assert_eq!(err.code(), "unsupported_region");
}

#[test]
fn empty_chord_errors() {
    assert!(parse_chord("").is_err());
    assert!(parse_chord("   ").is_err());
    assert!(parse_chord("ctrl++s").is_err());
}
