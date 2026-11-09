//! Theme validation for the Rust runtime.
//!
//! Covers the Valenced Themes API (`valenced.settings.themes` + `api.themes`)
//! and your `discord-themes` JSON files. Two flavours exist upstream:
//! - **Modern (Valenced Themes API)**: `semanticColors` + `plus` (+ fonts/icons)
//! - **Legacy**: imported via `plugins/themes.legacy`.
//!
//! This module validates the modern shape and can export the `semanticColors`
//! map the Hermes bundle would consume. The GPUI desktop app renders the same
//! CSS variables; the mobile bundle maps them via the existing `api.themes`
//! provider plugin — no change needed there.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A theme author entry, as in `discord-themes/*.json`:
/// `{ "name": "...", "id": "..." }`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeAuthor {
    pub name: String,
    #[serde(default)]
    pub id: String,
}

/// Legacy `plus` block: icon pack + format version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePlus {
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub iconpack: String,
}

/// Modern theme document (superset tolerated — unknown keys ignored, like
/// `ValencedJson`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub authors: Vec<ThemeAuthor>,
    #[serde(default)]
    pub plus: Option<ThemePlus>,
    /// Discord semantic colors: `"CHAT_INPUT_BACKGROUND": ["#000000"]`.
    /// Each value is a list of color strings (dark first, light second).
    #[serde(default, rename = "semanticColors")]
    pub semantic_colors: HashMap<String, Vec<String>>,
    /// Optional font pack reference.
    #[serde(default)]
    pub font: Option<String>,
}

/// Parse + validate theme JSON.
pub fn parse_theme(json: &str) -> Result<Theme, String> {
    let theme: Theme = serde_json::from_str(json).map_err(|e| format!("Unable to parse theme JSON: {e}"))?;
    theme.validate()?;
    Ok(theme)
}

impl Theme {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Theme name must not be empty".into());
        }
        for (key, colors) in &self.semantic_colors {
            if key.trim().is_empty() {
                return Err("Theme semanticColors key must not be empty".into());
            }
            if colors.is_empty() {
                return Err(format!("Theme color '{key}' must have at least one value"));
            }
            for color in colors {
                if !is_valid_color(color) {
                    return Err(format!("Theme color '{key}' has invalid color '{color}' (expected #RRGGBB or #RRGGBBAA)"));
                }
            }
        }
        Ok(())
    }

    /// CSS custom properties the GPUI preview renders:
    /// `--<lowercase-key>: <dark-value>;`
    pub fn css_variables(&self) -> String {
        let mut lines: Vec<String> = self.semantic_colors.iter()
            .filter_map(|(k, v)| v.first().map(|c| format!("  --{}: {};", k.to_lowercase(), c)))
            .collect();
        lines.sort();
        format!(":root {{\n{}\n}}", lines.join("\n"))
    }

    /// Field report for `valenced theme report <file.json>`.
    pub fn report(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "version": self.version,
            "authors": self.authors,
            "colors": self.semantic_colors.len(),
            "hasIconPack": self.plus.as_ref().is_some_and(|p| !p.iconpack.is_empty()),
            "cssBytes": self.css_variables().len(),
        })
    }
}

fn is_valid_color(s: &str) -> bool {
    if let Some(hex) = s.strip_prefix('#') {
        (hex.len() == 6 || hex.len() == 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_azure_amoled_shape() {
        let json = r##"{
            "name": "Azure Blue AMOLED",
            "description": "AMOLED black base",
            "version": "1",
            "authors": [{ "name": "im__turbo__", "id": "288054683161853952" }],
            "plus": { "version": 0, "iconpack": "rosiecord-plumpy" },
            "semanticColors": {
                "CHAT_INPUT_BACKGROUND": ["#000000"],
                "TEXT_STRONG": ["#FFFFFF"],
                "BACKGROUND_ACCENT": ["#08080830"]
            }
        }"##;
        let theme = parse_theme(json).unwrap();
        assert_eq!(theme.semantic_colors.len(), 3);
        assert!(theme.css_variables().contains("--chat_input_background: #000000;"));
    }

    #[test]
    fn rejects_bad_colors() {
        let json = r#"{"name": "Bad", "semanticColors": {"X": ["red"]}}"#;
        assert!(parse_theme(json).is_err());
        let json2 = r#"{"name": "Bad", "semanticColors": {"X": []}}"#;
        assert!(parse_theme(json2).is_err());
    }

    #[test]
    fn empty_name_fails() {
        assert!(parse_theme(r#"{"name": "  ", "semanticColors": {}}"#).is_err());
    }
}
