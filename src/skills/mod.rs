//! Skills system for SKILL.md prompt extensions.

pub mod attenuation;
pub mod parser;
pub mod registry;
pub mod selector;

pub use attenuation::attenuate_tools;
pub use registry::SkillRegistry;
pub use selector::prefilter_skills;

use std::path::PathBuf;

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

const MAX_KEYWORDS_PER_SKILL: usize = 20;
const MAX_PATTERNS_PER_SKILL: usize = 5;
const MAX_TAGS_PER_SKILL: usize = 10;
const MIN_KEYWORD_TAG_LENGTH: usize = 3;

/// Maximum file size for SKILL.md (64 KiB).
pub const MAX_PROMPT_FILE_SIZE: u64 = 64 * 1024;

static SKILL_NAME_PATTERN: std::sync::LazyLock<Regex> =
    std::sync::LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9._-]{0,63}$").unwrap());

pub fn validate_skill_name(name: &str) -> bool {
    SKILL_NAME_PATTERN.is_match(name)
}

/// Trust state for a skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTrust {
    Installed = 0,
    Trusted = 1,
}

impl std::fmt::Display for SkillTrust {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Installed => write!(f, "installed"),
            Self::Trusted => write!(f, "trusted"),
        }
    }
}

/// Where a skill was loaded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    User(PathBuf),
}

/// Activation criteria from SKILL.md frontmatter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActivationCriteria {
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
}

impl ActivationCriteria {
    pub fn enforce_limits(&mut self) {
        self.keywords.retain(|k| k.len() >= MIN_KEYWORD_TAG_LENGTH);
        self.keywords.truncate(MAX_KEYWORDS_PER_SKILL);
        self.patterns.truncate(MAX_PATTERNS_PER_SKILL);
        self.tags.retain(|t| t.len() >= MIN_KEYWORD_TAG_LENGTH);
        self.tags.truncate(MAX_TAGS_PER_SKILL);
    }
}

fn default_max_context_tokens() -> usize {
    2000
}

/// Parsed skill manifest from SKILL.md YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub activation: ActivationCriteria,
}

fn default_version() -> String {
    "0.0.0".to_string()
}

/// A fully loaded skill ready for activation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct LoadedSkill {
    pub manifest: SkillManifest,
    pub prompt_content: String,
    pub trust: SkillTrust,
    pub source: SkillSource,
    pub content_hash: String,
    pub compiled_patterns: Vec<Regex>,
    pub lowercased_keywords: Vec<String>,
    pub lowercased_tags: Vec<String>,
}

impl LoadedSkill {
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
        const MAX_REGEX_SIZE: usize = 1 << 16;
        patterns
            .iter()
            .filter_map(
                |p| match RegexBuilder::new(p).size_limit(MAX_REGEX_SIZE).build() {
                    Ok(re) => Some(re),
                    Err(e) => {
                        tracing::warn!("Invalid activation regex pattern '{}': {}", p, e);
                        None
                    }
                },
            )
            .collect()
    }
}

/// Escape a string for safe inclusion in XML attributes.
pub fn escape_xml_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Escape prompt content to prevent tag breakout from `<skill>` delimiters.
pub fn escape_skill_content(content: &str) -> String {
    static SKILL_TAG_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
        Regex::new(r"(?i)</?[\s\x00]*skill").unwrap()
    });

    SKILL_TAG_RE
        .replace_all(content, |caps: &regex::Captures| {
            let matched = caps.get(0).unwrap().as_str();
            format!("&lt;{}", &matched[1..])
        })
        .into_owned()
}

/// Normalize line endings to LF.
pub fn normalize_line_endings(content: &str) -> String {
    content.replace("\r\n", "\n").replace('\r', "\n")
}
