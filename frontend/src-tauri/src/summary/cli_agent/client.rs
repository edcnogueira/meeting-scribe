//! High-level client for the CLI agent summary provider.
//!
//! Resolves a [`CliAgentConfig`] into a concrete invocation, builds a single
//! prompt from the system + user prompts, runs the CLI one-shot, and returns the
//! sanitized markdown. Non-zero exits are turned into actionable auth errors.

use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::process::{run_cli_process, sanitize_cli_stdout, CliRunError};
use super::{effective_timeout_secs, login_hint_for, resolve_invocation};
use crate::summary::CliAgentConfig;

/// Combines the system and user prompts into a single prompt for the CLI.
///
/// The named presets are invoked in plain "read the prompt from stdin" mode, so
/// the system instructions are prepended with a clear separator rather than
/// relying on a provider-specific flag (e.g. `--append-system-prompt`), keeping
/// behavior uniform across codex/claude/gemini/custom.
fn build_combined_prompt(system_prompt: &str, user_prompt: &str) -> String {
    let system = system_prompt.trim();
    if system.is_empty() {
        return user_prompt.to_string();
    }
    format!("{system}\n\n---\n\n{user_prompt}")
}

/// Generates a summary by spawning the configured AI CLI one-shot.
///
/// # Arguments
/// * `config` - CLI agent configuration (preset/command/args/timeout)
/// * `system_prompt` - System instructions
/// * `user_prompt` - User content (transcript/report prompt)
/// * `cancellation_token` - Optional token to cancel (kills the child process)
///
/// # Returns
/// Sanitized markdown from the CLI's stdout, or an actionable error string.
pub async fn generate_with_cli_agent(
    config: &CliAgentConfig,
    system_prompt: &str,
    user_prompt: &str,
    cancellation_token: Option<&CancellationToken>,
) -> Result<String, String> {
    let invocation = resolve_invocation(config)?;
    let timeout = Duration::from_secs(effective_timeout_secs(config));
    let prompt = build_combined_prompt(system_prompt, user_prompt);

    log::info!(
        "cli-agent: running preset='{}' command='{}' (timeout {}s)",
        config.preset,
        invocation.command,
        timeout.as_secs()
    );

    match run_cli_process(
        &invocation.command,
        &invocation.args,
        &prompt,
        timeout,
        cancellation_token,
    )
    .await
    {
        Ok(output) => {
            let cleaned = sanitize_cli_stdout(&output.stdout);
            if cleaned.is_empty() {
                return Err(format!(
                    "The '{}' CLI produced no output. Ensure it is installed and authenticated (try `{}`).",
                    invocation.command,
                    login_hint_for(&config.preset)
                ));
            }
            log::info!("cli-agent: generation completed ({} chars)", cleaned.len());
            Ok(cleaned)
        }
        Err(err) => Err(map_run_error(&config.preset, &invocation.command, err)),
    }
}

/// Maps a low-level [`CliRunError`] into a user-facing, actionable message.
/// A non-zero exit most often means an expired/absent session, so the message
/// points at the CLI's login command (per the C1 detection strategy).
fn map_run_error(preset_id: &str, command: &str, err: CliRunError) -> String {
    match err {
        CliRunError::Spawn(msg) => format!(
            "Could not start the '{command}' CLI: {msg}. Make sure it is installed and on your PATH."
        ),
        CliRunError::Timeout(dur) => format!(
            "The '{}' CLI timed out after {}s. Try a shorter transcript or increase the timeout.",
            command,
            dur.as_secs()
        ),
        CliRunError::Cancelled => "Summary generation was cancelled".to_string(),
        CliRunError::NonZeroExit { code, stderr } => {
            let detail = stderr.trim();
            let base = format!(
                "The '{}' CLI exited with status {}. Your session may have expired \u{2014} try `{}` in a terminal, then retry.",
                command,
                code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()),
                login_hint_for(preset_id)
            );
            if detail.is_empty() {
                base
            } else {
                format!("{base}\nDetails: {detail}")
            }
        }
        CliRunError::Io(msg) => format!("The '{command}' CLI failed: {msg}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combined_prompt_joins_with_separator() {
        let combined = build_combined_prompt("SYS", "USER");
        assert!(combined.contains("SYS"));
        assert!(combined.contains("USER"));
        assert!(combined.contains("---"));
        assert!(combined.find("SYS").unwrap() < combined.find("USER").unwrap());
    }

    #[test]
    fn combined_prompt_empty_system_returns_user_only() {
        assert_eq!(build_combined_prompt("   ", "USER"), "USER");
    }

    #[test]
    fn non_zero_exit_message_is_actionable() {
        let msg = map_run_error(
            "codex",
            "codex",
            CliRunError::NonZeroExit {
                code: Some(1),
                stderr: "not logged in".to_string(),
            },
        );
        assert!(msg.contains("codex login"));
        assert!(msg.contains("not logged in"));
    }

    #[test]
    fn cancelled_maps_to_cancelled_marker() {
        let msg = map_run_error("claude", "claude", CliRunError::Cancelled);
        assert!(msg.contains("cancelled"));
    }

    #[test]
    fn spawn_error_hints_install() {
        let msg = map_run_error(
            "custom",
            "my-llm",
            CliRunError::Spawn("No such file or directory".to_string()),
        );
        assert!(msg.contains("installed"));
        assert!(msg.contains("my-llm"));
    }
}
