//! Tauri commands：前端通过 invoke 调用，替代原 Go 的 REST + WebSocket。
//! 原 API（server/internal/handler/*.go）→ 本地命令映射见 docs/MIGRATION.md。
//! 返回值直接是 serde 结构体，前端用 @tauri-apps/api 调用，无 HTTP、无 JWT。
//!
//! 实时：原 WebSocket 推送改为 Tauri 事件 `multica://entity-changed`
//! （`{kind, id}`），前端 `TauriRealtimeBridge` 监听后失效 React Query 缓存。

use rusqlite::{params, Row};
use tauri::{Emitter, Manager, State};
use crate::db::Db;
use crate::models::*;

pub mod expanded;
pub mod write2;
pub mod write3;

/// 前端变更事件：任何写命令成功后 emit，前端失效缓存（替代 WS 推送）。
pub fn emit_changed(app: &tauri::AppHandle, kind: &str, id: &str) {
    let _ = app.emit(
        "multica://entity-changed",
        serde_json::json!({ "kind": kind, "id": id }),
    );
}


/// workspace 键解析：调用方可传 workspace id 或 slug（前端有的地方只知道 slug
/// "default"，有的地方传真实 UUID），统一解析为真实 id；解析不到时原样返回
/// （后续查询/插入会自然返回空或报错，不静默张冠李戴）。
pub(crate) fn resolve_workspace_id(conn: &rusqlite::Connection, key: &str) -> String {
    conn.query_row(
        "SELECT id FROM workspace WHERE id = ?1 OR slug = ?1",
        params![key],
        |r| r.get::<_, String>(0),
    ).unwrap_or_else(|_| key.to_string())
}

fn json_text(v: &serde_json::Value) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".into())
}

/// 部分更新的 SET 拼接 helper（避免闭包借用 sets/vals 与后续直接 push 冲突）。
pub(crate) fn push_set(sets: &mut Vec<String>, vals: &mut Vec<rusqlite::types::Value>, col: &str, v: Option<String>) {
    if let Some(v) = v {
        sets.push(format!("{} = ?", col));
        vals.push(v.into());
    }
}

fn parse_text(s: &str, fallback: serde_json::Value) -> serde_json::Value {
    parse_json(s, fallback)
}

/// issue 行 → 完整 Issue（24 列 + 评论数子查询）。labels 暂为空数组
/// （标签在 issue_label 表，列表页不展开，详情页走 list_labels_for_issue）。
fn issue_from_row(r: &Row) -> rusqlite::Result<Issue> {
    let body: String = r.get(5)?;
    Ok(Issue {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        number: r.get(2)?,
        identifier: r.get(3)?,
        title: r.get(4)?,
        body: body.clone(),
        description: Some(body),
        status: r.get(6)?,
        status_category: r.get(7)?,
        priority: r.get(8)?,
        assignee_type: r.get(9)?,
        assignee_id: r.get(10)?,
        assignee_name: r.get(11)?,
        creator_type: r.get(12)?,
        creator_id: r.get(13)?,
        parent_issue_id: r.get(14)?,
        project_id: r.get(15)?,
        position: r.get(16)?,
        stage: r.get(17)?,
        start_date: r.get(18)?,
        due_date: r.get(19)?,
        labels: serde_json::json!([]),
        metadata: parse_text(&r.get::<_, String>(20)?, serde_json::json!({})),
        properties: parse_text(&r.get::<_, String>(21)?, serde_json::json!({})),
        comments_count: r.get(22)?,
        created_at: r.get(23)?,
        updated_at: r.get(24)?,
    })
}

/// issue 查询的列顺序必须与 issue_from_row 一致。
pub(crate) const ISSUE_COLS: &str = "i.id, i.workspace_id, i.number, i.identifier, i.title, i.body, i.status, i.status_category, i.priority, i.assignee_type, i.assignee_id, i.assignee_name, i.creator_type, i.creator_id, i.parent_issue_id, i.project_id, i.position, i.stage, i.start_date, i.due_date, i.metadata, i.properties, (SELECT COUNT(*) FROM comment c WHERE c.issue_id = i.id), i.created_at, i.updated_at";

/// expanded.rs 复用：pub 包装（mod 内函数默认私有）。
pub(crate) fn issue_from_row_pub(r: &Row) -> rusqlite::Result<Issue> {
    issue_from_row(r)
}

pub(crate) fn parse_json_pub(text: &str, fallback: serde_json::Value) -> serde_json::Value {
    parse_json(text, fallback)
}

fn agent_from_row(r: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        provider: r.get(2)?,
        cli: r.get(3)?,
        runtime_id: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        runtime_bound: !r.get::<_, Option<String>>(4)?.unwrap_or_default().is_empty(),
        runtime_availability: "offline".into(),
        name: r.get(5)?,
        description: r.get(6)?,
        instructions: r.get(7)?,
        avatar_url: r.get(8)?,
        runtime_mode: r.get(9)?,
        runtime_config: parse_text(&r.get::<_, String>(10)?, serde_json::json!({})),
        custom_args: parse_text(&r.get::<_, String>(11)?, serde_json::json!([])),
        skills: parse_text(&r.get::<_, String>(12)?, serde_json::json!([])),
        owner_id: r.get(13)?,
        model: r.get(14)?,
        visibility: r.get(15)?,
        max_concurrent_tasks: r.get(16)?,
        status: r.get(17)?,
        archived_at: r.get(18)?,
        created_at: r.get(19)?,
        updated_at: r.get::<_, Option<String>>(20)?.unwrap_or_default(),
    })
}

const AGENT_COLS: &str = "id, workspace_id, provider, cli, runtime_id, name, description, instructions, avatar_url, runtime_mode, runtime_config, custom_args, skills, owner_id, model, visibility, max_concurrent_tasks, status, archived_at, created_at, updated_at";

fn skill_from_row(r: &Row) -> rusqlite::Result<Skill> {
    Ok(Skill {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        name: r.get(2)?,
        description: r.get(3)?,
        content: r.get(4)?,
        config: parse_text(&r.get::<_, String>(5)?, serde_json::json!({})),
        created_by: r.get(6)?,
        created_at: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        files: serde_json::json!([]),
    })
}

fn autopilot_from_row(r: &Row) -> rusqlite::Result<Autopilot> {
    Ok(Autopilot {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        title: r.get(2)?,
        description: r.get(3)?,
        project_id: r.get(4)?,
        assignee_type: r.get(5)?,
        assignee_id: r.get(6)?,
        status: r.get(7)?,
        execution_mode: r.get(8)?,
        issue_title_template: r.get(9)?,
        created_by_type: r.get(10)?,
        created_by_id: r.get(11)?,
        last_run_at: r.get(12)?,
        created_at: r.get(13)?,
        updated_at: r.get(14)?,
    })
}

const AUTOPILOT_COLS: &str = "id, workspace_id, title, description, project_id, assignee_type, assignee_id, status, execution_mode, issue_title_template, created_by_type, created_by_id, last_run_at, created_at, updated_at";

/// 在字符边界处截断（String::truncate / s[..n] 在非边界处会 panic，
/// watcher 线程一旦 panic，run 行永远卡在 running，也没有终态写回）。
fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max { return s; }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) { end -= 1; }
    &s[..end]
}

/// DB 日志上限：防止啰嗦 CLI 把 SQLite 撑爆（流式刷 + 尾巴补齐都受此约束）。
const MAX_DB_LOG: usize = 1_048_576;

/// 后台 spawn 一个 run：写 run 行 → 在 issue 专属 workdir 里 spawn CLI →
/// 等完成写回（终态更新带 `WHERE status='running'`，cancel 后不再覆盖）。
/// 返回 run 行；CLI 未接入时直接写 failed，不假装成功。
pub fn spawn_run(
    app: &tauri::AppHandle,
    db: &Db,
    issue_id: &str,
    agent_id: &str,
    agent_name: &str,
    cli: &str,
    prompt: &str,
) -> Result<Run, String> {
    spawn_run_with_chat(app, db, issue_id, agent_id, agent_name, cli, prompt, None)
}

/// 带 chat 挂钩的 run：chat_session_id 有值时，run 结束把输出作为 assistant 消息追加进会话。
pub fn spawn_run_with_chat(
    app: &tauri::AppHandle,
    db: &Db,
    issue_id: &str,
    agent_id: &str,
    agent_name: &str,
    cli: &str,
    prompt: &str,
    chat_session_id: Option<&str>,
) -> Result<Run, String> {
    let run = Run {
        id: new_id(),
        issue_id: issue_id.to_string(),
        agent_id: agent_id.to_string(),
        agent_name: agent_name.to_string(),
        status: "running".into(),
        started_at: now_iso(),
        finished_at: None,
        log: String::new(),
    };
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO run (id, issue_id, agent_id, agent_name, status, started_at, log, chat_session_id) VALUES (?1,?2,?3,?4,?5,?6,'',?7)",
            params![run.id, if run.issue_id.is_empty() { None } else { Some(run.issue_id.as_str()) }, run.agent_id, run.agent_name, run.status, run.started_at, chat_session_id],
        )
        .map_err(|e| e.to_string())?;
    }
    let run_id = run.id.clone();
    let cli_owned = cli.to_string();
    let prompt_owned = prompt.to_string();
    let chat_owned = chat_session_id.map(|s| s.to_string());
    // 每 issue 一个工作目录（原 daemon 的 worktree 等价：没有可挂的 git 仓库时，
    // 用 app_data 下的独立目录隔离各 issue 的 CLI 副作用，不跑在应用 cwd）。
    // chat 会话没有 issue：用 chat 会话专属目录。
    let workdir = app
        .path()
        .app_data_dir()
        .map(|d| {
            if issue_id.is_empty() {
                d.join("worktrees").join(format!("chat-{}", chat_owned.as_deref().unwrap_or("unknown")))
            } else {
                d.join("worktrees").join(issue_id)
            }
        })
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&workdir);
    let app2 = app.clone();
    std::thread::spawn(move || {
        // 返回 (终态, 完整日志, 已流式刷入 DB 的日志长度)：CLI 未接入时直接失败，不起进程。
        let (status, log, flushed_len) = match crate::daemon::cli_command(&cli_owned, ".", &prompt_owned) {
            None => (
                "failed".to_string(),
                format!("[daemon] CLI `{}` 尚未接入（目前仅 claude / codex / gemini / opencode / aider / copilot）。", cli_owned),
                0usize,
            ),
            Some((bin, args)) => {
                let mut cmd = std::process::Command::new(&bin);
                cmd.args(&args)
                    .current_dir(&workdir)
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                // Unix：新进程组（pgid = 子进程 pid），cancel 时 kill(-pid) 连带杀
                // 掉 CLI spawn 的孙进程；单 kill 只杀父进程，工具子进程会残留。
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    cmd.process_group(0);
                }
                match cmd.spawn()
                {
                    Err(e) => (
                        "failed".to_string(),
                        format!("[daemon] 启动 `{}` 失败：{}（是否已安装并在 PATH 中？）", bin, e),
                        0usize,
                    ),
                    Ok(mut child) => {
                        // stdout/stderr 读线程：行缓冲 → 共享 buffer，后台轮询刷进 DB（流式日志）。
                        let buf: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
                            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
                        let mut readers = Vec::new();
                        for stream in [child.stdout.take().map(|x| Box::new(x) as Box<dyn std::io::Read + Send>), child.stderr.take().map(|x| Box::new(x) as Box<dyn std::io::Read + Send>)] {
                            let Some(stream) = stream else { continue };
                            let buf2 = buf.clone();
                            readers.push(std::thread::spawn(move || {
                                use std::io::BufRead;
                                let reader = std::io::BufReader::new(stream);
                                for line in reader.lines() {
                                    match line {
                                        Ok(l) => buf2.lock().unwrap().push(l),
                                        Err(_) => break,
                                    }
                                }
                            }));
                        }
                        // Child 留在 map 里直到退出：cancel_run 才能在运行期间拿到它真 kill。
                        // 注意：这里只短暂持锁插入，不在持锁状态下等待。
                        if let Some(children) = app2.try_state::<crate::daemon::RunChildren>() {
                            children.0.lock().unwrap().insert(run_id.clone(), child);
                        }
                        // try_wait 轮询：每次只短暂持锁，cancel 的 kill 才能插进来。
                        let mut exit_status: Option<std::process::ExitStatus> = None;
                        let mut last_emit = std::time::Instant::now();
                        let mut log_acc = String::new();
                        let mut log_capped = false;
                        let mut flushed_len = 0usize;
                        loop {
                            {
                                let Some(children) = app2.try_state::<crate::daemon::RunChildren>() else { break };
                                let mut guard = children.0.lock().unwrap();
                                if let Some(c) = guard.get_mut(&run_id) {
                                    match c.try_wait() {
                                        Ok(Some(st)) => { exit_status = Some(st); }
                                        Ok(None) => {}
                                        Err(_) => { exit_status = None; }
                                    }
                                } else {
                                    break; // map 里没了：已被 cancel 侧取走（不应发生）或状态丢失
                                }
                            }
                            if exit_status.is_some() { break; }
                            // 刷流式日志进 DB（节流 emit，避免每行都刷缓存失效）
                            let mut chunk = String::new();
                            {
                                let mut b = buf.lock().unwrap();
                                for l in b.drain(..) {
                                    chunk.push_str(&l);
                                    chunk.push('\n');
                                }
                            }
                            if !chunk.is_empty() {
                                if !log_capped {
                                    log_acc.push_str(&chunk);
                                    if log_acc.len() > MAX_DB_LOG {
                                        log_acc = format!("{}…[日志超 1MB，已截断]", truncate_utf8(&log_acc, MAX_DB_LOG));
                                        log_capped = true;
                                    }
                                }
                                // DB 侧只写预算内的前缀（chunk 逐段顺序到达，前缀拼接 = 流前缀）。
                                if flushed_len < MAX_DB_LOG {
                                    let to_write = truncate_utf8(&chunk, MAX_DB_LOG - flushed_len);
                                    if !to_write.is_empty() {
                                        flushed_len += to_write.len();
                                        if let Some(db) = app2.try_state::<Db>() {
                                            let conn = db.0.lock().unwrap();
                                            let _ = conn.execute(
                                                "UPDATE run SET log = log || ?1 WHERE id = ?2",
                                                params![to_write, run_id],
                                            );
                                        }
                                    }
                                }
                                if last_emit.elapsed().as_millis() >= 1000 {
                                    emit_changed(&app2, "run", &run_id);
                                    last_emit = std::time::Instant::now();
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_millis(150));
                        }
                        // 进程已退出：从 map 移除，读线程读完 EOF 后 join，刷剩余日志。
                        if let Some(children) = app2.try_state::<crate::daemon::RunChildren>() {
                            children.0.lock().unwrap().remove(&run_id);
                        }
                        for r in readers {
                            let _ = r.join();
                        }
                        let mut tail = String::new();
                        {
                            let mut b = buf.lock().unwrap();
                            for l in b.drain(..) {
                                tail.push_str(&l);
                                tail.push('\n');
                            }
                        }
                        if !log_capped {
                            log_acc.push_str(&tail);
                            if log_acc.len() > MAX_DB_LOG {
                                log_acc = format!("{}…[日志超 1MB，已截断]", truncate_utf8(&log_acc, MAX_DB_LOG));
                            }
                        }
                        if log_acc.trim().is_empty() {
                            log_acc = "[daemon] CLI 无输出。".to_string();
                        }
                        let final_status = match exit_status {
                            Some(st) if st.success() => "succeeded".to_string(),
                            Some(_) => "failed".to_string(),
                            None => "failed".to_string(),
                        };
                        (final_status, log_acc, flushed_len)
                    }
                }
            }
        };
        if let Some(db) = app2.try_state::<Db>() {
            let conn = db.0.lock().unwrap();
            // 补尾巴：流式已刷的前缀不再重复（失败路径 flushed_len=0，全量追加）。
            // remainder 同样受 1MB 预算约束。
            let remainder = if flushed_len <= log.len() { &log[flushed_len..] } else { "" };
            let remainder = truncate_utf8(remainder, MAX_DB_LOG.saturating_sub(flushed_len.min(MAX_DB_LOG)));
            if !remainder.is_empty() {
                let _ = conn.execute(
                    "UPDATE run SET log = log || ?1 WHERE id = ?2",
                    params![remainder, run_id],
                );
            }
            // 终态守卫：cancel_run 已把 status 置 cancelled 时，不再覆盖终态。
            let _ = conn.execute(
                "UPDATE run SET status = ?1, finished_at = ?2 WHERE id = ?3 AND status = 'running'",
                params![status, now_iso(), run_id],
            );
            // chat 挂钩：run 结束，把输出作为 assistant 消息追加进会话。
            if let Some(session_id) = &chat_owned {
                let db_status: String = conn.query_row(
                    "SELECT status FROM run WHERE id = ?1",
                    params![run_id],
                    |r| r.get(0),
                ).unwrap_or_else(|_| status.clone());
                let content = if db_status == "cancelled" {
                    "[已取消]".to_string()
                } else if log.len() > 8192 {
                    format!("{}…[截断]", truncate_utf8(&log, 8192))
                } else {
                    log.clone()
                };
                let _ = conn.execute(
                    "INSERT INTO chat_message (id, chat_session_id, role, content, task_id, created_at) VALUES (?1,?2,'assistant',?3,?4,?5)",
                    params![new_id(), session_id, content, run_id, now_iso()],
                );
                let _ = conn.execute(
                    "UPDATE chat_session SET updated_at = ?1 WHERE id = ?2",
                    params![now_iso(), session_id],
                );
            }
        }
        emit_changed(&app2, "run", &run_id);
        if chat_owned.is_some() {
            emit_changed(&app2, "chat", chat_owned.as_deref().unwrap_or(""));
        }
    });
    Ok(run)
}

/// issue 指派给 agent 时的 run：prompt 取 issue 标题 + body。
fn maybe_spawn_for_assignee(
    app: &tauri::AppHandle,
    db: &Db,
    issue_id: &str,
    assignee_type: &Option<String>,
    assignee_id: &Option<String>,
) {
    if assignee_type.as_deref() != Some("agent") {
        return;
    }
    let Some(agent_id) = assignee_id.clone() else { return };
    let (title, body, agent_name, cli) = {
        let conn = db.0.lock().unwrap();
        let issue = conn.query_row(
            "SELECT title, body FROM issue WHERE id = ?1",
            params![issue_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        );
        let agent = conn.query_row(
            "SELECT name, cli FROM agent WHERE id = ?1",
            params![agent_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        );
        match (issue, agent) {
            (Ok((t, b)), Ok((n, c))) => (t, b, n, c),
            _ => return,
        }
    };
    let prompt = format!("Issue: {}\n\n{}\n\n请完成该 issue，必要时修改工作目录下的文件，并用简洁中文总结做了什么。", title, body);
    let _ = spawn_run(app, db, issue_id, &agent_id, &agent_name, &cli, &prompt);
}

// ── Workspace ──
#[tauri::command]
pub fn list_workspaces(db: State<Db>) -> Result<Vec<Workspace>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, slug, name, created_at FROM workspace ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(Workspace {
        id: r.get(0)?, slug: r.get(1)?, name: r.get(2)?, created_at: r.get(3)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workspace(db: State<Db>, id: String) -> Result<Option<Workspace>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, slug, name, created_at FROM workspace WHERE id = ?1 OR slug = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], |r| Ok(Workspace {
        id: r.get(0)?, slug: r.get(1)?, name: r.get(2)?, created_at: r.get(3)?,
    })).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_workspace(app: tauri::AppHandle, db: State<Db>, name: String, slug: Option<String>) -> Result<Workspace, String> {
    let slug = slug.unwrap_or_else(|| name.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "-"));
    let ws = Workspace { id: new_id(), slug, name, created_at: now_iso() };
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT INTO workspace (id, slug, name, created_at) VALUES (?1,?2,?3,?4)",
        params![ws.id, ws.slug, ws.name, ws.created_at]).map_err(|e| e.to_string())?;
    emit_changed(&app, "workspace", &ws.id);
    Ok(ws)
}

// ── Issue（看板核心） ──
#[tauri::command]
pub fn list_issues(db: State<Db>, workspace_id: String) -> Result<Vec<Issue>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM issue i WHERE i.workspace_id = ?1 ORDER BY i.updated_at DESC", ISSUE_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], issue_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_issue(db: State<Db>, id: String) -> Result<Option<Issue>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM issue i WHERE i.id = ?1 OR i.identifier = ?1", ISSUE_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], issue_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_issue(
    app: tauri::AppHandle,
    db: State<Db>,
    workspace_id: String,
    title: String,
    body: Option<String>,
    priority: Option<String>,
    description: Option<String>,
    project_id: Option<String>,
    assignee_type: Option<String>,
    assignee_id: Option<String>,
    status: Option<String>,
) -> Result<Issue, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let (number, identifier) = {
        let conn = db.0.lock().unwrap();
        let max: i64 = conn.query_row(
            "SELECT COALESCE(MAX(number), 0) FROM issue WHERE workspace_id = ?1",
            params![workspace_id],
            |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        (max + 1, format!("MUL-{}", max + 1))
    };
    let body = body.or(description).unwrap_or_default();
    let status = status.unwrap_or_else(|| "backlog".to_string());
    let status_category = match status.as_str() {
        "done" | "cancelled" => "done",
        "in_progress" | "in_review" | "blocked" => "started",
        _ => "unstarted",
    };
    let issue = Issue {
        id: new_id(), workspace_id: workspace_id.clone(), number, identifier,
        title, body: body.clone(), description: Some(body.clone()), status: status.clone(),
        status_category: status_category.into(), priority: priority.unwrap_or_else(|| "medium".into()),
        assignee_type: assignee_type.clone(), assignee_id: assignee_id.clone(), assignee_name: None,
        creator_type: Some("member".into()), creator_id: Some("u-local".into()),
        parent_issue_id: None, project_id, position: number, stage: None,
        start_date: None, due_date: None, labels: serde_json::json!([]),
        metadata: serde_json::json!({}), properties: serde_json::json!({}),
        comments_count: 0, created_at: now.clone(), updated_at: now,
    };
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO issue (id, workspace_id, number, identifier, title, body, status, status_category, priority, assignee_type, assignee_id, creator_type, creator_id, project_id, position, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?16)",
            params![issue.id, issue.workspace_id, issue.number, issue.identifier, issue.title, issue.body, issue.status, issue.status_category, issue.priority, issue.assignee_type, issue.assignee_id, issue.creator_type, issue.creator_id, issue.project_id, issue.position, issue.created_at],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "issue", &issue.id);
    maybe_spawn_for_assignee(&app, &db, &issue.id, &assignee_type, &assignee_id);
    Ok(issue)
}

/// 部分更新：只更新传入的字段（None = 不动）。assignee 切到 agent 时 spawn run。
#[tauri::command]
pub fn update_issue(
    app: tauri::AppHandle,
    db: State<Db>,
    id: String,
    title: Option<String>,
    body: Option<String>,
    description: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    assignee_type: Option<String>,
    assignee_id: Option<String>,
    project_id: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "title", title);
    let body_val = body.or(description);
    push_set(&mut sets, &mut vals, "body", body_val);
    push_set(&mut sets, &mut vals, "priority", priority);
    push_set(&mut sets, &mut vals, "status", status.clone());
    if status.is_some() {
        let cat = match status.as_deref() {
            Some("done") | Some("cancelled") => "done",
            Some("in_progress") | Some("in_review") | Some("blocked") => "started",
            _ => "unstarted",
        };
        sets.push("status_category = ?".into());
        vals.push(cat.to_string().into());
    }
    push_set(&mut sets, &mut vals, "assignee_type", assignee_type.clone());
    push_set(&mut sets, &mut vals, "assignee_id", assignee_id.clone());
    push_set(&mut sets, &mut vals, "project_id", project_id);
    vals.push(id.clone().into());
    let sql = format!("UPDATE issue SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "issue", &id);
    // assignee 变化：读回当前 assignee，agent 则 spawn。
    let (at, ai) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT assignee_type, assignee_id FROM issue WHERE id = ?1", params![id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
        }).unwrap_or((None, None))
    };
    // 只有本次传入了 assignee 才触发（避免标题编辑误 spawn）。
    if assignee_type.is_some() || assignee_id.is_some() {
        maybe_spawn_for_assignee(&app, &db, &id, &at, &ai);
    }
    Ok(())
}

#[tauri::command]
pub fn update_issue_status(app: tauri::AppHandle, db: State<Db>, id: String, status: String) -> Result<(), String> {
    let cat = match status.as_str() {
        "done" | "cancelled" => "done",
        "in_progress" | "in_review" | "blocked" => "started",
        _ => "unstarted",
    };
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE issue SET status = ?1, status_category = ?2, updated_at = ?3 WHERE id = ?4",
        params![status, cat, now_iso(), id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &id);
    Ok(())
}


#[tauri::command]
pub fn delete_issue(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM comment WHERE issue_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM issue_label WHERE issue_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM wakeup WHERE issue_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM attachment WHERE issue_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM run WHERE issue_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM issue WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &id);
    Ok(())
}

// ── Comment（core Comment 形状：content / author_id / type / parent_id） ──
fn comment_from_row(r: &Row) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: r.get(0)?, issue_id: r.get(1)?, author_type: r.get(2)?, author_id: r.get(3)?,
        content: r.get(4)?, comment_type: r.get(5)?, parent_id: r.get(6)?,
        reactions: serde_json::json!([]), attachments: serde_json::json!([]),
        created_at: r.get(7)?, updated_at: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        resolved_at: r.get(9)?, resolved_by_type: r.get(10)?, resolved_by_id: r.get(11)?,
    })
}

const COMMENT_COLS: &str = "id, issue_id, author_type, author_id, content, type, parent_id, created_at, updated_at, resolved_at, resolved_by_type, resolved_by_id";

#[tauri::command]
pub fn list_comments(db: State<Db>, issue_id: String) -> Result<Vec<Comment>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM comment WHERE issue_id = ?1 ORDER BY created_at", COMMENT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], comment_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_comment(app: tauri::AppHandle, db: State<Db>, issue_id: String, content: String, comment_type: Option<String>, parent_id: Option<String>) -> Result<Comment, String> {
    let now = now_iso();
    let c = Comment {
        id: new_id(), issue_id, author_type: "member".into(), author_id: "u-local".into(),
        content, comment_type: comment_type.unwrap_or_else(|| "comment".into()), parent_id,
        reactions: serde_json::json!([]), attachments: serde_json::json!([]),
        created_at: now.clone(), updated_at: now, resolved_at: None, resolved_by_type: None, resolved_by_id: None,
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO comment (id, issue_id, author_type, author_name, body, author_id, content, type, parent_id, created_at, updated_at) VALUES (?1,?2,?3,'','',?4,?5,?6,?7,?8,?8)",
        params![c.id, c.issue_id, c.author_type, c.author_id, c.content, c.comment_type, c.parent_id, c.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &c.id);
    Ok(c)
}

#[tauri::command]
pub fn update_comment(app: tauri::AppHandle, db: State<Db>, id: String, content: String) -> Result<Option<Comment>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE comment SET content = ?1, updated_at = ?2 WHERE id = ?3",
        params![content, now_iso(), id]).map_err(|e| e.to_string())?;
    let sql = format!("SELECT {} FROM comment WHERE id = ?1", COMMENT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], comment_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn delete_comment(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM comment WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &id);
    Ok(())
}

// ── Agent / Runtime ──
#[tauri::command]
pub fn list_agents(db: State<Db>, workspace_id: String) -> Result<Vec<Agent>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM agent WHERE workspace_id = ?1 ORDER BY created_at", AGENT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], agent_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_agent(db: State<Db>, id: String) -> Result<Option<Agent>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM agent WHERE id = ?1", AGENT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], agent_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_agent(
    app: tauri::AppHandle,
    db: State<Db>,
    workspace_id: String,
    name: String,
    runtime_id: Option<String>,
    description: Option<String>,
    instructions: Option<String>,
    avatar_url: Option<String>,
    model: Option<String>,
    owner_id: Option<String>,
    cli: Option<String>,
    runtime_config: Option<String>,
    max_concurrent_tasks: Option<i64>,
) -> Result<Agent, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    // CLI：表单不传时取本机第一个已安装的 CLI（与“绑定本机 runtime”一致）；
    // 一个都没装时默认 claude，run 时会明确报“未接入/未安装”而不是静默失败。
    let cli = cli.unwrap_or_else(|| {
        crate::daemon::detect_clis().into_iter().next().unwrap_or_else(|| "claude".into())
    });
    let a = Agent {
        id: new_id(), workspace_id, provider: cli.clone(), cli,
        runtime_id: runtime_id.unwrap_or_else(|| "local".into()), runtime_bound: false,
        runtime_availability: "offline".into(), name,
        description: description.unwrap_or_default(),
        instructions: instructions.unwrap_or_default(), avatar_url,
        runtime_mode: "local".into(),
        runtime_config: parse_json(&runtime_config.unwrap_or_else(|| "{}".into()), serde_json::json!({})),
        custom_args: serde_json::json!([]), skills: serde_json::json!([]),
        owner_id, model: model.unwrap_or_default(), visibility: "workspace".into(),
        max_concurrent_tasks: max_concurrent_tasks.unwrap_or(1), status: "offline".into(), archived_at: None,
        created_at: now.clone(), updated_at: now,
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO agent (id, workspace_id, name, provider, cli, runtime_id, status, description, instructions, runtime_mode, runtime_config, custom_args, skills, owner_id, model, visibility, max_concurrent_tasks, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?18)",
        params![a.id, a.workspace_id, a.name, a.provider, a.cli, a.runtime_id, a.status, a.description, a.instructions, a.runtime_mode, json_text(&a.runtime_config), json_text(&a.custom_args), json_text(&a.skills), a.owner_id, a.model, a.visibility, a.max_concurrent_tasks, a.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &a.id);
    Ok(a)
}

#[tauri::command]
pub fn update_agent(
    app: tauri::AppHandle,
    db: State<Db>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    instructions: Option<String>,
    runtime_id: Option<String>,
    model: Option<String>,
    status: Option<String>,
    avatar_url: Option<String>,
    runtime_config: Option<String>,
    max_concurrent_tasks: Option<i64>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "name", name);
    push_set(&mut sets, &mut vals, "description", description);
    push_set(&mut sets, &mut vals, "instructions", instructions);
    push_set(&mut sets, &mut vals, "runtime_id", runtime_id);
    push_set(&mut sets, &mut vals, "model", model);
    push_set(&mut sets, &mut vals, "status", status);
    push_set(&mut sets, &mut vals, "avatar_url", avatar_url);
    push_set(&mut sets, &mut vals, "runtime_config", runtime_config);
    if let Some(n) = max_concurrent_tasks {
        sets.push("max_concurrent_tasks = ?".into());
        vals.push(n.into());
    }
    vals.push(id.clone().into());
    let sql = format!("UPDATE agent SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_agent(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM agent WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &id);
    Ok(())
}

#[tauri::command]
pub fn list_runtimes(db: State<Db>) -> Result<Vec<Runtime>, String> {
    // 自定义名存在 workspace.settings.runtime_custom_name（update_runtime 写入）。
    let name = {
        let conn = db.0.lock().unwrap();
        let raw: String = conn.query_row("SELECT COALESCE(settings, '{}') FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get(0)).unwrap_or_else(|_| "{}".to_string());
        parse_json_pub(&raw, serde_json::json!({})).get("runtime_custom_name").and_then(|v| v.as_str()).unwrap_or("本机").to_string()
    };
    Ok(vec![Runtime {
        id: "local".into(), name, kind: "local".into(),
        status: "online".into(), detected_clis: crate::daemon::detect_clis(),
    }])
}

// ── Run：指派后执行（真实 spawn，后台线程 + 日志落盘，前端轮询 get_run / 事件） ──
// 原 Go：daemon 在 worktree 里 spawn CLI，日志实时流回后端。本版：Tauri 进程内即
// daemon，在 worktrees/<issue_id>/ 里 spawn CLI；cancel_run 真 kill 子进程。


/// cancel 的共享实现：kill 子进程 + 置 cancelled（带终态守卫）。
pub(crate) fn cancel_run_by_id(app: &tauri::AppHandle, db: &Db, id: &str) -> Result<(), String> {
    // 只 kill，不 remove、不 wait：Child 留在 map 里，spawn 的 try_wait 轮询会 reap。
    // 旧实现 remove 后 wait_with_output 会把 map 锁占满整个运行期，cancel 永远 kill 不到。
    if let Some(children) = app.try_state::<crate::daemon::RunChildren>() {
        let mut guard = children.0.lock().unwrap();
        if let Some(child) = guard.get_mut(id) {
            // 杀整个进程组（spawn 时 process_group(0)，pgid = 子进程 pid）。
            // 失败（子进程已退出等）回退单进程 kill。
            #[cfg(unix)]
            {
                let pid = child.id() as i32;
                let killed = unsafe { libc::kill(-pid, libc::SIGKILL) } == 0;
                if !killed { let _ = child.kill(); }
            }
            #[cfg(not(unix))]
            {
                let _ = child.kill();
            }
        }
    }
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE run SET status = 'cancelled', finished_at = ?1 WHERE id = ?2 AND status = 'running'",
        params![now_iso(), id]).map_err(|e| e.to_string())?;
    Ok(())
}



// ── Skill ──
#[tauri::command]
pub fn list_skills(db: State<Db>, workspace_id: String) -> Result<Vec<Skill>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, description, content, config, created_by, created_at, updated_at FROM skill WHERE workspace_id = ?1 AND archived_at IS NULL ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], skill_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_skill(db: State<Db>, id: String) -> Result<Option<Skill>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, description, content, config, created_by, created_at, updated_at FROM skill WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], skill_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_skill(app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String, description: String, content: String) -> Result<Skill, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let s = Skill { id: new_id(), workspace_id, name, description, content, config: serde_json::json!({}), created_by: Some("u-local".into()), created_at: now.clone(), updated_at: now.clone(), files: serde_json::json!([]) };
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT INTO skill (id, workspace_id, name, description, content, config, created_by, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
        params![s.id, s.workspace_id, s.name, s.description, s.content, json_text(&s.config), s.created_by, s.created_at]).map_err(|e| e.to_string())?;
    emit_changed(&app, "skill", &s.id);
    Ok(s)
}

#[tauri::command]
pub fn update_skill(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, description: Option<String>, content: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "name", name);
    push_set(&mut sets, &mut vals, "description", description);
    push_set(&mut sets, &mut vals, "content", content);
    vals.push(id.clone().into());
    let sql = format!("UPDATE skill SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "skill", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_skill(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM skill WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "skill", &id);
    Ok(())
}

// ── Autopilot ──
#[tauri::command]
pub fn list_autopilots(db: State<Db>, workspace_id: String) -> Result<ListAutopilotsResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM autopilot WHERE workspace_id = ?1 ORDER BY created_at", AUTOPILOT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], autopilot_from_row).map_err(|e| e.to_string())?;
    let autopilots = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = autopilots.len() as i64;
    Ok(ListAutopilotsResponse { autopilots, total })
}

#[tauri::command]
pub fn get_autopilot(db: State<Db>, id: String) -> Result<Option<GetAutopilotResponse>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM autopilot WHERE id = ?1", AUTOPILOT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], autopilot_from_row).map_err(|e| e.to_string())?;
    match rows.next() {
        Some(r) => Ok(Some(GetAutopilotResponse { autopilot: r.map_err(|e| e.to_string())?, triggers: vec![], collaborators: vec![] })),
        None => Ok(None),
    }
}

#[tauri::command]
pub fn create_autopilot(
    app: tauri::AppHandle,
    db: State<Db>,
    workspace_id: String,
    title: String,
    description: Option<String>,
    project_id: Option<String>,
    assignee_type: Option<String>,
    assignee_id: String,
    execution_mode: String,
    issue_title_template: Option<String>,
) -> Result<Autopilot, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let a = Autopilot {
        id: new_id(), workspace_id, title, description, project_id,
        assignee_type: assignee_type.unwrap_or_else(|| "agent".into()), assignee_id,
        status: "active".into(), execution_mode, issue_title_template,
        created_by_type: "member".into(), created_by_id: "u-local".into(),
        last_run_at: None, created_at: now.clone(), updated_at: now,
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO autopilot (id, workspace_id, title, description, project_id, assignee_type, assignee_id, status, execution_mode, issue_title_template, created_by_type, created_by_id, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
        params![a.id, a.workspace_id, a.title, a.description, a.project_id, a.assignee_type, a.assignee_id, a.status, a.execution_mode, a.issue_title_template, a.created_by_type, a.created_by_id, a.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &a.id);
    Ok(a)
}

#[tauri::command]
pub fn update_autopilot(
    app: tauri::AppHandle,
    db: State<Db>,
    id: String,
    title: Option<String>,
    description: Option<String>,
    status: Option<String>,
    assignee_id: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "title", title);
    push_set(&mut sets, &mut vals, "description", description);
    push_set(&mut sets, &mut vals, "status", status);
    push_set(&mut sets, &mut vals, "assignee_id", assignee_id);
    vals.push(id.clone().into());
    let sql = format!("UPDATE autopilot SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_autopilot(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM autopilot_run WHERE autopilot_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM autopilot WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &id);
    Ok(())
}


#[tauri::command]
pub fn list_autopilot_runs(db: State<Db>, autopilot_id: String) -> Result<ListAutopilotRunsResponse, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, autopilot_id, status, issue_id, started_at, finished_at, log FROM autopilot_run WHERE autopilot_id = ?1 ORDER BY started_at DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([autopilot_id], |r| Ok(AutopilotRun {
        id: r.get(0)?, autopilot_id: r.get(1)?, status: r.get(2)?, issue_id: r.get(3)?,
        started_at: r.get(4)?, finished_at: r.get(5)?, log: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    let runs = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = runs.len() as i64;
    Ok(ListAutopilotRunsResponse { runs, total })
}

// ── Auth（本地） ──
#[tauri::command]
pub fn auth_has_password(db: State<Db>) -> Result<bool, String> {
    Ok(crate::auth::has_password(&db))
}

#[tauri::command]
pub fn auth_set_password(db: State<Db>, password: String) -> Result<(), String> {
    crate::auth::set_password(&db, &password).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn auth_unlock(db: State<Db>, password: String) -> Result<bool, String> {
    Ok(crate::auth::verify(&db, &password))
}
