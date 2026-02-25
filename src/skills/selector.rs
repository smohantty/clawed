//! Deterministic skill prefilter for selection.

use crate::skills::LoadedSkill;

const MAX_KEYWORD_SCORE: u32 = 30;
const MAX_TAG_SCORE: u32 = 15;
const MAX_REGEX_SCORE: u32 = 40;

/// Select candidate skills for a given message using deterministic scoring.
pub fn prefilter_skills<'a>(
    message: &str,
    available_skills: &'a [LoadedSkill],
    max_candidates: usize,
    max_context_tokens: usize,
) -> Vec<&'a LoadedSkill> {
    if available_skills.is_empty() || message.is_empty() {
        return vec![];
    }

    let message_lower = message.to_lowercase();

    let mut scored: Vec<(&LoadedSkill, u32)> = available_skills
        .iter()
        .filter_map(|skill| {
            let score = score_skill(skill, &message_lower, message);
            if score > 0 {
                Some((skill, score))
            } else {
                None
            }
        })
        .collect();

    scored.sort_by(|a, b| b.1.cmp(&a.1));

    let mut result = Vec::new();
    let mut budget_remaining = max_context_tokens;

    for (skill, _score) in scored {
        if result.len() >= max_candidates {
            break;
        }
        let declared_tokens = skill.manifest.activation.max_context_tokens;
        let approx_tokens = (skill.prompt_content.len() as f64 * 0.25) as usize;
        let raw_cost = if approx_tokens > declared_tokens * 2 {
            approx_tokens
        } else {
            declared_tokens
        };
        let token_cost = raw_cost.max(1);
        if token_cost <= budget_remaining {
            budget_remaining -= token_cost;
            result.push(skill);
        }
    }

    result
}

fn score_skill(skill: &LoadedSkill, message_lower: &str, message_original: &str) -> u32 {
    let mut score: u32 = 0;

    // Keyword scoring
    let mut keyword_score: u32 = 0;
    for kw_lower in &skill.lowercased_keywords {
        if message_lower
            .split_whitespace()
            .any(|word| word.trim_matches(|c: char| !c.is_alphanumeric()) == kw_lower.as_str())
        {
            keyword_score += 10;
        } else if message_lower.contains(kw_lower.as_str()) {
            keyword_score += 5;
        }
    }
    score += keyword_score.min(MAX_KEYWORD_SCORE);

    // Tag scoring
    let mut tag_score: u32 = 0;
    for tag_lower in &skill.lowercased_tags {
        if message_lower.contains(tag_lower.as_str()) {
            tag_score += 3;
        }
    }
    score += tag_score.min(MAX_TAG_SCORE);

    // Regex pattern scoring
    let mut regex_score: u32 = 0;
    for re in &skill.compiled_patterns {
        if re.is_match(message_original) {
            regex_score += 20;
        }
    }
    score += regex_score.min(MAX_REGEX_SCORE);

    // Fallback: if no explicit activation criteria, match against name and description
    if skill.lowercased_keywords.is_empty()
        && skill.lowercased_tags.is_empty()
        && skill.compiled_patterns.is_empty()
    {
        let mut name_score: u32 = 0;
        // Split skill name on non-alphanumeric chars (e.g. "tizen-tool-cli" → ["tizen", "tool", "cli"])
        for part in skill
            .manifest
            .name
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|p| p.len() >= 3)
        {
            if message_lower
                .split_whitespace()
                .any(|word| word.trim_matches(|c: char| !c.is_alphanumeric()) == part)
            {
                name_score += 10;
            }
        }
        score += name_score.min(MAX_KEYWORD_SCORE);

        let mut desc_score: u32 = 0;
        for word in skill
            .manifest
            .description
            .to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() >= 4)
        {
            let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.len() >= 4
                && message_lower
                    .split_whitespace()
                    .any(|mw| mw.trim_matches(|c: char| !c.is_alphanumeric()) == clean)
            {
                desc_score += 3;
            }
        }
        score += desc_score.min(MAX_TAG_SCORE);
    }

    score
}
