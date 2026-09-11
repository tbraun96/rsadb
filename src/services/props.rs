//! `getprop` parsing and property-name validation.

use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Reject anything that is not a plain Android property name.
///
/// Property names reach the device inside a shell command line, so this is
/// the only thing standing between a caller's string and `/system/bin/sh`.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 255
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b':' | b'@'));
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidArgument(format!(
            "invalid property name {name:?}"
        )))
    }
}

/// Parse the `[name]: [value]` lines that `getprop` prints.
///
/// Values may span lines; a value ends at the first line that ends in `]`.
pub fn parse_all(output: &str) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let mut pending: Option<(String, String)> = None;
    for line in output.lines() {
        let line = line.trim_end_matches('\r');
        if let Some((name, mut value)) = pending.take() {
            value.push('\n');
            value.push_str(line);
            if let Some(v) = value.strip_suffix(']') {
                props.insert(name, v.to_owned());
            } else {
                pending = Some((name, value));
            }
            continue;
        }
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((name, value)) = rest.split_once("]: [") else {
            continue;
        };
        if let Some(v) = value.strip_suffix(']') {
            props.insert(name.to_owned(), v.to_owned());
        } else {
            pending = Some((name.to_owned(), value.to_owned()));
        }
    }
    props
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_including_multiline_values() {
        let out = "[ro.product.model]: [Pixel 7]\n[persist.sys.multi]: [line one\nline two]\n[empty]: []\n";
        let props = parse_all(out);
        assert_eq!(props["ro.product.model"], "Pixel 7");
        assert_eq!(props["persist.sys.multi"], "line one\nline two");
        assert_eq!(props["empty"], "");
    }

    #[test]
    fn name_validation() {
        assert!(validate_name("ro.build.version.sdk").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("ro.x; rm -rf /").is_err());
        assert!(validate_name("a b").is_err());
    }
}
