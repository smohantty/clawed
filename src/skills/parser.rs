//! SKILL.md parser.

use crate::skills::{SkillManifest, validate_skill_name};

#[derive(Debug, thiserror::Error)]
pub enum SkillParseError {
    #[error("Missing YAML frontmatter delimiters")]
    MissingFrontmatter,

    #[error("Invalid YAML frontmatter: {0}")]
    InvalidYaml(String),

    #[error("Prompt body is empty")]
    EmptyPrompt,

    #[error("Invalid skill name '{name}'")]
    InvalidName { name: String },
}

pub struct ParsedSkill {
    pub manifest: SkillManifest,
    pub prompt_content: String,
}

pub fn parse_skill_md(content: &str) -> Result<ParsedSkill, SkillParseError> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);

    let trimmed = content.trim_start_matches(['\n', '\r']);
    if !trimmed.starts_with("---") {
        return Err(SkillParseError::MissingFrontmatter);
    }

    let after_first = &trimmed[3..];
    let after_first_line = match after_first.find('\n') {
        Some(pos) => &after_first[pos + 1..],
        None => return Err(SkillParseError::MissingFrontmatter),
    };

    let yaml_end =
        find_closing_delimiter(after_first_line).ok_or(SkillParseError::MissingFrontmatter)?;

    let yaml_str = &after_first_line[..yaml_end];

    let mut manifest: SkillManifest =
        serde_yml::from_str(yaml_str).map_err(|e| SkillParseError::InvalidYaml(e.to_string()))?;

    if !validate_skill_name(&manifest.name) {
        return Err(SkillParseError::InvalidName {
            name: manifest.name.clone(),
        });
    }

    manifest.activation.enforce_limits();

    let after_yaml = &after_first_line[yaml_end..];
    let prompt_start = after_yaml
        .find('\n')
        .map(|p| p + 1)
        .unwrap_or(after_yaml.len());
    let prompt_content = after_yaml[prompt_start..]
        .trim_start_matches('\n')
        .to_string();

    if prompt_content.trim().is_empty() {
        return Err(SkillParseError::EmptyPrompt);
    }

    Ok(ParsedSkill {
        manifest,
        prompt_content,
    })
}

fn find_closing_delimiter(content: &str) -> Option<usize> {
    let mut pos = 0;
    for line in content.lines() {
        if line.trim() == "---" {
            return Some(pos);
        }
        pos += line.len() + 1;
    }
    None
}
