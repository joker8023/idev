//! Daemon：原 Go server/internal/daemon + server/pkg/agent 的 Rust 等价。
//! 原架构：Go daemon 通过 WebSocket 连后端，后端派单，daemon 在 worktree 里 spawn
//! agent CLI（claude / codex / ... 共 26 个适配器）。
//! 本地版：Tauri 进程内即 daemon，无 WebSocket、无后端派单。issue 指派给 agent 后，
//! 前端直接调 run_issue，Rust 在后台线程 spawn CLI，日志写回 run 表，前端轮询 run 日志。
//!
//! 安全（与原版一致的警告）：Agent CLI 默认以当前 OS 用户的全部权限运行，
//! 原版 Codex 是 danger-full-access、Claude 是 --permission-mode bypassPermissions。
//! 本地版同样不做沙箱承诺；如需隔离，请用专用用户 / VM / 容器跑本程序。

use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 26 个 CLI 的二进制名与默认参数（原 server/pkg/agent/*.go 的最小等价映射）。
/// 第一版只真实执行 claude / codex，其余返回“未安装/未接入”错误并记录到 run 日志，
/// 后续按 MIGRATION.md 的优先级逐个接入，避免一次性抄 9000+ 行适配代码。
pub fn cli_command(cli: &str, _workdir: &str, prompt: &str) -> Option<(String, Vec<String>)> {
    match cli {
        "claude" => Some((
            "claude".to_string(),
            vec![
                "-p".into(),
                prompt.to_string(),
                "--permission-mode".into(),
                "bypassPermissions".into(),
            ],
        )),
        "codex" => Some((
            "codex".to_string(),
            vec!["exec".into(), "--sandbox".into(), "danger-full-access".into(), prompt.to_string()],
        )),
        // Gemini CLI：`gemini -p <prompt>` 非交互输出。
        "gemini" => Some((
            "gemini".to_string(),
            vec!["-p".into(), prompt.to_string()],
        )),
        // opencode：`opencode run <prompt>`。
        "opencode" => Some((
            "opencode".to_string(),
            vec!["run".into(), prompt.to_string()],
        )),
        // aider：--message + --yes 非交互；工作目录不是 git 仓库时 --no-git 避免报错。
        "aider" => Some((
            "aider".to_string(),
            vec!["--message".into(), prompt.to_string(), "--yes".into(), "--no-git".into()],
        )),
        // GitHub Copilot CLI：`copilot -p <prompt>` 非交互执行。
        "copilot" => Some((
            "copilot".to_string(),
            vec!["-p".into(), prompt.to_string()],
        )),
        _ => None,
    }
}

/// 探测本机已安装的 agent CLI（原 daemon agents_probe.go 的简化版：which）。
pub fn detect_clis() -> Vec<String> {
    let mut out = vec![];
    for cli in [
        "claude", "codex", "cursor-agent", "copilot", "kimi", "opencode", "openclaw",
        "grok", "qwen", "codebuddy", "traecli", "mcode", "dsh", "omp",
    ] {
        if which(cli) {
            out.push(cli.to_string());
        }
    }
    out
}

#[cfg(unix)]
fn which(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn which(bin: &str) -> bool {
    std::process::Command::new("where")
        .arg(bin)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub struct RunHandle {
    pub cancelled: Arc<AtomicBool>,
}

impl RunHandle {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

/// 运行中子进程表：run_id → Child，供 cancel_run 真 kill。
/// Child 在后台线程 wait_with_output，wait 完从表里移除。
pub struct RunChildren(pub std::sync::Mutex<std::collections::HashMap<String, std::process::Child>>);
