//! Derives a human-friendly meeting title from the generated summary.
//!
//! The summary template already asks the LLM for an H1 title
//! (`# <subject>` — see `Template::to_markdown_structure`). After a summary is
//! generated we turn that H1 into `YYYY-MM-DD - <subject>`, where the date is
//! the meeting's own `created_at` (never today's date) and `<subject>` is the
//! specific topic the LLM produced.
//!
//! Two safeguards keep this from clobbering user intent:
//! * a **manual-edit guard** — only the auto-generated placeholder title (from
//!   `useRecordingStart.ts` `generateMeetingTitle()`) or a title we previously
//!   produced (`YYYY-MM-DD - ...`) is replaced; a hand-edited title is left
//!   untouched;
//! * a **generic-title guard** — a missing, empty, placeholder, or generic H1
//!   (e.g. "Meeting Summary", the template name) is ignored, keeping the
//!   current title.

use crate::summary::processor::extract_meeting_name_from_markdown;
use once_cell::sync::Lazy;
use regex::Regex;

/// Maximum number of characters kept from the summary subject.
const MAX_SUBJECT_CHARS: usize = 80;

/// Auto-generated title from `useRecordingStart.ts` `generateMeetingTitle()`:
/// `Meeting DD_MM_YY_HH_MM_SS`.
static AUTO_TITLE_UNDERSCORE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^Meeting \d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}$").unwrap());

/// Older auto-generated fallback from `TranscriptContext.tsx`:
/// `Meeting YYYY-MM-DD_HH-MM-SS`.
static AUTO_TITLE_ISO: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^Meeting \d{4}-\d{2}-\d{2}_\d{2}-\d{2}-\d{2}$").unwrap());

/// A title already produced by this feature: `YYYY-MM-DD - <subject>`.
static DATE_PREFIX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2} - .+").unwrap());

/// Generic, non-specific H1 values (compared lowercased + trimmed) that must
/// never become a meeting title.
const GENERIC_SUBJECTS: &[&str] = &[
    "meeting",
    "meeting summary",
    "meeting minutes",
    "meeting notes",
    "meeting report",
    "summary",
    "report",
    "untitled",
    "untitled meeting",
    "title",
    "add title here",
    "ai-generated title",
    "[ai-generated title]",
];

/// True when the current title is the auto-generated recording placeholder.
pub fn is_auto_generated_title(current: &str) -> bool {
    AUTO_TITLE_UNDERSCORE.is_match(current) || AUTO_TITLE_ISO.is_match(current)
}

/// True when the current title already carries a `YYYY-MM-DD - ` prefix that
/// this feature produced (so re-generating a summary can refresh the subject).
pub fn has_date_prefix(current: &str) -> bool {
    DATE_PREFIX.is_match(current)
}

/// Whether the current title may be replaced automatically. A hand-edited
/// title matches neither branch and is therefore never overwritten.
pub fn should_replace_title(current: &str) -> bool {
    is_auto_generated_title(current) || has_date_prefix(current)
}

/// True when the extracted subject is empty, a leftover placeholder, or a
/// generic label that carries no information about the meeting.
pub fn is_generic_subject(subject: &str, template_name: Option<&str>) -> bool {
    let normalized = subject.trim().to_lowercase();

    if normalized.is_empty() {
        return true;
    }

    // Leftover template placeholder such as `<Add Title here>`.
    if subject.contains('<') || subject.contains('>') || normalized.contains("add title here") {
        return true;
    }

    if GENERIC_SUBJECTS.contains(&normalized.as_str()) {
        return true;
    }

    if let Some(name) = template_name {
        if normalized == name.trim().to_lowercase() {
            return true;
        }
    }

    false
}

/// Truncates the subject to at most `MAX_SUBJECT_CHARS` characters (Unicode
/// scalar values), trimming whitespace introduced by the cut.
fn truncate_subject(subject: &str) -> String {
    let trimmed = subject.trim();
    if trimmed.chars().count() <= MAX_SUBJECT_CHARS {
        return trimmed.to_string();
    }
    let truncated: String = trimmed.chars().take(MAX_SUBJECT_CHARS).collect();
    truncated.trim_end().to_string()
}

/// Builds `YYYY-MM-DD - <subject>` from the meeting date and (truncated) subject.
pub fn assemble_dated_title(created_at_date: &str, subject: &str) -> String {
    format!("{} - {}", created_at_date, truncate_subject(subject))
}

/// Derives the new meeting title from a generated summary, or `None` when the
/// current title must be kept (manual edit, or missing/generic summary H1).
///
/// # Arguments
/// * `final_markdown` - The cleaned final summary markdown (post
///   `clean_llm_markdown_output`).
/// * `current_title` - The meeting's current title.
/// * `created_at_date` - The meeting's creation date formatted as `YYYY-MM-DD`.
/// * `template_name` - The active template's display name, rejected as generic.
pub fn derive_meeting_title(
    final_markdown: &str,
    current_title: &str,
    created_at_date: &str,
    template_name: Option<&str>,
) -> Option<String> {
    if !should_replace_title(current_title) {
        return None;
    }

    let subject = extract_meeting_name_from_markdown(final_markdown)?;
    if is_generic_subject(&subject, template_name) {
        return None;
    }

    Some(assemble_dated_title(created_at_date, &subject))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- H1 extraction (delegated to extract_meeting_name_from_markdown) ---

    #[test]
    fn extracts_specific_h1_from_summary() {
        let md = "# CLI Provider Alignment\n\n## Decisions\n- Ship it";
        assert_eq!(
            extract_meeting_name_from_markdown(md).as_deref(),
            Some("CLI Provider Alignment")
        );
    }

    #[test]
    fn extracts_h1_ignoring_leading_noise_lines() {
        // clean_llm_markdown_output trims fences/whitespace; the first `# ` line wins.
        let md = "Some preamble the model leaked\n# Budget Review Q3\n## Notes\nbody";
        assert_eq!(
            extract_meeting_name_from_markdown(md).as_deref(),
            Some("Budget Review Q3")
        );
    }

    #[test]
    fn missing_h1_yields_none() {
        let md = "## Decisions\n- No top-level heading here";
        assert_eq!(extract_meeting_name_from_markdown(md), None);
    }

    // --- generic-subject guard ---

    #[test]
    fn generic_subjects_are_rejected() {
        for generic in [
            "Meeting Summary",
            "meeting minutes",
            "MEETING NOTES",
            "Summary",
            "Untitled",
            "  meeting  ",
            "<Add Title here>",
            "[AI-Generated Title]",
        ] {
            assert!(
                is_generic_subject(generic, None),
                "expected '{generic}' to be treated as generic"
            );
        }
    }

    #[test]
    fn template_name_is_rejected_as_generic() {
        assert!(is_generic_subject("Standard Meeting", Some("Standard Meeting")));
        assert!(is_generic_subject("standard meeting", Some("Standard Meeting")));
    }

    #[test]
    fn specific_subject_is_not_generic() {
        assert!(!is_generic_subject("Q3 Roadmap Prioritization", Some("Standard Meeting")));
        assert!(!is_generic_subject("Alinhamento do provedor CLI", None));
    }

    // --- manual-edit guard patterns ---

    #[test]
    fn auto_generated_underscore_title_is_replaceable() {
        assert!(is_auto_generated_title("Meeting 17_07_26_14_30_05"));
        assert!(should_replace_title("Meeting 17_07_26_14_30_05"));
    }

    #[test]
    fn auto_generated_iso_title_is_replaceable() {
        assert!(is_auto_generated_title("Meeting 2026-07-17_14-30-05"));
        assert!(should_replace_title("Meeting 2026-07-17_14-30-05"));
    }

    #[test]
    fn previously_dated_title_is_replaceable() {
        assert!(has_date_prefix("2026-07-17 - Alinhamento do provedor CLI"));
        assert!(should_replace_title("2026-07-17 - Alinhamento do provedor CLI"));
    }

    #[test]
    fn hand_edited_title_is_never_replaced() {
        for manual in [
            "Weekly sync with design",
            "Meeting about the new API",         // not the exact auto pattern
            "2026-7-1 - loose date",             // not zero-padded → not our prefix
            "Meeting 17_07_26",                  // partial timestamp only
            "Reunião de planejamento",
        ] {
            assert!(!should_replace_title(manual), "'{manual}' must be kept");
        }
    }

    // --- title assembly with the meeting date ---

    #[test]
    fn assembles_dated_title() {
        assert_eq!(
            assemble_dated_title("2026-07-17", "Alinhamento do provedor CLI"),
            "2026-07-17 - Alinhamento do provedor CLI"
        );
    }

    #[test]
    fn subject_is_truncated_to_80_chars() {
        let long = "A".repeat(200);
        let title = assemble_dated_title("2026-07-17", &long);
        let subject = title.strip_prefix("2026-07-17 - ").unwrap();
        assert_eq!(subject.chars().count(), 80);
    }

    #[test]
    fn truncation_respects_unicode_scalars() {
        let long = "é".repeat(120);
        let title = assemble_dated_title("2026-07-17", &long);
        let subject = title.strip_prefix("2026-07-17 - ").unwrap();
        assert_eq!(subject.chars().count(), 80);
    }

    // --- end-to-end derivation ---

    #[test]
    fn derives_title_from_auto_generated_meeting() {
        let md = "# Alinhamento do provedor CLI\n\n## Decisões\n- Feito";
        assert_eq!(
            derive_meeting_title(md, "Meeting 17_07_26_14_30_05", "2026-07-17", Some("Standard Meeting")),
            Some("2026-07-17 - Alinhamento do provedor CLI".to_string())
        );
    }

    #[test]
    fn refreshes_subject_on_previously_dated_title() {
        let md = "# Novo assunto\n## Notas\nbody";
        assert_eq!(
            derive_meeting_title(md, "2026-07-17 - Assunto antigo", "2026-07-17", None),
            Some("2026-07-17 - Novo assunto".to_string())
        );
    }

    #[test]
    fn keeps_hand_edited_title() {
        let md = "# Some specific subject\n## Notes\nbody";
        assert_eq!(
            derive_meeting_title(md, "My hand-picked title", "2026-07-17", None),
            None
        );
    }

    #[test]
    fn keeps_title_when_summary_h1_is_generic() {
        let md = "# Meeting Summary\n## Notes\nbody";
        assert_eq!(
            derive_meeting_title(md, "Meeting 17_07_26_14_30_05", "2026-07-17", None),
            None
        );
    }

    #[test]
    fn keeps_title_when_summary_h1_missing() {
        let md = "## Notes\nno top-level heading";
        assert_eq!(
            derive_meeting_title(md, "Meeting 17_07_26_14_30_05", "2026-07-17", None),
            None
        );
    }
}
