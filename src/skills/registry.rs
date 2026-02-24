//! Skill registry for discovering and loading skills from ~/.clawed/skills/.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::skills::parser::parse_skill_md;
use crate::skills::{
    LoadedSkill, MAX_PROMPT_FILE_SIZE, SkillSource, SkillTrust, normalize_line_endings,
};

const MAX_DISCOVERED_SKILLS: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum SkillRegistryError {
    #[error("Failed to read skill file {path}: {reason}")]
    ReadError { path: String, reason: String },

    #[error("Failed to parse SKILL.md for '{name}': {reason}")]
    ParseError { name: String, reason: String },

    #[error("Skill file too large for '{name}': {size} bytes (max {max} bytes)")]
    FileTooLarge { name: String, size: u64, max: u64 },

    #[allow(dead_code)]
    #[error("Symlink detected in skills directory: {path}")]
    SymlinkDetected { path: String },
}

/// Registry of available skills.
pub struct SkillRegistry {
    skills: Vec<LoadedSkill>,
    skills_dir: PathBuf,
}

impl SkillRegistry {
    pub fn new(skills_dir: PathBuf) -> Self {
        Self {
            skills: Vec::new(),
            skills_dir,
        }
    }

    /// Get all loaded skills.
    pub fn skills(&self) -> &[LoadedSkill] {
        &self.skills
    }

    /// Discover and load skills from the skills directory.
    pub async fn discover_all(&mut self) -> Vec<String> {
        let mut loaded_names = Vec::new();

        let dir = self.skills_dir.clone();
        if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
            tracing::debug!("Skills directory does not exist: {:?}", dir);
            return loaded_names;
        }

        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read skills directory {:?}: {}", dir, e);
                return loaded_names;
            }
        };

        let mut count = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            if count >= MAX_DISCOVERED_SKILLS {
                tracing::warn!(
                    "Reached maximum skill count ({}), skipping remaining",
                    MAX_DISCOVERED_SKILLS
                );
                break;
            }

            let path = entry.path();

            // Reject symlinks
            if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await {
                if metadata.file_type().is_symlink() {
                    tracing::warn!("Rejecting symlink in skills directory: {:?}", path);
                    continue;
                }
            }

            // Check for subdirectory layout: <name>/SKILL.md
            if path.is_dir() {
                let skill_file = path.join("SKILL.md");
                if tokio::fs::try_exists(&skill_file).await.unwrap_or(false) {
                    match self
                        .load_skill(&skill_file, SkillTrust::Trusted, &path)
                        .await
                    {
                        Ok(skill) => {
                            let name = skill.name().to_string();
                            tracing::info!("Loaded skill: {} (trusted)", name);
                            loaded_names.push(name);
                            self.skills.push(skill);
                            count += 1;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load skill from {:?}: {}", skill_file, e);
                        }
                    }
                }
            }
        }

        loaded_names
    }

    async fn load_skill(
        &self,
        skill_file: &Path,
        trust: SkillTrust,
        source_dir: &Path,
    ) -> Result<LoadedSkill, SkillRegistryError> {
        // Check file size
        let metadata = tokio::fs::metadata(skill_file).await.map_err(|e| {
            SkillRegistryError::ReadError {
                path: skill_file.display().to_string(),
                reason: e.to_string(),
            }
        })?;

        if metadata.len() > MAX_PROMPT_FILE_SIZE {
            return Err(SkillRegistryError::FileTooLarge {
                name: source_dir.display().to_string(),
                size: metadata.len(),
                max: MAX_PROMPT_FILE_SIZE,
            });
        }

        let raw_content =
            tokio::fs::read_to_string(skill_file)
                .await
                .map_err(|e| SkillRegistryError::ReadError {
                    path: skill_file.display().to_string(),
                    reason: e.to_string(),
                })?;

        let parsed = parse_skill_md(&raw_content).map_err(|e| SkillRegistryError::ParseError {
            name: source_dir
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("unknown")
                .to_string(),
            reason: e.to_string(),
        })?;

        // Compute content hash
        let normalized = normalize_line_endings(&parsed.prompt_content);
        let mut hasher = Sha256::new();
        hasher.update(normalized.as_bytes());
        let hash = format!("sha256:{:x}", hasher.finalize());

        // Compile patterns
        let compiled_patterns =
            LoadedSkill::compile_patterns(&parsed.manifest.activation.patterns);

        // Pre-compute lowercased keywords and tags
        let lowercased_keywords = parsed
            .manifest
            .activation
            .keywords
            .iter()
            .map(|k| k.to_lowercase())
            .collect();
        let lowercased_tags = parsed
            .manifest
            .activation
            .tags
            .iter()
            .map(|t| t.to_lowercase())
            .collect();

        Ok(LoadedSkill {
            manifest: parsed.manifest,
            prompt_content: parsed.prompt_content,
            trust,
            source: SkillSource::User(source_dir.to_path_buf()),
            content_hash: hash,
            compiled_patterns,
            lowercased_keywords,
            lowercased_tags,
        })
    }
}
