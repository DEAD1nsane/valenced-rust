//! Port of `core/.../plugins/Version.kt` (Kotlin Multiplatform).
//!
//! Format: `<n>[.<n>]*(-<label>)?`. Numeric segments are non-negative, leading
//! zeros allowed and insignificant (`1.02 == 1.2`).
//!
//! Ordering:
//! 1. Numeric segments split by `.`; the shorter side is right-padded with zeros
//!    (`1.2 == 1.2.0`).
//! 2. If numeric parts are equal: bare > labeled (`1.2.0 > 1.2.0-rc`). Every
//!    label is a prerelease.
//! 3. If both labeled, digit-run comparison: split labels into alternating
//!    non-digit/digit runs; non-digit runs compare byte-lexically, digit runs
//!    numerically (`beta2 < beta10`). A label that is a run-prefix of another
//!    sorts first (`beta < beta1`).

use std::cmp::Ordering;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A Valenced version.
/// Wire format matches the TS bundle's `PluginVersion { nums, label? }`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Version {
    pub nums: Vec<u64>,
    pub label: Option<String>,
}

impl Version {
    pub fn new(nums: Vec<u64>, label: Option<String>) -> Result<Self, String> {
        if nums.is_empty() {
            return Err("Version must have at least one numeric segment".into());
        }
        if let Some(l) = &label {
            if !is_valid_label(l) {
                return Err(format!("Invalid version label: '{l}'"));
            }
        }
        Ok(Self { nums, label })
    }

    /// Parse `"1.2"`, `"1.2.0-rc"`, etc.
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.is_empty() {
            return Err("Version string must not be empty".into());
        }
        let (nums_part, label) = match value.find('-') {
            Some(i) => (&value[..i], Some(value[i + 1..].to_string())),
            None => (value, None),
        };
        if let Some(ref l) = label {
            if !is_valid_label(l) {
                return Err(format!("Invalid version label: '{l}' in '{value}'"));
            }
        }
        let mut nums = Vec::new();
        for seg in nums_part.split('.') {
            if seg.is_empty() {
                return Err(format!("Invalid version numeric segment: '{seg}' in '{value}'"));
            }
            // Must be all digits.
            if !seg.chars().all(|c| c.is_ascii_digit()) {
                return Err(format!("Invalid version numeric segment: '{seg}' in '{value}'"));
            }
            let n: u64 = seg.parse().map_err(|_| format!("Invalid version numeric segment: '{seg}' in '{value}'"))?;
            nums.push(n);
        }
        Ok(Self { nums, label })
    }

    /// Strip the label, returning the bare numeric version.
    pub fn bare(&self) -> Version {
        Version { nums: self.nums.clone(), label: None }
    }

    pub fn is_bare(&self) -> bool {
        self.label.is_none()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: Vec<String> = self.nums.iter().map(|n| n.to_string()).collect();
        write!(f, "{}", s.join("."))?;
        if let Some(l) = &self.label {
            write!(f, "-{l}")?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.nums.len().max(other.nums.len());
        for i in 0..len {
            let a = self.nums.get(i).copied().unwrap_or(0);
            let b = other.nums.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        match (&self.label, &other.label) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => compare_labels(a, b),
        }
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn is_valid_label(label: &str) -> bool {
    !label.is_empty() && label.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn split_runs(label: &str) -> Vec<String> {
    let bytes: Vec<char> = label.chars().collect();
    let mut runs = Vec::new();
    let mut start = 0usize;
    for i in 1..=bytes.len() {
        if i == bytes.len() || bytes[i].is_ascii_digit() != bytes[start].is_ascii_digit() {
            runs.push(bytes[start..i].iter().collect::<String>());
            start = i;
        }
    }
    runs
}

fn compare_digit_runs(a: &str, b: &str) -> Ordering {
    let na = a.trim_start_matches('0');
    let nb = b.trim_start_matches('0');
    let na = if na.is_empty() { "0" } else { na };
    let nb = if nb.is_empty() { "0" } else { nb };
    match na.len().cmp(&nb.len()) {
        Ordering::Equal => na.cmp(nb),
        o => o,
    }
}

fn compare_labels(a: &str, b: &str) -> Ordering {
    let runs_a = split_runs(a);
    let runs_b = split_runs(b);
    let len = runs_a.len().min(runs_b.len());
    for i in 0..len {
        let ra = &runs_a[i];
        let rb = &runs_b[i];
        let both_digits = ra.chars().next().is_some_and(|c| c.is_ascii_digit())
            && rb.chars().next().is_some_and(|c| c.is_ascii_digit());
        let cmp = if both_digits {
            compare_digit_runs(ra, rb)
        } else {
            ra.cmp(rb)
        };
        if cmp != Ordering::Equal {
            return cmp;
        }
    }
    runs_a.len().cmp(&runs_b.len())
}

/// A version range: either the wildcard `"*"` or a conjunction of bounds,
/// e.g. `">=1.0 <2"`.
///
/// Satisfaction is on the **integers only** — the candidate's label is stripped
/// before bounds checking (`1.5-rc` satisfies `>=1.0 <2`; `2.0-rc` does NOT
/// satisfy `<2`). Bounds themselves must be bare (no labels).
///
/// Wire format is the range string (`"*"`, `">=1.0 <2"`), matching
/// `PluginDependencyConstraint.version` in the TS bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionRange {
    pub bounds: Vec<Bound>,
}

impl Serialize for VersionRange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for VersionRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        VersionRange::parse(&s).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub op: Op,
    pub version: Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Lt,
    Lte,
    Eq,
    Gte,
    Gt,
}

impl Op {
    pub fn symbol(&self) -> &'static str {
        match self {
            Op::Lte => "<=",
            Op::Gte => ">=",
            Op::Lt => "<",
            Op::Gt => ">",
            Op::Eq => "=",
        }
    }
}

impl VersionRange {
    /// The wildcard range (`"*"`): satisfied by every version, labels included.
    pub fn any() -> Self {
        Self { bounds: Vec::new() }
    }

    pub fn is_any(&self) -> bool {
        self.bounds.is_empty()
    }

    /// Parse `"*"` to ANY, or a whitespace-separated conjunction of bounds.
    pub fn parse(value: &str) -> Result<Self, String> {
        let trimmed = value.trim();
        if trimmed == "*" {
            return Ok(Self::any());
        }
        if trimmed.is_empty() {
            return Err("Version range must not be empty".into());
        }
        let mut bounds = Vec::new();
        for token in trimmed.split_whitespace() {
            let op = if token.starts_with("<=") {
                Op::Lte
            } else if token.starts_with(">=") {
                Op::Gte
            } else if token.starts_with('<') {
                Op::Lt
            } else if token.starts_with('>') {
                Op::Gt
            } else if token.starts_with('=') {
                Op::Eq
            } else {
                return Err(format!("Invalid version range bound: '{token}' in '{value}'"));
            };
            let version_str = token.trim_start_matches(['<', '>', '=', ' ']);
            // Recompute correctly for two-char ops.
            let version_str = if token.starts_with("<=") || token.starts_with(">=") {
                &token[2..]
            } else {
                version_str
            };
            let version = Version::parse(version_str)?;
            if version.label.is_some() {
                let _ = op;
                return Err(format!("Version range bounds must not be labeled: '{token}' in '{value}'"));
            }
            bounds.push(Bound { op, version });
        }
        Ok(Self { bounds })
    }

    /// Whether `version` satisfies every bound, compared on integers only.
    pub fn satisfies(&self, version: &Version) -> bool {
        if self.is_any() {
            return true;
        }
        let bare = Version { nums: version.nums.clone(), label: None };
        self.bounds.iter().all(|b| {
            let cmp = bare.cmp(&b.version);
            match b.op {
                Op::Lt => cmp == Ordering::Less,
                Op::Lte => cmp == Ordering::Less || cmp == Ordering::Equal,
                Op::Eq => cmp == Ordering::Equal,
                Op::Gte => cmp == Ordering::Greater || cmp == Ordering::Equal,
                Op::Gt => cmp == Ordering::Greater,
            }
        })
    }
}

impl std::fmt::Display for VersionRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_any() {
            return write!(f, "*");
        }
        let parts: Vec<String> = self.bounds.iter().map(|b| format!("{}{}", b.op.symbol(), b.version)).collect();
        write!(f, "{}", parts.join(" "))
    }
}

impl Default for VersionRange {
    fn default() -> Self {
        Self::any()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        assert_eq!(Version::parse("1.2.3").unwrap().to_string(), "1.2.3");
        assert_eq!(Version::parse("1.02").unwrap().nums, vec![1, 2]);
        assert_eq!(Version::parse("1.2.0-rc").unwrap().label.as_deref(), Some("rc"));
        assert!(Version::parse("").is_err());
        assert!(Version::parse("1.0-").is_err());
        assert!(Version::parse("1.0-RC").is_err());
        assert!(Version::parse("a.b").is_err());
    }

    #[test]
    fn ordering_numeric_padded() {
        assert_eq!(Version::parse("1.2").unwrap().cmp(&Version::parse("1.2.0").unwrap()), Ordering::Equal);
        assert!(Version::parse("1.2.0").unwrap() < Version::parse("1.10").unwrap());
        assert!(Version::parse("2").unwrap() > Version::parse("1.9.9").unwrap());
    }

    #[test]
    fn ordering_bare_beats_labeled() {
        assert!(Version::parse("1.2.0").unwrap() > Version::parse("1.2.0-rc").unwrap());
        assert!(Version::parse("1.0.0-beta2").unwrap() < Version::parse("1.0.0-beta10").unwrap());
        assert!(Version::parse("1.0.0-beta").unwrap() < Version::parse("1.0.0-beta1").unwrap());
    }

    #[test]
    fn range_wildcard() {
        let r = VersionRange::parse("*").unwrap();
        assert!(r.is_any());
        assert!(r.satisfies(&Version::parse("0.0.1-rc").unwrap()));
    }

    #[test]
    fn range_conjunction() {
        let r = VersionRange::parse(">=1.0 <2").unwrap();
        assert!(r.satisfies(&Version::parse("1.5-rc").unwrap()));
        assert!(!r.satisfies(&Version::parse("2.0-rc").unwrap()));
        assert!(r.satisfies(&Version::parse("1.0").unwrap()));
        assert!(!r.satisfies(&Version::parse("0.9").unwrap()));
    }

    #[test]
    fn range_rejects_labels_and_ops() {
        assert!(VersionRange::parse(">=1.0-rc").is_err());
        assert!(VersionRange::parse("^1.0").is_err());
        assert!(VersionRange::parse("").is_err());
    }

    #[test]
    fn range_display() {
        assert_eq!(VersionRange::parse("*").unwrap().to_string(), "*");
        assert_eq!(VersionRange::parse(">=1.0 <2").unwrap().to_string(), ">=1.0 <2");
    }
}
