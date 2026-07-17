//! One-shot subprocess runner for the CLI agent provider.
//!
//! Spawns a binary with a fixed argument vector (never through a shell), writes
//! the prompt to the child's stdin, and captures stdout/stderr. The run is bound
//! by a timeout and an optional cancellation token; on either, the child is
//! killed so no orphaned CLI keeps running in the background.

use std::future::pending;
use std::process::Stdio;
use std::time::Duration;

use once_cell::sync::Lazy;
use regex::Regex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Matches ANSI CSI escape sequences (e.g. color codes) so residual terminal
/// formatting can be stripped from captured stdout.
static ANSI_ESCAPE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]").unwrap());

/// Successful output of a one-shot CLI run.
#[derive(Debug)]
pub struct CliOutput {
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

/// Reason a one-shot CLI run failed.
#[derive(Debug)]
pub enum CliRunError {
    /// The binary could not be spawned (not found / not executable).
    Spawn(String),
    /// The process ran longer than the allowed timeout and was killed.
    Timeout(Duration),
    /// The run was cancelled by the caller and the process was killed.
    Cancelled,
    /// The process exited with a non-zero status. Carries the exit code (if any)
    /// and the captured stderr, for building an actionable message upstream.
    NonZeroExit {
        code: Option<i32>,
        stderr: String,
    },
    /// An I/O error occurred while writing stdin or reading stdout/stderr.
    Io(String),
}

impl std::fmt::Display for CliRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliRunError::Spawn(msg) => write!(f, "failed to start CLI: {msg}"),
            CliRunError::Timeout(dur) => {
                write!(f, "CLI timed out after {}s", dur.as_secs())
            }
            CliRunError::Cancelled => write!(f, "CLI run was cancelled"),
            CliRunError::NonZeroExit { code, stderr } => write!(
                f,
                "CLI exited with status {}: {}",
                code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".into()),
                stderr.trim()
            ),
            CliRunError::Io(msg) => write!(f, "CLI I/O error: {msg}"),
        }
    }
}

/// Strips residual terminal noise (ANSI escapes) from captured stdout and trims
/// surrounding whitespace. Markdown-level cleanup (code fences, `<think>` tags)
/// is handled later by `clean_llm_markdown_output`.
pub fn sanitize_cli_stdout(raw: &str) -> String {
    ANSI_ESCAPE_REGEX.replace_all(raw, "").trim().to_string()
}

/// Runs `command args...` one-shot: writes `stdin_input`, waits for exit under a
/// timeout and optional cancellation, and returns the captured output.
///
/// The command is spawned directly (argv), never via a shell, so no argument is
/// interpreted. stdout and stderr are drained by concurrent tasks to avoid pipe
/// back-pressure deadlocks on large prompts (>100 KB transcripts).
pub async fn run_cli_process(
    command: &str,
    args: &[String],
    stdin_input: &str,
    timeout: Duration,
    cancellation_token: Option<&CancellationToken>,
) -> Result<CliOutput, CliRunError> {
    // Fail fast if already cancelled before spawning anything.
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(CliRunError::Cancelled);
        }
    }

    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().map_err(|e| CliRunError::Spawn(e.to_string()))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CliRunError::Io("failed to capture child stdin".to_string()))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| CliRunError::Io("failed to capture child stdout".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| CliRunError::Io("failed to capture child stderr".to_string()))?;

    // Drain stdout/stderr concurrently so a large prompt write cannot deadlock
    // against the child filling its stdout pipe.
    let stdout_task =
        tokio::spawn(async move {
            let mut buf = Vec::new();
            stdout.read_to_end(&mut buf).await.map(|_| buf)
        });
    let stderr_task =
        tokio::spawn(async move {
            let mut buf = Vec::new();
            stderr.read_to_end(&mut buf).await.map(|_| buf)
        });

    // Write the prompt then close stdin (EOF) so the CLI starts producing output.
    let write_result = async {
        stdin.write_all(stdin_input.as_bytes()).await?;
        stdin.shutdown().await
    }
    .await;
    drop(stdin);
    if let Err(e) = write_result {
        // A broken pipe here usually means the child already exited; fall through
        // to reaping it below rather than masking the real exit status.
        log::debug!("cli-agent: writing stdin failed (child may have exited): {e}");
    }

    // Future that resolves only when the caller cancels; pending forever otherwise.
    let cancelled = async {
        match cancellation_token {
            Some(token) => token.cancelled().await,
            None => pending::<()>().await,
        }
    };

    let status = tokio::select! {
        wait_result = child.wait() => {
            wait_result.map_err(|e| CliRunError::Io(e.to_string()))?
        }
        _ = tokio::time::sleep(timeout) => {
            let _ = child.kill().await;
            return Err(CliRunError::Timeout(timeout));
        }
        _ = cancelled => {
            let _ = child.kill().await;
            return Err(CliRunError::Cancelled);
        }
    };

    let stdout_bytes = stdout_task
        .await
        .map_err(|e| CliRunError::Io(format!("stdout reader join error: {e}")))?
        .map_err(|e| CliRunError::Io(e.to_string()))?;
    let stderr_bytes = stderr_task
        .await
        .map_err(|e| CliRunError::Io(format!("stderr reader join error: {e}")))?
        .map_err(|e| CliRunError::Io(e.to_string()))?;

    let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

    if !status.success() {
        return Err(CliRunError::NonZeroExit {
            code: status.code(),
            stderr,
        });
    }

    Ok(CliOutput { stdout, stderr })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    // Writes an executable shell-script fixture to a unique temp path and returns
    // it. These fakes stand in for real CLIs so tests never invoke codex/claude.
    #[cfg(unix)]
    fn write_fake_binary(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let mut path = std::env::temp_dir();
        path.push(format!(
            "cli_agent_fake_{}_{}_{}",
            name,
            std::process::id(),
            fake_counter()
        ));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        file.flush().unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).unwrap();
        path
    }

    // Monotonic per-process counter to keep fixture paths unique across tests.
    fn fake_counter() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        COUNTER.fetch_add(1, Ordering::SeqCst)
    }

    #[test]
    fn sanitize_strips_ansi_and_trims() {
        let raw = "\u{1b}[32m# Summary\u{1b}[0m\n\nBody line\n  ";
        assert_eq!(sanitize_cli_stdout(raw), "# Summary\n\nBody line");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn success_echoes_stdin_to_stdout() {
        // Fake CLI that echoes whatever it receives on stdin to stdout.
        let bin = write_fake_binary("echo", "#!/bin/sh\ncat\n");
        let out = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "hello from prompt",
            Duration::from_secs(10),
            None,
        )
        .await
        .unwrap();
        assert_eq!(out.stdout.trim(), "hello from prompt");
        let _ = std::fs::remove_file(&bin);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn timeout_kills_slow_process() {
        // Fake CLI that sleeps far longer than the timeout.
        let bin = write_fake_binary("slow", "#!/bin/sh\nsleep 30\n");
        let start = std::time::Instant::now();
        let err = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "prompt",
            Duration::from_millis(300),
            None,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CliRunError::Timeout(_)), "got {:?}", err);
        // Should return promptly after the timeout, not after the 30s sleep.
        assert!(start.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_file(&bin);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn non_zero_exit_captures_stderr() {
        // Fake CLI that emits an auth-style error and exits non-zero.
        let bin = write_fake_binary(
            "fail",
            "#!/bin/sh\necho 'session expired, please log in' 1>&2\nexit 7\n",
        );
        let err = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "prompt",
            Duration::from_secs(10),
            None,
        )
        .await
        .unwrap_err();
        match err {
            CliRunError::NonZeroExit { code, stderr } => {
                assert_eq!(code, Some(7));
                assert!(stderr.contains("session expired"));
            }
            other => panic!("expected NonZeroExit, got {:?}", other),
        }
        let _ = std::fs::remove_file(&bin);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancellation_kills_process() {
        let bin = write_fake_binary("cancel", "#!/bin/sh\nsleep 30\n");
        let token = CancellationToken::new();
        let token_for_task = token.clone();
        // Cancel shortly after the process starts.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            token_for_task.cancel();
        });
        let start = std::time::Instant::now();
        let err = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "prompt",
            Duration::from_secs(30),
            Some(&token),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CliRunError::Cancelled), "got {:?}", err);
        assert!(start.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_file(&bin);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn noisy_stdout_is_sanitized() {
        // Fake CLI that wraps the markdown in ANSI color codes and padding.
        let bin = write_fake_binary(
            "noisy",
            "#!/bin/sh\nprintf '\\033[2m\\033[32m# Report\\033[0m\\n\\nDone\\n'\n",
        );
        let out = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "prompt",
            Duration::from_secs(10),
            None,
        )
        .await
        .unwrap();
        assert_eq!(sanitize_cli_stdout(&out.stdout), "# Report\n\nDone");
        let _ = std::fs::remove_file(&bin);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn already_cancelled_token_returns_immediately() {
        let bin = write_fake_binary("echo2", "#!/bin/sh\ncat\n");
        let token = CancellationToken::new();
        token.cancel();
        let err = run_cli_process(
            bin.to_str().unwrap(),
            &[],
            "prompt",
            Duration::from_secs(10),
            Some(&token),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CliRunError::Cancelled));
        let _ = std::fs::remove_file(&bin);
    }
}
