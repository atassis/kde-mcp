use kde_mcp::keymap::{KEY_ENTER, KEY_TAB, keycode_for_char, keycode_for_named_key, shift_required};

#[test]
fn returns_evdev_codes_for_common_ascii_letters() {
    assert_eq!(keycode_for_char('a'), Some(30));
    assert_eq!(keycode_for_char('z'), Some(44));
    assert_eq!(keycode_for_char('m'), Some(50));
    assert_eq!(keycode_for_char('A'), Some(30));
}

#[test]
fn returns_evdev_codes_for_digits() {
    assert_eq!(keycode_for_char('0'), Some(11));
    assert_eq!(keycode_for_char('1'), Some(2));
    assert_eq!(keycode_for_char('9'), Some(10));
}

#[test]
fn returns_none_for_non_ascii_chars() {
    assert_eq!(keycode_for_char('é'), None);
    assert_eq!(keycode_for_char('字'), None);
    assert_eq!(keycode_for_char('!'), None);
}

#[test]
fn whitespace_and_special_chars() {
    assert_eq!(keycode_for_char(' '), Some(57));
    assert_eq!(keycode_for_char('\n'), Some(KEY_ENTER));
    assert_eq!(keycode_for_char('\t'), Some(KEY_TAB));
}

#[test]
fn shift_required_matches_capital_ascii_letters() {
    assert!(shift_required('A'));
    assert!(shift_required('Z'));
    assert!(!shift_required('a'));
    assert!(!shift_required('1'));
    assert!(!shift_required(' '));
}

#[test]
fn named_keys_resolve() {
    assert_eq!(keycode_for_named_key("Return"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("Tab"), Some(KEY_TAB));
    assert_eq!(keycode_for_named_key("Escape"), Some(1));
    assert_eq!(keycode_for_named_key("Page_Down"), Some(109));
    assert_eq!(keycode_for_named_key("F5"), Some(63));
}

#[test]
fn named_key_lookup_is_case_insensitive() {
    assert_eq!(keycode_for_named_key("RETURN"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("return"), Some(KEY_ENTER));
    assert_eq!(keycode_for_named_key("Return"), Some(KEY_ENTER));
}
