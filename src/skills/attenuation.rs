//! Trust-based tool filtering (authority attenuation).

use crate::llm::ToolDefinition;
use crate::skills::{LoadedSkill, SkillTrust};

/// Tools that are always safe -- read-only, no side effects.
const READ_ONLY_TOOLS: &[&str] = &[
    "time", "echo", "json", "read_file", "list_dir", "skill_list", "load_skill",
];

/// Filter tool definitions based on the trust level of active skills.
pub fn attenuate_tools(
    tools: &[ToolDefinition],
    active_skills: &[&LoadedSkill],
) -> Vec<ToolDefinition> {
    if active_skills.is_empty() {
        return tools.to_vec();
    }

    let min_trust = active_skills
        .iter()
        .map(|s| s.trust)
        .min()
        .unwrap_or(SkillTrust::Trusted);

    match min_trust {
        SkillTrust::Trusted => tools.to_vec(),
        SkillTrust::Installed => tools
            .iter()
            .filter(|t| READ_ONLY_TOOLS.contains(&t.name.as_str()))
            .cloned()
            .collect(),
    }
}
