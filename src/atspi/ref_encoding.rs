//! Encode AT-SPI `(bus_name, object_path)` into the RefStore's payload string
//! and back. Format: `"<bus_name>|<object_path>"`.
//!
//! D-Bus bus names and object paths share the constraint that they cannot
//! contain `|` (the spec restricts both to a subset of ASCII; bus names use
//! `[A-Za-z0-9_-]` with `.` and `:` separators, object paths use `/` as a
//! separator and `[A-Za-z0-9_]` as path elements). Splitting on `|` is
//! therefore unambiguous.

pub fn encode_element_ref(bus_name: &str, object_path: &str) -> String {
    format!("{bus_name}|{object_path}")
}

pub fn decode_element_ref(payload: &str) -> Option<(&str, &str)> {
    let mut parts = payload.splitn(2, '|');
    let bus = parts.next()?;
    let path = parts.next()?;
    if bus.is_empty() || path.is_empty() {
        return None;
    }
    Some((bus, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_simple() {
        let s = encode_element_ref(":1.42", "/org/a11y/atspi/accessible/123");
        let (b, p) = decode_element_ref(&s).unwrap();
        assert_eq!(b, ":1.42");
        assert_eq!(p, "/org/a11y/atspi/accessible/123");
    }

    #[test]
    fn round_trip_well_known_name() {
        let s = encode_element_ref("org.kde.kate", "/org/a11y/atspi/accessible/root");
        let (b, p) = decode_element_ref(&s).unwrap();
        assert_eq!(b, "org.kde.kate");
        assert_eq!(p, "/org/a11y/atspi/accessible/root");
    }

    #[test]
    fn empty_inputs_decode_to_none() {
        assert_eq!(decode_element_ref(""), None);
        assert_eq!(decode_element_ref("|"), None);
        assert_eq!(decode_element_ref(":1.42|"), None);
        assert_eq!(decode_element_ref("|/some/path"), None);
    }

    #[test]
    fn no_separator_decodes_to_none() {
        assert_eq!(decode_element_ref("nopipe"), None);
    }

    #[test]
    fn path_containing_no_extra_pipes_is_returned_whole() {
        // Object paths cannot contain `|` per D-Bus spec, but the encoding
        // would still recover the first separator correctly if one did.
        let s = ":1.42|/a/b/c";
        let (b, p) = decode_element_ref(s).unwrap();
        assert_eq!(b, ":1.42");
        assert_eq!(p, "/a/b/c");
    }
}
