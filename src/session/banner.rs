//! The `A_CNXN` identity string ("banner").
//!
//! Format: `<kind>::<key>=<value>;<key>=<value>;…` where `kind` is `device`,
//! `host`, `bootloader`, `recovery`, or `sideload`. The `features` key holds a
//! comma-separated list.

use std::collections::BTreeMap;

/// Features this crate announces in its own banner.
pub const HOST_FEATURES: &str =
    "shell_v2,cmd,stat_v2,ls_v2,apex,abb,abb_exec,fixed_push_mkdir,fixed_push_symlink_timestamp";

/// A parsed `A_CNXN` banner.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Banner {
    /// `device`, `host`, `recovery`, …
    pub kind: String,
    /// Every `key=value` pair except `features`.
    pub properties: BTreeMap<String, String>,
    /// The advertised feature list.
    pub features: Vec<String>,
}

impl Banner {
    /// Parse a banner; unknown shapes yield an empty banner rather than an error.
    pub fn parse(text: &str) -> Self {
        let text = text.trim_end_matches('\0');
        let (kind, rest) = text.split_once("::").unwrap_or((text, ""));
        let mut banner = Self {
            kind: kind.to_owned(),
            ..Self::default()
        };
        for pair in rest.split(';').filter(|p| !p.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            if key == "features" {
                banner.features = value
                    .split(',')
                    .filter(|f| !f.is_empty())
                    .map(str::to_owned)
                    .collect();
            } else {
                banner.properties.insert(key.to_owned(), value.to_owned());
            }
        }
        banner
    }

    /// The banner this crate sends.
    pub fn host() -> String {
        format!("host::features={HOST_FEATURES}")
    }

    /// Whether the peer advertised `feature`.
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }

    /// Serialise back to the wire form (used by device emulators and tests).
    pub fn to_wire(&self) -> String {
        let props: Vec<String> = self
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v};"))
            .collect();
        let features = if self.features.is_empty() {
            String::new()
        } else {
            format!("features={}", self.features.join(","))
        };
        format!("{}::{}{features}", self.kind, props.concat())
    }
}

#[cfg(test)]
mod tests {
    use super::Banner;

    #[test]
    fn parses_device_banner() {
        let b = Banner::parse(
            "device::ro.product.name=sdk_gphone64;ro.product.model=Pixel;features=shell_v2,cmd\0",
        );
        assert_eq!(b.kind, "device");
        assert_eq!(
            b.properties.get("ro.product.model").map(String::as_str),
            Some("Pixel")
        );
        assert_eq!(b.features, vec!["shell_v2", "cmd"]);
        assert!(b.has_feature("cmd"));
        assert!(!b.has_feature("abb"));
        assert_eq!(Banner::parse(&b.to_wire()), b);
    }

    #[test]
    fn tolerates_minimal_banner() {
        let b = Banner::parse("device::");
        assert_eq!(b.kind, "device");
        assert!(b.features.is_empty());
        assert_eq!(Banner::parse("").kind, "");
    }
}
