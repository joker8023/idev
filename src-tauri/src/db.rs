use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct Db(pub Mutex<Connection>);

/// 数据库文件：Tauri app_data_dir/multica.db（本地 SQLite，替代原 Postgres 17）
/// 原 Go 用 pgx + sqlc + 1196 个迁移文件；本地版只保留核心表，见 migrations/001_init.sql。
pub fn db_path(app: &AppHandle) -> anyhow::Result<PathBuf> {
    let dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("multica.db"))
}

pub fn init_db(app: &AppHandle) -> anyhow::Result<Db> {
    let path = db_path(app)?;
    let conn = Connection::open(&path)?;
    apply_migrations(&conn)?;
    // 首次启动种子：一个默认 workspace + 两个示例 agent + 三条示例 issue，
    // 样式与原 Multica 首次进入看板一致，便于直接对照。
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM workspace",
        [],
        |r| r.get(0),
    )?;
    if count == 0 {
        seed(&conn)?;
    }
    // 回填：001 时期已存在的 issue 没有 identifier/number（002 新增列默认 ''/0），
    // 按 workspace 内 number 顺序补齐，否则命令面板 identifier.toLowerCase() 会 crash。
    backfill_issue_ids(&conn)?;
    // 本地成员：单机单用户，workspace 永远有本地用户这个 owner 成员。
    seed_member(&conn)?;
    Ok(Db(Mutex::new(conn)))
}

fn has_table(conn: &Connection, table: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        params![table],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

fn column_notnull(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = match conn.prepare(&format!("PRAGMA table_info({})", table)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let rows = match stmt.query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(3)?))) {
        Ok(rows) => rows,
        Err(_) => return false,
    };
    let found = rows.flatten().any(|(name, notnull)| name == column && notnull == 1);
    found
}

fn has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let mut stmt = match conn.prepare(&format!("PRAGMA table_info({})", table)) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let rows = match stmt.query_map([], |r| r.get::<_, String>(1)) {
        Ok(rows) => rows,
        Err(_) => return false,
    };
    let found = rows.flatten().any(|c| c == column);
    found
}

/// 版本化迁移：只执行未应用的迁移。旧库（无版本表但 schema 已是新版）
/// 用“效果已存在”检测跳过执行、只记版本，避免 002–004 的 ALTER 重复列失败，
/// 以及 002 的 DROP TABLE autopilot 在每次启动时丢数据。
fn apply_migrations(conn: &Connection) -> anyhow::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migration (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)",
        [],
    )?;
    let migrations: [(i64, &str, fn(&Connection) -> bool); 7] = [
        (1, include_str!("../migrations/001_init.sql"), |c| has_table(c, "workspace")),
        (2, include_str!("../migrations/002_expand.sql"), |c| has_column(c, "issue", "identifier")),
        (3, include_str!("../migrations/003_wakeup.sql"), |c| has_column(c, "wakeup", "origin")),
        (4, include_str!("../migrations/004_comment_wakeup.sql"), |c| has_column(c, "comment", "content")),
        (5, include_str!("../migrations/005_write_paths.sql"), |c| has_table(c, "issue_property_value")),
        (6, include_str!("../migrations/006_schema_fixes.sql"), |c| {
            column_notnull(c, "system_wakeup", "issue_id")
                && column_notnull(c, "issue_view_preference", "scope_id")
                && !column_notnull(c, "run", "issue_id")
        }),
        (7, include_str!("../migrations/007_batch3.sql"), |c| {
            has_table(c, "workspace_mcp_server")
                && has_table(c, "agent_mcp_server")
                && has_table(c, "runtime_profile")
                && has_table(c, "runtime_job")
                && has_column(c, "workspace", "settings")
        }),
    ];
    for (version, sql, already_applied) in migrations {
        let recorded: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migration WHERE version = ?1)",
            params![version],
            |r| r.get(0),
        )?;
        if recorded {
            continue;
        }
        if !already_applied(conn) {
            conn.execute_batch(sql)?;
        }
        // v7 的 settings 列：SQLite ALTER ADD COLUMN 无 IF NOT EXISTS，
        // 不能放进 007 文件（partial 迁移会让每次启动重试 ALTER 都失败）。
        // 这里守卫：缺列才加，已有列跳过；与 007 的 already_applied 检测一致。
        if version == 7 && !has_column(conn, "workspace", "settings") {
            conn.execute(
                "ALTER TABLE workspace ADD COLUMN settings TEXT NOT NULL DEFAULT '{}'",
                [],
            )?;
        }
        conn.execute(
            "INSERT INTO schema_migration (version, applied_at) VALUES (?1, ?2)",
            params![version, crate::models::now_iso()],
        )?;
    }
    Ok(())
}

fn backfill_issue_ids(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id FROM issue WHERE identifier = '' ORDER BY workspace_id, created_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut pending: Vec<(String, String)> = vec![];
    for row in rows {
        pending.push(row?);
    }
    drop(stmt);
    for (id, ws) in pending {
        let max: i64 = conn.query_row(
            "SELECT COALESCE(MAX(number), 0) FROM issue WHERE workspace_id = ?1",
            params![ws],
            |r| r.get(0),
        )?;
        let n = max + 1;
        conn.execute(
            "UPDATE issue SET number = ?1, identifier = ?2 WHERE id = ?3",
            params![n, format!("MUL-{}", n), id],
        )?;
    }
    Ok(())
}

fn seed_member(conn: &Connection) -> anyhow::Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM workspace")?;
    let ws_ids = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut ids: Vec<String> = vec![];
    for row in ws_ids {
        ids.push(row?);
    }
    drop(stmt);
    for ws in ids {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM member WHERE workspace_id = ?1",
            params![ws],
            |r| r.get(0),
        )?;
        if count == 0 {
            conn.execute(
                "INSERT INTO member (id, workspace_id, user_id, name, email, role, created_at) VALUES (?1,?2,'u-local','本地用户','local@multica.local','owner',?3)",
                params![crate::models::new_id(), ws, crate::models::now_iso()],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migrations_are_idempotent_and_preserve_autopilot() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        // 模拟已有数据：autopilot 行 + issue
        conn.execute(
            "INSERT INTO autopilot (id, workspace_id, title, status, created_at, updated_at) VALUES ('ap1','ws1','t','active','2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM autopilot WHERE id = 'ap1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "重复执行迁移不应丢 autopilot 数据");
        let v: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migration", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 7);
    }

    #[test]
    fn legacy_db_without_version_table_upgrades_cleanly() {
        // 旧库：已执行 001+002，但无版本表
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/001_init.sql")).unwrap();
        conn.execute_batch(include_str!("../migrations/002_expand.sql")).unwrap();
        conn.execute(
            "INSERT INTO autopilot (id, workspace_id, title, status, created_at, updated_at) VALUES ('ap1','ws1','t','active','2026-01-01','2026-01-01')",
            [],
        )
        .unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM autopilot WHERE id = 'ap1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "旧库升级不应丢 autopilot 数据");
        assert!(has_column(&conn, "comment", "content"));
        assert!(has_column(&conn, "wakeup", "origin"));
    }
}

fn seed(conn: &Connection) -> anyhow::Result<()> {
    let ws = crate::models::new_id();
    let now = crate::models::now_iso();
    conn.execute(
        "INSERT INTO workspace (id, slug, name, created_at) VALUES (?1,?2,?3,?4)",
        params![ws, "default", "默认工作区", now],
    )?;
    for (name, provider, cli) in [
        ("Claude", "claude", "claude"),
        ("Codex", "codex", "codex"),
    ] {
        conn.execute(
            "INSERT INTO agent (id, workspace_id, name, provider, cli, status, created_at) VALUES (?1,?2,?3,?4,?5,'offline',?6)",
            params![crate::models::new_id(), ws, name, provider, cli, now],
        )?;
    }
    for (i, (title, status, priority)) in [
        ("示例：把登录页改成深色模式", "todo", "medium"),
        ("示例：给 Agent 加一个 review gate", "in_progress", "high"),
        ("示例：修掉看板拖拽抖动", "backlog", "low"),
    ]
    .into_iter()
    .enumerate()
    {
        let n = (i + 1) as i64;
        conn.execute(
            "INSERT INTO issue (id, workspace_id, identifier, number, title, body, status, priority, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,'',?6,?7,?8,?8)",
            params![crate::models::new_id(), ws, format!("MUL-{}", n), n, title, status, priority, now],
        )?;
    }
    Ok(())
}
