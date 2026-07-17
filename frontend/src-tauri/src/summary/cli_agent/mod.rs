//! CLI agent summary provider.
//!
//! Generates a meeting summary by spawning an installed AI CLI (e.g. `codex`,
//! `claude`, `gemini`) as a one-shot, non-interactive subprocess: the prompt is
//! written to the child's stdin and the markdown answer is read from stdout.
//! No API key and no shell are involved.
//!
//! The preset registry below encodes the invocation contract validated in the
//! C1 spike (`tasks/cli-summary/C1-resultados.md`): every CLI reads the prompt
//! from stdin and writes clean markdown to stdout (preamble/logs go to stderr).

pub mod client;
pub mod process;

pub use client::generate_with_cli_agent;

/// Default one-shot timeout (10 minutes). Measured latencies in C1 were 20-27s
/// even for ~118 KB transcripts; the generous default absorbs cold start, high
/// reasoning effort, and long real meetings without hanging the UI forever.
pub const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// A resolved CLI invocation: the binary plus its argument vector. The prompt is
/// always delivered via stdin, so no argument ever carries transcript content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInvocation {
    pub command: String,
    pub args: Vec<String>,
}

/// Static preset definition for a known AI CLI.
struct CliPreset {
    /// Preset id as stored in the config (`preset` field).
    id: &'static str,
    /// Binary name (resolved via PATH by the OS).
    command: &'static str,
    /// Non-interactive arguments. A trailing `-` (where present) tells the CLI
    /// to read the prompt from stdin.
    args: &'static [&'static str],
    /// Human-friendly command the user runs to (re)authenticate, surfaced in the
    /// actionable error when the CLI exits non-zero (likely expired session).
    login_hint: &'static str,
}

/// Preset table validated in C1. `codex` and `claude` were validated on real
/// runs; `gemini` comes from the official docs (CLI not installed during the
/// spike) and should be reconfirmed once available.
const PRESETS: &[CliPreset] = &[
    // codex exec: stdout carries only the final message; version/model/session
    // logs go to stderr. `-` reads instructions from stdin.
    CliPreset {
        id: "codex",
        command: "codex",
        args: &[
            "exec",
            "--color",
            "never",
            "-s",
            "read-only",
            "--skip-git-repo-check",
            "-",
        ],
        login_hint: "codex login",
    },
    // claude -p --output-format text: prints only the final answer to stdout;
    // stdin (piped) is treated as the prompt.
    CliPreset {
        id: "claude",
        command: "claude",
        args: &["-p", "--output-format", "text"],
        login_hint: "claude (then complete the login flow)",
    },
    // gemini -p - --output-format text (headless): piped stdin becomes the
    // prompt; text output prints only the response. Not validated in C1.
    CliPreset {
        id: "gemini",
        command: "gemini",
        args: &["-p", "-", "--output-format", "text"],
        login_hint: "gemini (then complete the login flow)",
    },
];

/// Resolves a preset id to its static definition.
fn find_preset(id: &str) -> Option<&'static CliPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// Resolves a [`CliAgentConfig`](crate::summary::CliAgentConfig) into a concrete
/// [`CliInvocation`]. Named presets ignore the config's `command`/`args`; the
/// `custom` preset requires both to be provided.
pub fn resolve_invocation(
    config: &crate::summary::CliAgentConfig,
) -> Result<CliInvocation, String> {
    if config.preset == "custom" {
        let command = config
            .command
            .as_ref()
            .map(|c| c.trim())
            .filter(|c| !c.is_empty())
            .ok_or_else(|| "Custom CLI agent requires a 'command' to be set".to_string())?;
        let args = config.args.clone().unwrap_or_default();
        return Ok(CliInvocation {
            command: command.to_string(),
            args,
        });
    }

    let preset = find_preset(&config.preset).ok_or_else(|| {
        format!(
            "Unknown CLI agent preset '{}'. Expected one of: codex, claude, gemini, custom",
            config.preset
        )
    })?;

    Ok(CliInvocation {
        command: preset.command.to_string(),
        args: preset.args.iter().map(|s| s.to_string()).collect(),
    })
}

/// Returns the login hint for a preset id, used to build actionable auth errors.
/// Falls back to a generic hint for the custom preset or unknown ids.
fn login_hint_for(preset_id: &str) -> String {
    find_preset(preset_id)
        .map(|p| p.login_hint.to_string())
        .unwrap_or_else(|| "the CLI's login command".to_string())
}

/// Effective timeout for a config, applying the [`DEFAULT_TIMEOUT_SECS`] default
/// and treating an explicit `0` as "use the default" (never a zero timeout).
fn effective_timeout_secs(config: &crate::summary::CliAgentConfig) -> u64 {
    match config.timeout_secs {
        Some(secs) if secs > 0 => secs,
        _ => DEFAULT_TIMEOUT_SECS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::summary::CliAgentConfig;

    fn config(preset: &str) -> CliAgentConfig {
        CliAgentConfig {
            preset: preset.to_string(),
            command: None,
            args: None,
            timeout_secs: None,
        }
    }

    #[test]
    fn resolves_codex_preset_with_stdin_marker() {
        let inv = resolve_invocation(&config("codex")).unwrap();
        assert_eq!(inv.command, "codex");
        assert_eq!(inv.args.first().unwrap(), "exec");
        assert_eq!(inv.args.last().unwrap(), "-");
        assert!(inv.args.iter().any(|a| a == "read-only"));
    }

    #[test]
    fn resolves_claude_preset() {
        let inv = resolve_invocation(&config("claude")).unwrap();
        assert_eq!(inv.command, "claude");
        assert_eq!(inv.args, vec!["-p", "--output-format", "text"]);
    }

    #[test]
    fn resolves_gemini_preset() {
        let inv = resolve_invocation(&config("gemini")).unwrap();
        assert_eq!(inv.command, "gemini");
        assert!(inv.args.iter().any(|a| a == "-p"));
        assert!(inv.args.iter().any(|a| a == "text"));
    }

    #[test]
    fn custom_preset_uses_config_command_and_args() {
        let cfg = CliAgentConfig {
            preset: "custom".to_string(),
            command: Some("/usr/local/bin/my-llm".to_string()),
            args: Some(vec!["run".to_string(), "--stdin".to_string()]),
            timeout_secs: Some(120),
        };
        let inv = resolve_invocation(&cfg).unwrap();
        assert_eq!(inv.command, "/usr/local/bin/my-llm");
        assert_eq!(inv.args, vec!["run", "--stdin"]);
    }

    #[test]
    fn custom_preset_without_command_errors() {
        let mut cfg = config("custom");
        cfg.command = Some("   ".to_string());
        assert!(resolve_invocation(&cfg).is_err());
    }

    #[test]
    fn unknown_preset_errors() {
        assert!(resolve_invocation(&config("bogus")).is_err());
    }

    #[test]
    fn timeout_defaults_and_zero_falls_back() {
        assert_eq!(effective_timeout_secs(&config("codex")), DEFAULT_TIMEOUT_SECS);

        let mut cfg = config("codex");
        cfg.timeout_secs = Some(0);
        assert_eq!(effective_timeout_secs(&cfg), DEFAULT_TIMEOUT_SECS);

        cfg.timeout_secs = Some(42);
        assert_eq!(effective_timeout_secs(&cfg), 42);
    }
}
