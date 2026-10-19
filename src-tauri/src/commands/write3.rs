//! 第三批写路径：Batch 3 缺失方法（archiveAgent、deleteAttachment、getLabel、
//! 归档 inbox 分页/facets、分组 issues、quick-create、renderQuickAction、
//! replayAutopilotDelivery、prioritizeQueuedChatTask、unsubscribeSubtree、
//! member/workspace CRUD、runtime、runtime profile、MCP 服务器、技能导入、
//! cron 预览、运行时用量、配额、models/local-skills 任务）。
//!
//! 形状约定：与 vendor/core/types 对齐；本地没有的数据（token 用量、优先级
//! 透传等）返回 0/空并在 docs/MIGRATION.md 注明，不编造。

use rusqlite::{params, OptionalExtension};
use tauri::Manager;
use crate::db::Db;
use crate::models::{new_id, now_iso, InboxItem};
use super::{emit_changed, issue_from_row_pub, parse_json_pub, resolve_workspace_id, ISSUE_COLS};

fn get_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(|s| s.to_string())
}
fn get_arr<'a>(v: &'a serde_json::Value, key: &str) -> Vec<&'a serde_json::Value> {
    v.get(key).and_then(|x| x.as_array()).map(|a| a.iter().collect()).unwrap_or_default()
}
fn get_bool(v: &serde_json::Value, key: &str) -> Option<bool> {
    v.get(key).and_then(|x| x.as_bool())
}

// ── Agent 归档 / 恢复 ──────────────────────────────────────────────────────
#[tauri::command]
pub fn archive_agent(app: tauri::AppHandle, db: tauri::State<Db>, id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let now = now_iso();
    let n = conn.execute("UPDATE agent SET archived_at = ?1, updated_at = ?1 WHERE id = ?2", params![now, id]).map_err(|e| e.to_string())?;
    if n == 0 { return Err(format!("agent {} 不存在", id)); }
    drop(conn);
    emit_changed(&app, "agent", &id);
    super::get_agent(db, id).map(|a| serde_json::to_value(a).map_err(|e| e.to_string())).and_then(|r| r)
}
#[tauri::command]
pub fn restore_agent(app: tauri::AppHandle, db: tauri::State<Db>, id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let now = now_iso();
    let n = conn.execute("UPDATE agent SET archived_at = NULL, updated_at = ?1 WHERE id = ?2", params![now, id]).map_err(|e| e.to_string())?;
    if n == 0 { return Err(format!("agent {} 不存在", id)); }
    drop(conn);
    emit_changed(&app, "agent", &id);
    super::get_agent(db, id).map(|a| serde_json::to_value(a).map_err(|e| e.to_string())).and_then(|r| r)
}

// ── 附件删除 ───────────────────────────────────────────────────────────────
#[tauri::command]
pub fn delete_attachment(app: tauri::AppHandle, db: tauri::State<Db>, id: String) -> Result<(), String> {
    let path: Option<String> = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT path FROM attachment WHERE id = ?1", params![id], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten()
    };
    {
        let conn = db.0.lock().unwrap();
        conn.execute("DELETE FROM attachment WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    }
    if let Some(rel) = path {
        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::remove_file(dir.join("attachments").join(rel));
        }
    }
    emit_changed(&app, "attachment", &id);
    Ok(())
}

// ── 单个 Label ─────────────────────────────────────────────────────────────
#[tauri::command]
pub fn get_label(db: tauri::State<Db>, id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(
        "SELECT id, workspace_id, name, description, color, created_at, updated_at FROM label WHERE id = ?1",
        params![id],
        |r| Ok(serde_json::json!({ "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?, "description": r.get::<_, Option<String>>(3)?, "color": r.get::<_, String>(4)?, "created_at": r.get::<_, String>(5)?, "updated_at": r.get::<_, String>(6)? })),
    ).optional().map_err(|e| e.to_string())?;
    row.ok_or_else(|| format!("label {} 不存在", id))
}

#[tauri::command]
pub fn get_unread_inbox_count(db: tauri::State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM inbox_item WHERE read_at IS NULL", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "count": count }))
}

fn inbox_item_from_row(r: &rusqlite::Row) -> rusqlite::Result<InboxItem> {
    Ok(InboxItem {
        id: r.get(0)?,
        workspace_id: r.get(1)?,
        recipient_type: r.get(2)?,
        recipient_id: r.get(3)?,
        actor_type: r.get(4)?,
        actor_id: r.get(5)?,
        item_type: r.get(6)?,
        severity: r.get(7)?,
        issue_id: r.get(8)?,
        title: r.get(9)?,
        body: r.get(10)?,
        issue_status: r.get(11)?,
        created_at: r.get(12)?,
        read_at: r.get(13)?,
    })
}

// ── Inbox 未读数 / 归档分页 / Facets ──────────────────────────────────────
// 归档箱语义对齐原后端 inbox_archive.sql：
//  - 代表行：按 COALESCE(issue_id, id) 分组取最新一条已归档项（一个 issue 只显示一行）；
//  - 排除：同 issue 仍有未归档项时，整个 issue 不进归档箱；
//  - facets 是交叉过滤的：statuses 计数叠加 priority+actor+unread 过滤（不叠加自身），
//    其余维度同理；unreadCount 叠加全部四项过滤。
// 本地适配（见 MIGRATION）：不按 recipient 过滤（单用户本地版）；archived 用
// archived_at IS NOT NULL（本地列）而非原后端的 archived 布尔列；read 用 read_at。
/// 归档代表行 CTE（分页与 facets 共用）：newest = 每组最新已归档项，且同 issue
/// 无未归档项；projected = 补 issue_status / issue_priority / actor_key。
/// 注意：projected 必须显式列列名，不能用 n.*——inbox_item 自身就有 issue_status
/// 列，n.* 再加 i.status AS issue_status 会产生重名列，SQLite 按第一个解析，
/// facets/page 拿到的 issue_status 会是 inbox_item 的 NULL 而不是 issue 的状态。
const ARCHIVED_CTE: &str = "WITH newest AS (SELECT x.* FROM inbox_item x WHERE x.archived_at IS NOT NULL AND x.id = (SELECT y.id FROM inbox_item y WHERE COALESCE(y.issue_id, y.id) = COALESCE(x.issue_id, x.id) AND y.archived_at IS NOT NULL ORDER BY y.created_at DESC, y.id DESC LIMIT 1) AND (x.issue_id IS NULL OR NOT EXISTS (SELECT 1 FROM inbox_item a WHERE a.issue_id = x.issue_id AND a.archived_at IS NULL))), projected AS (SELECT n.id, n.workspace_id, n.recipient_type, n.recipient_id, n.actor_type, n.actor_id, n.type, n.severity, n.issue_id, n.title, n.body, n.created_at, n.read_at, n.archived_at, i.status AS issue_status, i.priority AS issue_priority, CASE WHEN n.actor_type = 'system' THEN 'system' ELSE n.actor_type || ':' || COALESCE(n.actor_id, '') END AS actor_key FROM newest n LEFT JOIN issue i ON i.id = n.issue_id)";

/// 归档过滤条件（作用于 projected 输出，列名前缀 p.）：statuses / priorities /
/// actors / unreadOnly。返回 (AND 片段, 参数)。
fn archived_filter_sql(filters: &serde_json::Value) -> (String, Vec<rusqlite::types::Value>) {
    let mut conds: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    let statuses = get_arr(filters, "statuses");
    if !statuses.is_empty() {
        let ph: Vec<String> = (0..statuses.len()).map(|_| "?".to_string()).collect();
        conds.push(format!("p.issue_status IN ({})", ph.join(",")));
        for v in statuses { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
    }
    let priorities = get_arr(filters, "priorities");
    if !priorities.is_empty() {
        let ph: Vec<String> = (0..priorities.len()).map(|_| "?".to_string()).collect();
        conds.push(format!("p.issue_priority IN ({})", ph.join(",")));
        for v in priorities { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
    }
    let actors = get_arr(filters, "actors");
    if !actors.is_empty() {
        let mut ors = vec![];
        for a in &actors {
            let key = a.as_str().unwrap_or_default().replace('\'', "''");
            ors.push(format!("(p.actor_key = '{}')", key));
        }
        if !ors.is_empty() { conds.push(format!("({})", ors.join(" OR "))); }
    }
    if get_bool(filters, "unreadOnly") == Some(true) { conds.push("p.read_at IS NULL".to_string()); }
    (if conds.is_empty() { String::new() } else { format!(" AND {}", conds.join(" AND ")) }, vals)
}

#[tauri::command]
pub fn list_archived_inbox_page(db: tauri::State<Db>, filters: serde_json::Value, cursor: Option<String>, group_id: Option<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let (filter_sql, mut vals) = archived_filter_sql(&filters);
    let mut sql = format!("{} SELECT p.id, p.workspace_id, p.recipient_type, p.recipient_id, p.actor_type, p.actor_id, p.type, p.severity, p.issue_id, p.title, p.body, p.issue_status, p.created_at, p.read_at FROM projected p WHERE 1 = 1{}",
        ARCHIVED_CTE, filter_sql);
    if let Some(gid) = &group_id {
        sql.push_str(" AND (p.issue_id = ? OR (p.issue_id IS NULL AND p.id = ?))");
        vals.push(rusqlite::types::Value::from(gid.clone()));
        vals.push(rusqlite::types::Value::from(gid.clone()));
    }
    if let Some(cur) = &cursor {
        sql.push_str(" AND (p.created_at, p.id) < (SELECT created_at, id FROM inbox_item WHERE id = ?)");
        vals.push(rusqlite::types::Value::from(cur.clone()));
    }
    sql.push_str(" ORDER BY p.created_at DESC, p.id DESC LIMIT 51");
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let rows = stmt.query_map(params_ref.as_slice(), inbox_item_from_row).map_err(|e| e.to_string())?;
    let mut items: Vec<InboxItem> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let has_more = items.len() > 50;
    if has_more { items.truncate(50); }
    let next_cursor = if has_more { items.last().map(|i| i.id.clone()) } else { None };
    Ok(serde_json::json!({ "items": items, "nextCursor": next_cursor, "hasMore": has_more }))
}

#[tauri::command]
pub fn get_archived_inbox_facets(db: tauri::State<Db>, filters: serde_json::Value) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    // 每维度计数时叠加其余三项过滤（原后端 FILTER (WHERE ...) 语义）。
    // match 表达式作用于 projected 列（无前缀，CTE 内直接用列名）。
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    let mut match_expr = |dim: &str, vals: &mut Vec<rusqlite::types::Value>| -> String {
        match dim {
            "status" => {
                let arr = get_arr(&filters, "statuses");
                if arr.is_empty() { "1".to_string() } else {
                    let ph: Vec<String> = (0..arr.len()).map(|_| "?".to_string()).collect();
                    for v in arr { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
                    format!("(issue_status IN ({}))", ph.join(","))
                }
            }
            "priority" => {
                let arr = get_arr(&filters, "priorities");
                if arr.is_empty() { "1".to_string() } else {
                    let ph: Vec<String> = (0..arr.len()).map(|_| "?".to_string()).collect();
                    for v in arr { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
                    format!("(issue_priority IN ({}))", ph.join(","))
                }
            }
            "actor" => {
                let arr = get_arr(&filters, "actors");
                if arr.is_empty() { "1".to_string() } else {
                    let ors: Vec<String> = arr.iter().map(|a| format!("(actor_key = '{}')", a.as_str().unwrap_or_default().replace('\'', "''"))).collect();
                    format!("({})", ors.join(" OR "))
                }
            }
            _ => if get_bool(&filters, "unreadOnly") == Some(true) { "(read_at IS NULL)".to_string() } else { "1".to_string() },
        }
    };
    let sm = match_expr("status", &mut vals);
    let pm = match_expr("priority", &mut vals);
    let am = match_expr("actor", &mut vals);
    let rm = match_expr("read", &mut vals);
    let sql = format!("{}, matched AS (SELECT projected.*, ({}) AS status_match, ({}) AS priority_match, ({}) AS actor_match, ({}) AS read_match FROM projected) SELECT 'statuses', issue_status, COUNT(*) FILTER (WHERE priority_match AND actor_match AND read_match) FROM matched WHERE issue_status IS NOT NULL GROUP BY issue_status UNION ALL SELECT 'priorities', issue_priority, COUNT(*) FILTER (WHERE status_match AND actor_match AND read_match) FROM matched WHERE issue_priority IS NOT NULL GROUP BY issue_priority UNION ALL SELECT 'actors', actor_key, COUNT(*) FILTER (WHERE status_match AND priority_match AND read_match) FROM matched WHERE actor_key IS NOT NULL GROUP BY actor_key UNION ALL SELECT 'unread', 'unread', COUNT(*) FILTER (WHERE status_match AND priority_match AND actor_match AND read_at IS NULL) FROM matched", ARCHIVED_CTE, sm, pm, am, rm);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))).map_err(|e| e.to_string())?;
    let mut statuses = serde_json::Map::new();
    let mut priorities = serde_json::Map::new();
    let mut actors = serde_json::Map::new();
    let mut unread = 0i64;
    for row in rows.flatten() {
        match row.0.as_str() {
            "statuses" => { statuses.insert(row.1, serde_json::json!(row.2)); }
            "priorities" => { priorities.insert(row.1, serde_json::json!(row.2)); }
            "actors" => { actors.insert(row.1, serde_json::json!(row.2)); }
            "unread" => { unread = row.2; }
            _ => {}
        }
    }
    Ok(serde_json::json!({ "statuses": statuses, "priorities": priorities, "actors": actors, "unreadCount": unread }))
}

// ── 分组 Issues（group_by=assignee） ───────────────────────────────────────
// 语义对齐原后端 ListGroupedIssues：limit 默认 50、上限 100，offset 默认 0，
// limit/offset 作用于每组（原 SQL 是 ROW_NUMBER() OVER (PARTITION BY assignee) 窗口；
// 本地库规模小，过滤+排序在 SQL 做、按组切片在 Rust 做，结果等价）。
// 组顺序：member → agent → squad → 未指派；组内按 sort_by 排序。
// 近似点（见 MIGRATION）：status 排序按 status 字母序（原后端按工作区自定义状态顺序）；
// last_activity 按 updated_at（本地无 last_activity_at 列）；status_categories 直接匹配
// issue.status_category（原后端把类别展开成状态 key）。
#[tauri::command]
pub fn list_grouped_issues(db: tauri::State<Db>, params: serde_json::Value) -> Result<serde_json::Value, String> {
    let group_by = get_str(&params, "group_by").unwrap_or_else(|| "assignee".to_string());
    if group_by != "assignee" { return Err("仅支持 group_by=assignee".to_string()); }
    let ws = get_str(&params, "workspace_id").unwrap_or_default();
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &ws) };
    let limit = params.get("limit").and_then(|v| v.as_i64()).filter(|v| *v > 0).unwrap_or(50).min(100);
    let offset = params.get("offset").and_then(|v| v.as_i64()).filter(|v| *v > 0).unwrap_or(0);
    let conn = db.0.lock().unwrap();
    let mut sql = format!("SELECT {} FROM issue i WHERE i.workspace_id = ?1", ISSUE_COLS);
    let mut vals: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::from(ws)];
    let mut push_in = |sql: &mut String, vals: &mut Vec<rusqlite::types::Value>, col: &str, arr: Vec<&serde_json::Value>| {
        if arr.is_empty() { return; }
        let ph: Vec<String> = (0..arr.len()).map(|_| "?".to_string()).collect();
        sql.push_str(&format!(" AND {} IN ({})", col, ph.join(",")));
        for v in arr { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
    };
    push_in(&mut sql, &mut vals, "i.status", get_arr(&params, "statuses"));
    push_in(&mut sql, &mut vals, "i.status_category", get_arr(&params, "status_categories"));
    push_in(&mut sql, &mut vals, "i.priority", get_arr(&params, "priorities"));
    push_in(&mut sql, &mut vals, "i.assignee_type", get_arr(&params, "assignee_types"));
    if let Some(v) = get_str(&params, "assignee_id") { sql.push_str(" AND i.assignee_id = ?"); vals.push(rusqlite::types::Value::from(v)); }
    push_in(&mut sql, &mut vals, "i.assignee_id", get_arr(&params, "assignee_ids"));
    if let Some(v) = get_str(&params, "creator_id") { sql.push_str(" AND i.creator_id = ?"); vals.push(rusqlite::types::Value::from(v)); }
    if let Some(v) = get_str(&params, "project_id") { sql.push_str(" AND i.project_id = ?"); vals.push(rusqlite::types::Value::from(v)); }
    // metadata：JSON 包含（每 key 一条比较；对象/数组值按精确相等近似）。
    // 注意：json_extract 对字符串值返回裸文本 alpha，而 json('"alpha"') 返回带引号的
    // JSON 文本，两边永不相等；必须用 CAST(... AS TEXT) 后与裸值比较。
    // 布尔值 json_extract 返回 1/0，CAST 后为 '1'/'0'，参数侧同款转换。
    if let Some(meta) = params.get("metadata").and_then(|v| v.as_object()) {
        for (k, v) in meta {
            if k.contains('"') || k.contains('\\') { continue; }
            let path = format!("$.\"{}\"", k);
            let raw = match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Bool(b) => if *b { "1".to_string() } else { "0".to_string() },
                serde_json::Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            sql.push_str(" AND CAST(json_extract(i.metadata, ?) AS TEXT) = ?");
            vals.push(rusqlite::types::Value::from(path));
            vals.push(rusqlite::types::Value::from(raw));
        }
    }
    // properties：定义 id → 接受值列表（定义内 OR，定义间 AND）。值可以是字符串或
    // {eq/in/ne/nin/exists} 操作对象；checkbox 的 "true"/"false" 同时匹配存储的 1/0。
    if let Some(props) = params.get("properties").and_then(|v| v.as_object()) {
        for (def_id, accepted) in props {
            if !def_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') { continue; }
            let path = format!("$.\"{}\"", def_id);
            let arr = accepted.as_array().cloned().unwrap_or_else(|| vec![accepted.clone()]);
            let mut ors: Vec<String> = vec![];
            for v in &arr {
                if let Some(obj) = v.as_object() {
                    if let Some(eq) = obj.get("eq") { ors.push(format!("CAST(json_extract(i.properties, '{}') AS TEXT) IN ({})", path, eq_candidates(eq))); }
                    else if let Some(ne) = obj.get("ne") { sql.push_str(&format!(" AND CAST(json_extract(i.properties, '{}') AS TEXT) NOT IN ({})", path, eq_candidates(ne))); }
                    else if let Some(list) = obj.get("in").and_then(|x| x.as_array()) {
                        let mut all: Vec<String> = vec![];
                        for x in list { all.push(eq_candidates(x)); }
                        let joined = all.join(",");
                        ors.push(format!("CAST(json_extract(i.properties, '{}') AS TEXT) IN ({})", path, joined));
                    }
                    else if let Some(list) = obj.get("nin").and_then(|x| x.as_array()) {
                        let mut all: Vec<String> = vec![];
                        for x in list { all.push(eq_candidates(x)); }
                        sql.push_str(&format!(" AND CAST(json_extract(i.properties, '{}') AS TEXT) NOT IN ({})", path, all.join(",")));
                    }
                    else if obj.get("exists") == Some(&serde_json::json!(true)) { sql.push_str(&format!(" AND json_extract(i.properties, '{}') IS NOT NULL", path)); }
                    else if obj.get("exists") == Some(&serde_json::json!(false)) { sql.push_str(&format!(" AND json_extract(i.properties, '{}') IS NULL", path)); }
                } else {
                    ors.push(format!("CAST(json_extract(i.properties, '{}') AS TEXT) IN ({})", path, eq_candidates(v)));
                }
            }
            if !ors.is_empty() { sql.push_str(&format!(" AND ({})", ors.join(" OR "))); }
        }
    }
    // involves_user_id：原后端 4 分支 UNION（直接指派 member 故意排除，与 assignee_id 分页正交）。
    if let Some(uid) = get_str(&params, "involves_user_id") {
        let u = uid.replace('\'', "''");
        sql.push_str(&format!(" AND ((i.assignee_type = 'agent' AND i.assignee_id IN (SELECT a.id FROM agent a WHERE a.workspace_id = i.workspace_id AND a.owner_id = '{}')) OR (i.assignee_type = 'squad' AND i.assignee_id IN (SELECT sm.squad_id FROM squad_member sm JOIN squad s ON s.id = sm.squad_id WHERE s.workspace_id = i.workspace_id AND sm.member_type = 'member' AND sm.member_id = '{}' UNION SELECT s.id FROM squad s JOIN agent a ON a.id = s.leader_id WHERE s.workspace_id = i.workspace_id AND a.workspace_id = i.workspace_id AND a.owner_id = '{}' UNION SELECT sm.squad_id FROM squad_member sm JOIN squad s ON s.id = sm.squad_id JOIN agent a ON a.id = sm.member_id WHERE s.workspace_id = i.workspace_id AND sm.member_type = 'agent' AND a.workspace_id = i.workspace_id AND a.owner_id = '{}')))", u, u, u, u));
    }
    // assignee_filters + include_no_assignee（原后端：过滤列表 OR 未指派）。
    {
        let af = get_arr(&params, "assignee_filters");
        let include_none = get_bool(&params, "include_no_assignee") == Some(true);
        if !af.is_empty() || include_none {
            let mut ors: Vec<String> = vec![];
            for f in &af {
                let t = get_str(f, "type").unwrap_or_default().replace('\'', "''");
                let i = get_str(f, "id").unwrap_or_default().replace('\'', "''");
                ors.push(format!("(i.assignee_type = '{}' AND i.assignee_id = '{}')", t, i));
            }
            if include_none { ors.push("(COALESCE(i.assignee_id, '') = '')".to_string()); }
            sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
        }
    }
    // creator_filters（原后端同款 OR）。
    {
        let cf = get_arr(&params, "creator_filters");
        if !cf.is_empty() {
            let mut ors: Vec<String> = vec![];
            for f in &cf {
                let t = get_str(f, "type").unwrap_or_default().replace('\'', "''");
                let i = get_str(f, "id").unwrap_or_default().replace('\'', "''");
                ors.push(format!("(i.creator_type = '{}' AND i.creator_id = '{}')", t, i));
            }
            sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
        }
    }
    // project_ids + include_no_project。
    {
        let pids = get_arr(&params, "project_ids");
        let include_none = get_bool(&params, "include_no_project") == Some(true);
        if !pids.is_empty() || include_none {
            let mut ors: Vec<String> = vec![];
            if !pids.is_empty() {
                let ph: Vec<String> = (0..pids.len()).map(|_| "?".to_string()).collect();
                ors.push(format!("i.project_id IN ({})", ph.join(",")));
                for v in pids { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
            }
            if include_none { ors.push("i.project_id IS NULL".to_string()); }
            sql.push_str(&format!(" AND ({})", ors.join(" OR ")));
        }
    }
    // label_ids：issue_label 存在即命中（原后端 issue_to_label，本地表名不同）。
    {
        let lids = get_arr(&params, "label_ids");
        if !lids.is_empty() {
            let ph: Vec<String> = (0..lids.len()).map(|_| "?".to_string()).collect();
            sql.push_str(&format!(" AND EXISTS (SELECT 1 FROM issue_label il WHERE il.issue_id = i.id AND il.label_id IN ({}))", ph.join(",")));
            for v in lids { vals.push(rusqlite::types::Value::from(v.as_str().unwrap_or_default().to_string())); }
        }
    }
    // 日期窗口（date_field 白名单）。
    {
        let field = get_str(&params, "date_field").unwrap_or_default();
        if field == "created_at" || field == "updated_at" {
            if let Some(ds) = get_str(&params, "date_start") { sql.push_str(&format!(" AND substr(i.{}, 1, 10) >= ?", field)); vals.push(rusqlite::types::Value::from(ds)); }
            if let Some(de) = get_str(&params, "date_end") { sql.push_str(&format!(" AND substr(i.{}, 1, 10) <= ?", field)); vals.push(rusqlite::types::Value::from(de)); }
        }
    }
    // group_assignee_type/id：限定到单个组。
    if let Some(gat) = get_str(&params, "group_assignee_type") {
        if gat == "none" {
            sql.push_str(" AND COALESCE(i.assignee_id, '') = ''");
        } else if ["member", "agent", "squad"].contains(&gat.as_str()) {
            if let Some(gid) = get_str(&params, "group_assignee_id") {
                sql.push_str(" AND i.assignee_type = ? AND i.assignee_id = ?");
                vals.push(rusqlite::types::Value::from(gat));
                vals.push(rusqlite::types::Value::from(gid));
            } else { return Err("group_assignee_id 不能为空".to_string()); }
        } else { return Err(format!("group_assignee_type 非法：{}", gat)); }
    }
    // 排序：原后端规则（position 恒 ASC；last_activity 默认 DESC；date/property NULLS LAST；
    // 决胜 i.created_at DESC, i.id DESC；last_activity 决胜仅 i.id DESC）。
    let (sort_expr, sort_dir, nulls_last, is_last_activity) = {
        let sort_by = get_str(&params, "sort_by").unwrap_or_else(|| "position".to_string());
        let dir = get_str(&params, "sort_direction").unwrap_or_default().to_lowercase();
        let dir = if dir == "desc" { "DESC" } else { "ASC" };
        match sort_by.as_str() {
            "position" => ("i.position".to_string(), "ASC", false, false),
            "title" => ("i.title".to_string(), dir, false, false),
            "created_at" => ("i.created_at".to_string(), dir, false, false),
            "updated_at" => ("i.updated_at".to_string(), dir, false, false),
            "start_date" => ("i.start_date".to_string(), dir, true, false),
            "due_date" => ("i.due_date".to_string(), dir, true, false),
            "last_activity" => ("i.updated_at".to_string(), if get_str(&params, "sort_direction").is_none() { "DESC" } else { dir }, true, true),
            "status" => ("i.status".to_string(), dir, false, false),
            "priority" => ("CASE i.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 WHEN 'low' THEN 3 ELSE 4 END".to_string(), dir, false, false),
            other if other.starts_with("property:") => {
                let def_id = &other["property:".len()..];
                if def_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !def_id.is_empty() {
                    (format!("json_extract(i.properties, '$.\"{}\"')", def_id), dir, true, false)
                } else {
                    ("i.position".to_string(), "ASC", false, false)
                }
            }
            other => return Err(format!("sort_by 非法：{}", other)),
        }
    };
    let mut order = " ORDER BY CASE COALESCE(i.assignee_type, '') WHEN 'member' THEN 0 WHEN 'agent' THEN 1 WHEN 'squad' THEN 2 ELSE 3 END, COALESCE(i.assignee_type, ''), COALESCE(i.assignee_id, '')".to_string();
    if nulls_last { order.push_str(&format!(", {} IS NULL, {} {}", sort_expr, sort_expr, sort_dir)); }
    else { order.push_str(&format!(", {} {}", sort_expr, sort_dir)); }
    if is_last_activity { order.push_str(", i.id DESC"); }
    else { order.push_str(", i.created_at DESC, i.id DESC"); }
    sql.push_str(&order);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let rows = stmt.query_map(params_ref.as_slice(), issue_from_row_pub).map_err(|e| e.to_string())?;
    let issues: Vec<crate::models::Issue> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    // 按 (assignee_type, assignee_id) 分组；NULL 与 '' 归一为未指派（本地历史数据两种形式都有）。
    let mut groups: Vec<(String, Option<String>, Option<String>, Vec<serde_json::Value>)> = vec![];
    for it in &issues {
        let (at, ai) = (
            it.assignee_type.clone().filter(|v| !v.is_empty()),
            it.assignee_id.clone().filter(|v| !v.is_empty()),
        );
        let key = match (&at, &ai) {
            (Some(t), Some(i)) => format!("{}:{}", t, i),
            _ => "unassigned".to_string(),
        };
        let pos = groups.iter().position(|g| g.0 == key);
        let val = serde_json::to_value(it).map_err(|e| e.to_string())?;
        match pos {
            Some(p) => groups[p].3.push(val),
            None => groups.push((key, at, ai, vec![val])),
        }
    }
    // 每组切片（原后端 ROW_NUMBER 窗口语义）+ total 为组内总数。
    let out: Vec<serde_json::Value> = groups.into_iter().map(|(id, at, ai, items)| {
        let total = items.len();
        let page: Vec<serde_json::Value> = items.into_iter().skip(offset as usize).take(limit as usize).collect();
        serde_json::json!({ "id": id, "assignee_type": at, "assignee_id": ai, "issues": page, "total": total })
    }).collect();
    Ok(serde_json::json!({ "groups": out }))
}

/// property 过滤值的 SQL 字面量候选：字符串本身 + checkbox 的 1/0 等价 + 数字原值。
fn eq_candidates(v: &serde_json::Value) -> String {
    let raw = match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    let mut cands = vec![format!("'{}'", raw.replace('\'', "''"))];
    if raw == "true" { cands.push("'1'".to_string()); }
    if raw == "false" { cands.push("'0'".to_string()); }
    cands.join(",")
}

// ── Quick Create ───────────────────────────────────────────────────────────
#[tauri::command]
pub fn quick_create_issue(app: tauri::AppHandle, db: tauri::State<Db>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let prompt = get_str(&data, "prompt").unwrap_or_default();
    if prompt.trim().is_empty() { return Err("prompt 不能为空".to_string()); }
    // 原后端：agent_id / squad_id 必须二选一（前端对话框总是二选一）。
    let agent_id = get_str(&data, "agent_id").filter(|v| !v.trim().is_empty());
    let squad_id = get_str(&data, "squad_id").filter(|v| !v.trim().is_empty());
    if agent_id.is_some() == squad_id.is_some() {
        return Err("agent_id 与 squad_id 必须二选一".to_string());
    }
    // priority 白名单（原后端同款）。
    let priority = get_str(&data, "priority").unwrap_or_else(|| "medium".to_string()).to_lowercase();
    if !["urgent", "high", "medium", "low"].contains(&priority.as_str()) {
        return Err("priority 必须是 urgent/high/medium/low 之一".to_string());
    }
    // due_date 必须是 YYYY-MM-DD（原后端同款校验）。
    let due_date = get_str(&data, "due_date").filter(|v| !v.trim().is_empty());
    if let Some(d) = &due_date {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            return Err("due_date 格式非法，应为 YYYY-MM-DD".to_string());
        }
    }
    let ws = { let conn = db.0.lock().unwrap(); conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default() };
    // 解析执行 agent：squad → 其 leader；agent → 自身；同时校验同工作区、未归档。
    // 指派写到 issue 上（assignee_type/id/name），与原后端“issue 指派给 squad/agent”一致。
    let (run_agent_id, assignee_type, assignee_id, assignee_name) = {
        let conn = db.0.lock().unwrap();
        if let Some(sid) = squad_id {
            let (leader, sname, archived): (String, String, Option<String>) = conn.query_row(
                "SELECT leader_id, name, archived_at FROM squad WHERE id = ?1 AND workspace_id = ?2",
                params![sid, ws], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).map_err(|_| format!("squad {} 不存在于当前工作区", sid))?;
            if archived.is_some() { return Err(format!("squad {} 已归档", sid)); }
            if leader.is_empty() { return Err(format!("squad {} 没有 leader agent", sid)); }
            let (aname, cli, aarch): (String, String, Option<String>) = conn.query_row(
                "SELECT name, cli, archived_at FROM agent WHERE id = ?1 AND workspace_id = ?2",
                params![leader, ws], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            ).map_err(|_| format!("squad {} 的 leader agent 不存在", sid))?;
            if aarch.is_some() { return Err("squad 的 leader agent 已归档".to_string()); }
            let _ = aname; let _ = cli;
            (leader, "squad".to_string(), sid, sname)
        } else {
            let aid = agent_id.unwrap();
            let (aname, archived): (String, Option<String>) = conn.query_row(
                "SELECT name, archived_at FROM agent WHERE id = ?1 AND workspace_id = ?2",
                params![aid, ws], |r| Ok((r.get(0)?, r.get(1)?)),
            ).map_err(|_| format!("agent {} 不存在于当前工作区", aid))?;
            if archived.is_some() { return Err(format!("agent {} 已归档", aid)); }
            (aid.clone(), "agent".to_string(), aid, aname)
        }
    };
    // project / parent 同工作区校验（原后端同款信任边界）。
    let project_id = get_str(&data, "project_id").filter(|v| !v.trim().is_empty());
    let parent_issue_id = get_str(&data, "parent_issue_id").filter(|v| !v.trim().is_empty());
    {
        let conn = db.0.lock().unwrap();
        if let Some(pid) = &project_id {
            let ok: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM project WHERE id = ?1 AND workspace_id = ?2)", params![pid, ws], |r| r.get(0)).map_err(|e| e.to_string())?;
            if !ok { return Err(format!("project {} 不存在于当前工作区", pid)); }
        }
        if let Some(pid) = &parent_issue_id {
            let ok: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM issue WHERE id = ?1 AND workspace_id = ?2)", params![pid, ws], |r| r.get(0)).map_err(|e| e.to_string())?;
            if !ok { return Err(format!("parent issue {} 不存在于当前工作区", pid)); }
        }
    }
    // attachment_ids：必须存在，绑定到新 issue（原后端 EnqueueQuickCreateTask 同款绑定）。
    let attachment_ids: Vec<String> = data.get("attachment_ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default();
    let title: String = prompt.lines().next().unwrap_or("").chars().take(120).collect();
    let title = if title.trim().is_empty() { "未命名 issue".to_string() } else { title };
    let issue_id = new_id();
    let now = now_iso();
    let (number, identifier) = {
        let conn = db.0.lock().unwrap();
        let max: i64 = conn.query_row("SELECT COALESCE(MAX(number), 0) FROM issue WHERE workspace_id = ?1", params![ws], |r| r.get(0)).unwrap_or(0);
        (max + 1, format!("MUL-{}", max + 1))
    };
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO issue (id, workspace_id, number, identifier, title, body, status, status_category, priority, assignee_type, assignee_id, assignee_name, due_date, project_id, parent_issue_id, position, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,'backlog','backlog',?7,?8,?9,?10,?11,?12,?13,0,?14,?14)",
            params![issue_id, ws, number, identifier, title, prompt, priority, assignee_type, assignee_id, assignee_name, due_date, project_id, parent_issue_id, now],
        ).map_err(|e| e.to_string())?;
        for att in &attachment_ids {
            let n = conn.execute("UPDATE attachment SET issue_id = ?1 WHERE id = ?2", params![issue_id, att]).map_err(|e| e.to_string())?;
            if n == 0 { return Err(format!("attachment {} 不存在", att)); }
        }
    }
    // 有 agent 则 spawn run；原后端是 daemon 队列，本地版直接 spawn（见 spawn_run）。
    let task_id = {
        let (aname, cli) = {
            let conn = db.0.lock().unwrap();
            conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![run_agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_else(|_| ("agent".to_string(), "claude".to_string()))
        };
        match super::spawn_run(&app, &db, &issue_id, &run_agent_id, &aname, &cli, &prompt) {
            Ok(run) => run.id,
            Err(_) => String::new(),
        }
    };
    emit_changed(&app, "issue", &issue_id);
    Ok(serde_json::json!({ "task_id": task_id, "issue_id": issue_id }))
}

// ── Render Quick Action ────────────────────────────────────────────────────
#[tauri::command]
pub fn render_quick_action(db: tauri::State<Db>, issue_id: String, quick_action_id: String) -> Result<String, String> {
    let conn = db.0.lock().unwrap();
    let prompt: String = conn.query_row("SELECT prompt FROM quick_action WHERE id = ?1", params![quick_action_id], |r| r.get(0)).map_err(|_| format!("quick action {} 不存在", quick_action_id))?;
    let issue = conn.query_row("SELECT title, identifier, body, status, priority, id FROM issue WHERE id = ?1", params![issue_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, String>(5)?))).map_err(|_| format!("issue {} 不存在", issue_id))?;
    let out = prompt
        .replace("{{issue.title}}", &issue.0)
        .replace("{{issue.identifier}}", &issue.1)
        .replace("{{issue.body}}", &issue.2)
        .replace("{{issue.status}}", &issue.3)
        .replace("{{issue.priority}}", &issue.4)
        .replace("{{issue.id}}", &issue.5)
        .replace("{{issue.url}}", "");
    Ok(out)
}

// ── Replay Autopilot Delivery ──────────────────────────────────────────────
#[tauri::command]
pub fn replay_autopilot_delivery(app: tauri::AppHandle, db: tauri::State<Db>, autopilot_id: String, delivery_id: String) -> Result<serde_json::Value, String> {
    let (ws, trigger_id, run_id, detail) = {
        let conn = db.0.lock().unwrap();
        let (trigger_id, run_id, detail) = conn.query_row(
            "SELECT trigger_id, run_id, detail FROM autopilot_delivery WHERE id = ?1 AND autopilot_id = ?2",
            params![delivery_id, autopilot_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?)),
        ).map_err(|_| format!("delivery {} 不存在", delivery_id))?;
        let ws: String = conn.query_row("SELECT workspace_id FROM autopilot WHERE id = ?1", params![autopilot_id], |r| r.get(0)).map_err(|_| format!("autopilot {} 不存在", autopilot_id))?;
        (ws, trigger_id, run_id, detail)
    };
    let new_id = new_id();
    let now = now_iso();
    let new_detail = serde_json::json!({ "replayed_from": delivery_id, "original_detail": parse_json_pub(&detail, serde_json::json!({})) }).to_string();
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO autopilot_delivery (id, autopilot_id, run_id, trigger_id, status, detail, created_at) VALUES (?1,?2,?3,?4,'pending',?5,?6)",
            params![new_id, autopilot_id, run_id, trigger_id, new_detail, now],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "autopilot", &autopilot_id);
    Ok(serde_json::json!({
        "id": new_id, "workspace_id": ws, "autopilot_id": autopilot_id,
        "trigger_id": trigger_id, "provider": "local", "event": "autopilot.trigger",
        "dedupe_key": null, "dedupe_source": null, "signature_status": "not_required",
        "status": "queued", "attempt_count": 0, "dispatch_attempts": 0,
        "available_at": now, "content_type": null, "response_status": null,
        "autopilot_run_id": run_id, "replayed_from_delivery_id": delivery_id,
        "error": null, "reason_code": null, "replay_idempotency_key": new_id, "created_at": now,
    }))
}

// ── Prioritize Queued Chat Task ────────────────────────────────────────────
#[tauri::command]
pub fn prioritize_queued_chat_task(db: tauri::State<Db>, session_id: String, task_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM run WHERE id = ?1 AND chat_session_id = ?2)", params![task_id, session_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !exists { return Err(format!("task {} 不存在于会话 {}", task_id, session_id)); }
    // 本地版 run 直接进入 running，没有排队层：优先 = 返回当前正在跑的任务，
    // 前端据此决定是否先停它。无正在跑的任务时 active_task_id 省略。
    let active: Option<String> = conn.query_row(
        "SELECT id FROM run WHERE chat_session_id = ?1 AND status = 'running' AND id != ?2 ORDER BY started_at LIMIT 1",
        params![session_id, task_id], |r| r.get(0)).optional().map_err(|e| e.to_string())?.flatten();
    Ok(match active {
        Some(a) => serde_json::json!({ "task_id": task_id, "active_task_id": a }),
        None => serde_json::json!({ "task_id": task_id }),
    })
}

// ── Unsubscribe From Issue Subtree ─────────────────────────────────────────
#[tauri::command]
pub fn unsubscribe_from_issue_subtree(app: tauri::AppHandle, db: tauri::State<Db>, issue_id: String, user_id: Option<String>, user_type: Option<String>) -> Result<(), String> {
    // BFS 收集 issue + 所有后代
    let mut ids = vec![issue_id.clone()];
    {
        let conn = db.0.lock().unwrap();
        let mut i = 0;
        while i < ids.len() {
            let cur = ids[i].clone();
            let mut stmt = conn.prepare("SELECT id FROM issue WHERE parent_issue_id = ?1").map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![cur], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
            for row in rows.flatten() { if !ids.contains(&row) { ids.push(row); } }
            i += 1;
        }
    }
    {
        let conn = db.0.lock().unwrap();
        let ph: Vec<String> = (0..ids.len()).map(|_| "?".to_string()).collect();
        let mut sql = format!("DELETE FROM issue_subscriber WHERE issue_id IN ({})", ph.join(","));
        let mut vals: Vec<rusqlite::types::Value> = ids.iter().map(|i| rusqlite::types::Value::from(i.clone())).collect();
        if let (Some(u), Some(t)) = (user_id, user_type) {
            sql.push_str(" AND user_id = ? AND user_type = ?");
            vals.push(rusqlite::types::Value::from(u));
            vals.push(rusqlite::types::Value::from(t));
        }
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "issue", &issue_id);
    Ok(())
}

// ── Member CRUD（本地单用户：create 直接建成员，不走邀请流程） ──────────
#[tauri::command]
pub fn create_member(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let email = get_str(&data, "email").or_else(|| get_str(&data, "invitee_email")).unwrap_or_default();
    if email.trim().is_empty() { return Err("email 不能为空".to_string()); }
    let id = new_id();
    let now = now_iso();
    let name = get_str(&data, "name").unwrap_or_else(|| email.clone());
    let role = get_str(&data, "role").unwrap_or_else(|| "member".to_string());
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO member (id, workspace_id, user_id, name, email, avatar_url, role, created_at) VALUES (?1,?2,?3,?4,?5,NULL,?6,?7)",
            params![id, ws, id, name, email, role, now],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "member", &id);
    // 云端返回 Invitation；本地直接建成员，status 直接 accepted。
    Ok(serde_json::json!({ "id": id, "workspace_id": ws, "inviter_id": "local", "invitee_email": email, "invitee_user_id": id, "role": role, "status": "accepted", "created_at": now, "updated_at": now, "expires_at": now, "inviter_name": "本地用户" }))
}

#[tauri::command]
pub fn update_member(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, member_id: String, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    for (col, key) in [("role", "role"), ("name", "name"), ("email", "email"), ("avatar_url", "avatar_url")] {
        if let Some(v) = get_str(&data, key) { sets.push(format!("{} = ?", col)); vals.push(rusqlite::types::Value::from(v)); }
    }
    if sets.is_empty() { return Err("没有可更新的字段".to_string()); }
    vals.push(rusqlite::types::Value::from(member_id.clone()));
    vals.push(rusqlite::types::Value::from(ws.clone()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let n = conn.execute(&format!("UPDATE member SET {} WHERE id = ? AND workspace_id = ?", sets.join(", ")), params_ref.as_slice()).map_err(|e| e.to_string())?;
    if n == 0 { return Err(format!("member {} 不存在", member_id)); }
    let row = conn.query_row(
        "SELECT id, workspace_id, user_id, role, created_at, name, email, avatar_url FROM member WHERE id = ?1",
        params![member_id],
        |r| Ok(serde_json::json!({ "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?, "user_id": r.get::<_, String>(2)?, "role": r.get::<_, String>(3)?, "created_at": r.get::<_, String>(4)?, "name": r.get::<_, String>(5)?, "email": r.get::<_, String>(6)?, "avatar_url": r.get::<_, Option<String>>(7)? })),
    ).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "member", &member_id);
    Ok(row)
}

#[tauri::command]
pub fn delete_member(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, member_id: String) -> Result<(), String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM member WHERE id = ?1 AND workspace_id = ?2", params![member_id, ws]).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "member", &member_id);
    Ok(())
}

// ── Workspace 更新 / 删除 ──────────────────────────────────────────────────
fn workspace_settings(conn: &rusqlite::Connection, ws: &str) -> serde_json::Value {
    let raw: String = conn.query_row("SELECT COALESCE(settings, '{}') FROM workspace WHERE id = ?1", params![ws], |r| r.get(0)).unwrap_or_else(|_| "{}".to_string());
    parse_json_pub(&raw, serde_json::json!({}))
}

fn workspace_json(conn: &rusqlite::Connection, ws: &str) -> Result<serde_json::Value, String> {
    let (id, slug, name, created_at) = conn.query_row(
        "SELECT id, slug, name, created_at FROM workspace WHERE id = ?1",
        params![ws], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
    ).map_err(|_| format!("workspace {} 不存在", ws))?;
    let settings = workspace_settings(conn, ws);
    let mut out = serde_json::json!({ "id": id, "slug": slug, "name": name, "created_at": created_at });
    if let Some(obj) = out.as_object_mut() {
        if let Some(sobj) = settings.as_object() {
            for (k, v) in sobj { obj.insert(k.clone(), v.clone()); }
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn update_workspace(app: tauri::AppHandle, db: tauri::State<Db>, id: String, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &id) };
    {
        let conn = db.0.lock().unwrap();
        if let Some(name) = get_str(&data, "name") {
            conn.execute("UPDATE workspace SET name = ?1 WHERE id = ?2", params![name, ws]).map_err(|e| e.to_string())?;
        }
        // workspace 表只有 id/slug/name，其余字段存 settings JSON。
        let mut settings = workspace_settings(&conn, &ws);
        let mut changed = false;
        for key in ["description", "context", "settings", "repos", "issue_prefix", "avatar_url"] {
            if let Some(v) = data.get(key) {
                if key == "settings" {
                    if let (Some(dst), Some(src)) = (settings.as_object_mut(), v.as_object()) {
                        for (k, val) in src { dst.insert(k.clone(), val.clone()); }
                    }
                } else {
                    settings[key] = v.clone();
                }
                changed = true;
            }
        }
        if changed {
            conn.execute("UPDATE workspace SET settings = ?1 WHERE id = ?2", params![settings.to_string(), ws]).map_err(|e| e.to_string())?;
        }
    }
    emit_changed(&app, "workspace", &ws);
    let conn = db.0.lock().unwrap();
    workspace_json(&conn, &ws)
}

#[tauri::command]
pub fn delete_workspace(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String) -> Result<(), String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    // 级联删 workspace 下的主要数据（不在列表里的表按 workspace_id 不可见，不影响查询）。
    for (table, col) in [
        ("issue", "workspace_id"), ("agent", "workspace_id"), ("skill", "workspace_id"),
        ("squad", "workspace_id"), ("autopilot", "workspace_id"), ("inbox_item", "workspace_id"),
        ("member", "workspace_id"), ("chat_session", "workspace_id"), ("label", "workspace_id"),
        ("property_def", "workspace_id"), ("issue_status", "workspace_id"), ("pin", "workspace_id"),
        ("channel_installation", "workspace_id"), ("workspace_mcp_server", "workspace_id"),
        ("runtime_profile", "workspace_id"), ("issue_view", "workspace_id"),
        ("notification_preferences", "workspace_id"), ("system_wakeup", "workspace_id"),
        ("project", "workspace_id"), ("issue_property_value", "workspace_id"),
    ] {
        let _ = conn.execute(&format!("DELETE FROM {} WHERE {} = ?1", table, col), params![ws]);
    }
    conn.execute("DELETE FROM workspace WHERE id = ?1", params![ws]).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "workspace", &ws);
    Ok(())
}

// ── Runtime（本地版只有一个运行时：本机） ────────────────────────────────
/// updateRuntime：custom_name/visibility 存入 workspace settings；返回应用后的 Runtime。
#[tauri::command]
pub fn update_runtime(app: tauri::AppHandle, db: tauri::State<Db>, runtime_id: String, patch: serde_json::Value) -> Result<serde_json::Value, String> {
    if runtime_id != "local" { return Err(format!("runtime {} 不存在", runtime_id)); }
    let ws = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default()
    };
    {
        let conn = db.0.lock().unwrap();
        let mut settings = workspace_settings(&conn, &ws);
        if let Some(name) = get_str(&patch, "custom_name") {
            if name.is_empty() { settings.as_object_mut().map(|o| o.remove("runtime_custom_name")); }
            else { settings["runtime_custom_name"] = serde_json::json!(name); }
        }
        if let Some(v) = get_str(&patch, "visibility") { settings["runtime_visibility"] = serde_json::json!(v); }
        conn.execute("UPDATE workspace SET settings = ?1 WHERE id = ?2", params![settings.to_string(), ws]).map_err(|e| e.to_string())?;
    }
    let _ = app;
    let conn = db.0.lock().unwrap();
    let settings = workspace_settings(&conn, &ws);
    let name = settings.get("runtime_custom_name").and_then(|v| v.as_str()).unwrap_or("本机").to_string();
    Ok(serde_json::json!({ "id": "local", "name": name, "kind": "local", "status": "online", "detected_clis": crate::daemon::detect_clis() }))
}

fn unbind_all_agents(conn: &rusqlite::Connection, runtime_id: &str) -> Result<i64, String> {
    let n = conn.execute("UPDATE agent SET runtime_id = '' WHERE runtime_id = ?1", params![runtime_id]).map_err(|e| e.to_string())?;
    Ok(n as i64)
}
fn cancel_running_runs(app: &tauri::AppHandle, db: &Db, runtime_id: &str) -> Result<i64, String> {
    // 真取消：逐个调 cancel_run_by_id（kill RunChildren 里的子进程 + 置 cancelled，
    // 带终态守卫，watcher 不会覆盖）。run 表无 runtime 列，按 agent.runtime_id 定位。
    // 必须在 unbind 之前调用——unbind 会清空 agent.runtime_id，之后就找不到这些 run。
    // 旧实现只 UPDATE DB 状态、不 kill 进程，且 delete_runtime 先解绑后取消，
    // 导致 running 任务既杀不掉、DB 状态也不变。
    let ids: Vec<String> = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM run WHERE status = 'running' AND agent_id IN (SELECT id FROM agent WHERE runtime_id = ?1)").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![runtime_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    let n = ids.len() as i64;
    for id in ids {
        super::cancel_run_by_id(app, db, &id)?;
    }
    Ok(n)
}

/// deleteRuntime：本地运行时即本机，无法真正删除；语义 = 解绑全部 agent + 取消运行中任务。
#[tauri::command]
pub fn delete_runtime(app: tauri::AppHandle, db: tauri::State<Db>, runtime_id: String) -> Result<(), String> {
    if runtime_id != "local" { return Err(format!("runtime {} 不存在", runtime_id)); }
    // 先真 kill + 取消运行中任务，再解绑（顺序不能反，见 cancel_running_runs）。
    cancel_running_runs(&app, &db, &runtime_id)?;
    let conn = db.0.lock().unwrap();
    unbind_all_agents(&conn, &runtime_id)?;
    drop(conn);
    emit_changed(&app, "runtime", &runtime_id);
    Ok(())
}

#[tauri::command]
pub fn unbind_agents_and_delete_runtime(app: tauri::AppHandle, db: tauri::State<Db>, runtime_id: String, expected_active_agent_ids: Vec<String>) -> Result<serde_json::Value, String> {
    if runtime_id != "local" { return Err(format!("runtime {} 不存在", runtime_id)); }
    let live: Vec<String> = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM agent WHERE runtime_id = ?1 AND archived_at IS NULL ORDER BY id").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![runtime_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    let mut expected = expected_active_agent_ids.clone();
    expected.sort();
    let mut live_sorted = live.clone();
    live_sorted.sort();
    if expected != live_sorted {
        return Err("runtime_delete_plan_changed：活动 agent 集合已变化，请重新确认".to_string());
    }
    // 先真 kill + 取消这些 agent 的运行中任务，再解绑（解绑后按 runtime_id 找不到它们）。
    // cancel 内部会拿 db 锁，所以必须在不持有 conn 锁时调。
    let tasks_cancelled = cancel_running_runs(&app, &db, &runtime_id)?;
    let conn = db.0.lock().unwrap();
    let agents_unbound = unbind_all_agents(&conn, &runtime_id)?;
    // 仅暂停本次解绑的 agent 的 autopilot（原语义），避免误伤早已解绑的 agent。
    let paused = if live.is_empty() { 0 } else {
        let ph: Vec<String> = (0..live.len()).map(|_| "?".to_string()).collect();
        // 占位符顺序：status 在前，ids 在后。
        let mut vals: Vec<rusqlite::types::Value> = vec![rusqlite::types::Value::from("active".to_string())];
        vals.extend(live.iter().map(|i| rusqlite::types::Value::from(i.clone())));
        let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
        conn.execute(
            &format!("UPDATE autopilot SET status = 'paused' WHERE status = ? AND assignee_type = 'agent' AND assignee_id IN ({})", ph.join(",")),
            params_ref.as_slice(),
        ).map_err(|e| e.to_string())? as i64
    };
    drop(conn);
    emit_changed(&app, "runtime", &runtime_id);
    Ok(serde_json::json!({ "status": "deleted", "agents_unbound": agents_unbound, "agents_archived": agents_unbound, "tasks_cancelled": tasks_cancelled, "autopilots_paused": paused }))
}

// ── Runtime Profile CRUD ───────────────────────────────────────────────────
fn profile_from_row(r: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    let fixed: String = r.get(7)?;
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?,
        "display_name": r.get::<_, String>(2)?, "protocol_family": r.get::<_, String>(3)?,
        "runtime_type": r.get::<_, Option<String>>(4)?, "command_name": r.get::<_, String>(5)?,
        "description": r.get::<_, Option<String>>(6)?, "fixed_args": parse_json_pub(&fixed, serde_json::json!([])),
        "visibility": r.get::<_, String>(8)?, "created_by": r.get::<_, Option<String>>(9)?,
        "enabled": r.get::<_, i64>(10)? != 0, "created_at": r.get::<_, String>(11)?, "updated_at": r.get::<_, String>(12)?,
    }))
}
const PROFILE_COLS: &str = "id, workspace_id, display_name, protocol_family, runtime_type, command_name, description, fixed_args, visibility, created_by, enabled, created_at, updated_at";

#[tauri::command]
pub fn get_runtime_profile(db: tauri::State<Db>, workspace_id: String, profile_id: String) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(&format!("SELECT {} FROM runtime_profile WHERE id = ?1 AND workspace_id = ?2", PROFILE_COLS), params![profile_id, ws], profile_from_row).optional().map_err(|e| e.to_string())?;
    row.ok_or_else(|| format!("runtime profile {} 不存在", profile_id))
}

#[tauri::command]
pub fn create_runtime_profile(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, body: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let display_name = get_str(&body, "display_name").unwrap_or_default();
    let command_name = get_str(&body, "command_name").unwrap_or_default();
    if display_name.trim().is_empty() || command_name.trim().is_empty() { return Err("display_name / command_name 不能为空".to_string()); }
    let runtime_type = get_str(&body, "runtime_type");
    let protocol_family = get_str(&body, "protocol_family").unwrap_or_else(|| match runtime_type.as_deref() {
        Some("codex") => "codex".to_string(), Some("gemini") => "gemini".to_string(),
        Some("opencode") => "opencode".to_string(), Some("aider") => "aider".to_string(),
        Some("copilot") => "copilot".to_string(), _ => "claude".to_string(),
    });
    let id = new_id();
    let now = now_iso();
    let fixed = body.get("fixed_args").cloned().unwrap_or_else(|| serde_json::json!([]));
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO runtime_profile (id, workspace_id, display_name, protocol_family, runtime_type, command_name, description, fixed_args, visibility, created_by, enabled, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10,?11,?11)",
            params![id, ws, display_name, protocol_family, runtime_type, command_name, get_str(&body, "description"), fixed.to_string(), get_str(&body, "visibility").unwrap_or_else(|| "workspace".to_string()), if get_bool(&body, "enabled") == Some(false) { 0 } else { 1 }, now],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "runtime_profile", &id);
    let conn = db.0.lock().unwrap();
    conn.query_row(&format!("SELECT {} FROM runtime_profile WHERE id = ?1", PROFILE_COLS), params![id], profile_from_row).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_runtime_profile(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, profile_id: String, patch: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    for (col, key) in [("display_name", "display_name"), ("command_name", "command_name"), ("description", "description"), ("visibility", "visibility")] {
        if let Some(v) = get_str(&patch, key) { sets.push(format!("{} = ?", col)); vals.push(rusqlite::types::Value::from(v)); }
    }
    if let Some(fa) = patch.get("fixed_args") { sets.push("fixed_args = ?".to_string()); vals.push(rusqlite::types::Value::from(fa.to_string())); }
    if let Some(e) = get_bool(&patch, "enabled") { sets.push("enabled = ?".to_string()); vals.push(rusqlite::types::Value::from(if e { 1 } else { 0 })); }
    if sets.is_empty() { return Err("没有可更新的字段".to_string()); }
    sets.push("updated_at = ?".to_string());
    vals.push(rusqlite::types::Value::from(now_iso()));
    vals.push(rusqlite::types::Value::from(profile_id.clone()));
    vals.push(rusqlite::types::Value::from(ws.clone()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let n = conn.execute(&format!("UPDATE runtime_profile SET {} WHERE id = ? AND workspace_id = ?", sets.join(", ")), params_ref.as_slice()).map_err(|e| e.to_string())?;
    if n == 0 { return Err(format!("runtime profile {} 不存在", profile_id)); }
    let row = conn.query_row(&format!("SELECT {} FROM runtime_profile WHERE id = ?1", PROFILE_COLS), params![profile_id], profile_from_row).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "runtime_profile", &profile_id);
    Ok(row)
}

#[tauri::command]
pub fn delete_runtime_profile(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, profile_id: String) -> Result<(), String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM runtime_profile WHERE id = ?1 AND workspace_id = ?2", params![profile_id, ws]).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "runtime_profile", &profile_id);
    Ok(())
}

// ── MCP 服务器 ─────────────────────────────────────────────────────────────
fn mcp_transport(config: &serde_json::Value) -> String {
    if config.get("command").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false) { return "stdio".to_string(); }
    if let Some(url) = config.get("url").and_then(|v| v.as_str()) {
        if url.contains("/sse") || url.ends_with("sse") { return "sse".to_string(); }
        return "http".to_string();
    }
    if config.get("type").and_then(|v| v.as_str()).is_some() { return config["type"].as_str().unwrap_or("stdio").to_string(); }
    "stdio".to_string()
}
fn mcp_server_json(conn: &rusqlite::Connection, id: &str, enabled: Option<bool>) -> Result<serde_json::Value, String> {
    let (ws, name, config_raw, created_at, updated_at) = conn.query_row(
        "SELECT workspace_id, name, config, created_at, updated_at FROM workspace_mcp_server WHERE id = ?1",
        params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
    ).map_err(|_| format!("mcp server {} 不存在", id))?;
    let config = parse_json_pub(&config_raw, serde_json::json!({}));
    let agent_count: i64 = conn.query_row("SELECT COUNT(*) FROM agent_mcp_server WHERE server_id = ?1", params![id], |r| r.get(0)).unwrap_or(0);
    let mut out = serde_json::json!({
        "id": id, "workspace_id": ws, "name": name, "transport": mcp_transport(&config),
        "agent_count": agent_count, "created_at": created_at, "updated_at": updated_at,
    });
    if let Some(e) = enabled { out["enabled"] = serde_json::json!(e); }
    Ok(out)
}

#[tauri::command]
pub fn list_workspace_mcp_servers(db: tauri::State<Db>, workspace_id: String) -> Result<Vec<serde_json::Value>, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut ids = vec![];
    {
        let mut stmt = conn.prepare("SELECT id FROM workspace_mcp_server WHERE workspace_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![ws], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows.flatten() { ids.push(row); }
    }
    ids.iter().map(|id| mcp_server_json(&conn, id, None)).collect()
}

#[tauri::command]
pub fn create_workspace_mcp_server(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, name: String, config: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    if name.trim().is_empty() { return Err("name 不能为空".to_string()); }
    let id = new_id();
    let now = now_iso();
    {
        let conn = db.0.lock().unwrap();
        conn.execute("INSERT INTO workspace_mcp_server (id, workspace_id, name, config, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5)", params![id, ws, name, config.to_string(), now]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "mcp_server", &id);
    let conn = db.0.lock().unwrap();
    mcp_server_json(&conn, &id, None)
}

#[tauri::command]
pub fn update_workspace_mcp_server(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, server_id: String, update: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    if let Some(n) = get_str(&update, "name") { sets.push("name = ?".to_string()); vals.push(rusqlite::types::Value::from(n)); }
    if let Some(c) = update.get("config") { sets.push("config = ?".to_string()); vals.push(rusqlite::types::Value::from(c.to_string())); }
    if sets.is_empty() { return Err("没有可更新的字段".to_string()); }
    sets.push("updated_at = ?".to_string());
    vals.push(rusqlite::types::Value::from(now_iso()));
    vals.push(rusqlite::types::Value::from(server_id.clone()));
    vals.push(rusqlite::types::Value::from(ws.clone()));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    let n = conn.execute(&format!("UPDATE workspace_mcp_server SET {} WHERE id = ? AND workspace_id = ?", sets.join(", ")), params_ref.as_slice()).map_err(|e| e.to_string())?;
    if n == 0 { return Err(format!("mcp server {} 不存在", server_id)); }
    let row = mcp_server_json(&conn, &server_id, None)?;
    drop(conn);
    emit_changed(&app, "mcp_server", &server_id);
    Ok(row)
}

#[tauri::command]
pub fn delete_workspace_mcp_server(app: tauri::AppHandle, db: tauri::State<Db>, workspace_id: String, server_id: String) -> Result<(), String> {
    let ws = { let conn = db.0.lock().unwrap(); resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM agent_mcp_server WHERE server_id = ?1", params![server_id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM workspace_mcp_server WHERE id = ?1 AND workspace_id = ?2", params![server_id, ws]).map_err(|e| e.to_string())?;
    drop(conn);
    emit_changed(&app, "mcp_server", &server_id);
    Ok(())
}

#[tauri::command]
pub fn list_agent_mcp_servers(db: tauri::State<Db>, agent_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut out = vec![];
    let mut stmt = conn.prepare("SELECT s.id, a.enabled FROM workspace_mcp_server s JOIN agent_mcp_server a ON a.server_id = s.id WHERE a.agent_id = ?1 ORDER BY s.created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? != 0))).map_err(|e| e.to_string())?;
    let pairs: Vec<(String, bool)> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    drop(stmt);
    for (id, enabled) in pairs { out.push(mcp_server_json(&conn, &id, Some(enabled))?); }
    Ok(out)
}

#[tauri::command]
pub fn add_agent_mcp_server(app: tauri::AppHandle, db: tauri::State<Db>, agent_id: String, server_id: String) -> Result<Vec<serde_json::Value>, String> {
    {
        let conn = db.0.lock().unwrap();
        conn.execute("INSERT OR IGNORE INTO agent_mcp_server (agent_id, server_id, enabled) VALUES (?1,?2,1)", params![agent_id, server_id]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "agent", &agent_id);
    list_agent_mcp_servers(db, agent_id)
}

#[tauri::command]
pub fn remove_agent_mcp_server(app: tauri::AppHandle, db: tauri::State<Db>, agent_id: String, server_id: String) -> Result<Vec<serde_json::Value>, String> {
    {
        let conn = db.0.lock().unwrap();
        conn.execute("DELETE FROM agent_mcp_server WHERE agent_id = ?1 AND server_id = ?2", params![agent_id, server_id]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "agent", &agent_id);
    list_agent_mcp_servers(db, agent_id)
}

#[tauri::command]
pub fn set_agent_mcp_server_enabled(app: tauri::AppHandle, db: tauri::State<Db>, agent_id: String, server_id: String, enabled: bool) -> Result<Vec<serde_json::Value>, String> {
    {
        let conn = db.0.lock().unwrap();
        conn.execute("INSERT INTO agent_mcp_server (agent_id, server_id, enabled) VALUES (?1,?2,?3) ON CONFLICT(agent_id, server_id) DO UPDATE SET enabled = ?3", params![agent_id, server_id, if enabled { 1 } else { 0 }]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "agent", &agent_id);
    list_agent_mcp_servers(db, agent_id)
}

// ── Agent 技能列表（agent.skills 存 skill id 数组） ───────────────────────
#[tauri::command]
pub fn list_agent_skills(db: tauri::State<Db>, agent_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let raw: String = conn.query_row("SELECT COALESCE(skills, '[]') FROM agent WHERE id = ?1", params![agent_id], |r| r.get(0)).map_err(|_| format!("agent {} 不存在", agent_id))?;
    let ids = parse_json_pub(&raw, serde_json::json!([]));
    let mut out = vec![];
    if let Some(arr) = ids.as_array() {
        for id in arr {
            if let Some(sid) = id.as_str() {
                let row = conn.query_row(
                    "SELECT id, workspace_id, name, description, content, config, created_by, created_at, updated_at FROM skill WHERE id = ?1",
                    params![sid],
                    |r| Ok(serde_json::json!({ "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?, "description": r.get::<_, String>(3)?, "content": r.get::<_, String>(4)?, "config": parse_json_pub(&r.get::<_, String>(5)?, serde_json::json!({})), "created_by": r.get::<_, Option<String>>(6)?, "created_at": r.get::<_, String>(7)?, "updated_at": r.get::<_, String>(8)? })),
                ).optional().map_err(|e| e.to_string())?;
                if let Some(row) = row { out.push(row); }
            }
        }
    }
    Ok(out)
}

// ── 技能导入 / 刷新 ────────────────────────────────────────────────────────
fn fetch_url_text(url: &str) -> Result<String, String> {
    // 本地版没有 HTTP 客户端依赖；用系统 curl（与 CLI 适配器同样的外部工具策略）。
    let out = std::process::Command::new("curl")
        .args(["-sL", "--max-time", "20", url])
        .output()
        .map_err(|e| format!("curl 不可用，无法从 URL 导入：{}", e))?;
    if !out.status.success() { return Err(format!("下载失败（curl exit {}）：{}", out.status, url)); }
    String::from_utf8(out.stdout).map_err(|_| "下载内容不是有效 UTF-8 文本".to_string())
}

/// SKILL.md frontmatter 解析：返回 (name, description)。无 frontmatter 时用标题/默认。
fn parse_skill_md(content: &str) -> (String, String) {
    let mut name = String::new();
    let mut desc = String::new();
    let mut body = content;
    if let Some(rest) = content.strip_prefix("---") {
        if let Some(end) = rest.find("\n---") {
            let fm = &rest[..end];
            for line in fm.lines() {
                if let Some(v) = line.strip_prefix("name:") { name = v.trim().trim_matches(|c| c == '"' || c == '\'').to_string(); }
                if let Some(v) = line.strip_prefix("description:") { desc = v.trim().trim_matches(|c| c == '"' || c == '\'').to_string(); }
            }
            body = &rest[end + 4..];
        }
    }
    if name.is_empty() {
        for line in body.lines() {
            if let Some(t) = line.strip_prefix("# ") { name = t.trim().to_string(); break; }
        }
    }
    if name.is_empty() { name = "imported-skill".to_string(); }
    (name, desc)
}

fn skill_json(conn: &rusqlite::Connection, id: &str) -> Result<serde_json::Value, String> {
    conn.query_row(
        "SELECT id, workspace_id, name, description, content, config, created_by, created_at, updated_at, files FROM skill WHERE id = ?1",
        params![id],
        |r| Ok(serde_json::json!({ "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?, "description": r.get::<_, String>(3)?, "content": r.get::<_, String>(4)?, "config": parse_json_pub(&r.get::<_, String>(5)?, serde_json::json!({})), "created_by": r.get::<_, Option<String>>(6)?, "created_at": r.get::<_, String>(7)?, "updated_at": r.get::<_, String>(8)?, "files": parse_json_pub(&r.get::<_, Option<String>>(9)?.unwrap_or_else(|| "[]".to_string()), serde_json::json!([])) })),
    ).map_err(|_| format!("skill {} 不存在", id))
}

fn find_skill_by_name(conn: &rusqlite::Connection, name: &str) -> Option<String> {
    conn.query_row("SELECT id FROM skill WHERE name = ?1", params![name], |r| r.get::<_, String>(0)).ok()
}

#[tauri::command]
pub fn import_skill(app: tauri::AppHandle, db: tauri::State<Db>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let url = get_str(&data, "url").unwrap_or_default();
    if url.trim().is_empty() { return Err("url 不能为空".to_string()); }
    let lower = url.to_lowercase();
    if lower.contains("skills.sh") || lower.contains("clawhub") {
        return Err("本地版不支持 skills.sh / ClawHub 注册表导入，请用 GitHub 直链或归档文件导入".to_string());
    }
    // GitHub blob 页转 raw 直链；raw 直链与其它 .md 直链直接下载。
    // URL 结构：owner/repo/blob/<branch>/<path...>，即 parts[2]=="blob"、
    // parts[3] 是分支、parts[4..] 是路径（旧代码把 parts[3] 当 blob、parts[4]
    // 当分支，会把分支名丢进路径，下载 404）。
    let fetch_url = if let Some(cap) = url.strip_prefix("https://github.com/").or_else(|| url.strip_prefix("http://github.com/")) {
        let parts: Vec<&str> = cap.split('/').collect();
        if parts.len() >= 4 && parts[2] == "blob" {
            format!("https://raw.githubusercontent.com/{}/{}/{}/{}", parts[0], parts[1], parts[3], parts[4..].join("/"))
        } else { url.clone() }
    } else { url.clone() };
    let content = fetch_url_text(&fetch_url)?;
    let (name, desc) = parse_skill_md(&content);
    let ws = { let conn = db.0.lock().unwrap(); conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default() };
    if find_skill_by_name(&db.0.lock().unwrap(), &name).is_some() {
        return Err(format!("同名 skill 已存在：{}（请先删除或改名）", name));
    }
    let id = new_id();
    let now = now_iso();
    let config = serde_json::json!({ "origin": url, "fetched_from": fetch_url });
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO skill (id, workspace_id, name, description, content, config, created_by, created_at, updated_at, files) VALUES (?1,?2,?3,?4,?5,?6,NULL,?7,?7,'[]')",
            params![id, ws, name, desc, content, config.to_string(), now],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "skill", &id);
    let conn = db.0.lock().unwrap();
    skill_json(&conn, &id)
}

#[tauri::command]
pub fn refresh_skill(app: tauri::AppHandle, db: tauri::State<Db>, id: String) -> Result<serde_json::Value, String> {
    let (origin, _name) = {
        let conn = db.0.lock().unwrap();
        let row = conn.query_row("SELECT config, name FROM skill WHERE id = ?1", params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|_| format!("skill {} 不存在", id))?;
        let cfg = parse_json_pub(&row.0, serde_json::json!({}));
        // 优先 fetched_from（GitHub blob 转换后的 raw 直链）；origin 是用户输入的
        // 原始 URL（blob 页），直接下载会拿到 HTML。无 fetched_from 时回退 origin。
        let fetch_url = cfg.get("fetched_from").and_then(|v| v.as_str()).unwrap_or("")
            .to_string();
        let origin = if !fetch_url.is_empty() { fetch_url } else {
            cfg.get("origin").and_then(|v| v.as_str()).unwrap_or("").to_string()
        };
        (origin, row.1)
    };
    if origin.starts_with("http") {
        // 有 origin（URL 导入的 skill）：重新下载覆盖内容。
        let content = fetch_url_text(&origin)?;
        let (name, desc) = parse_skill_md(&content);
        let conn = db.0.lock().unwrap();
        conn.execute("UPDATE skill SET name = ?1, description = ?2, content = ?3, updated_at = ?4 WHERE id = ?5", params![name, desc, content, now_iso(), id]).map_err(|e| e.to_string())?;
        drop(conn);
        emit_changed(&app, "skill", &id);
    }
    // 无 origin（手动/本地导入）：没有可重新下载的来源，原样返回。
    let conn = db.0.lock().unwrap();
    skill_json(&conn, &id)
}

/// ZIP 读取：支持 stored（method 0）与 deflate（method 8，flate2 原始 deflate 流）。
/// 单条目解压上限 32 MiB（防 zip bomb）；加密条目（flag bit 0）明确拒绝。
fn zip_entries(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    // 找 EOCD。注意：无注释的 zip 的 EOCD 恰好在 len-22 处，范围必须包含该位置
    //（旧代码用 ..len-22，把最常见的情况排除了，导致正常 zip 全部报“找不到 EOCD”）。
    if data.len() < 22 { return Err("不是有效的 zip 文件（太小）".to_string()); }
    let scan_start = data.len().saturating_sub(65557);
    let mut eocd = None;
    for i in (scan_start..=data.len() - 22).rev() {
        if &data[i..i + 4] == b"PK\x05\x06" { eocd = Some(i); break; }
    }
    let eocd = eocd.ok_or("不是有效的 zip 文件（找不到 EOCD）")?;
    let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
    let total = u16_at(eocd + 10);
    let cd_off = u32_at(eocd + 16);
    let mut out = vec![];
    let mut p = cd_off;
    for _ in 0..total {
        if p + 46 > data.len() || &data[p..p + 4] != b"PK\x01\x02" { return Err("zip 中央目录损坏".to_string()); }
        let flags = u16_at(p + 8);
        if flags & 0x1 != 0 { return Err(format!("zip 条目 {} 已加密，本地版不支持", String::from_utf8_lossy(&data[p + 46..p + 46 + u16_at(p + 28)]))); }
        let method = u16_at(p + 10);
        let comp_size = u32_at(p + 20);
        let name_len = u16_at(p + 28);
        let extra_len = u16_at(p + 30);
        let comment_len = u16_at(p + 32);
        let local_off = u32_at(p + 42);
        let name = String::from_utf8_lossy(&data[p + 46..p + 46 + name_len]).to_string();
        // local header：PK\x03\x04 + 26 字节 + name + extra → 数据。
        // 尺寸取中央目录（data descriptor 场景下 local header 的尺寸为 0）。
        let lh = local_off;
        if lh + 30 > data.len() || &data[lh..lh + 4] != b"PK\x03\x04" { return Err(format!("zip 局部头损坏：{}", name)); }
        let lh_name = u16_at(lh + 26);
        let lh_extra = u16_at(lh + 28);
        let start = lh + 30 + lh_name + lh_extra;
        let end = start + comp_size;
        if end > data.len() { return Err(format!("zip 数据越界：{}", name)); }
        if method == 0 {
            out.push((name, data[start..end].to_vec()));
        } else if method == 8 {
            // ZIP 用原始 deflate 流（RFC 1951，无 zlib 头）→ DeflateDecoder。
            use std::io::Read as _;
            let mut dec = flate2::read::DeflateDecoder::new(&data[start..end]);
            let mut buf = Vec::new();
            dec.read_to_end(&mut buf).map_err(|e| format!("zip 条目 {} 解压失败：{}", name, e))?;
            if buf.len() > 32 * 1024 * 1024 { return Err(format!("zip 条目 {} 解压后超过 32 MiB 上限", name)); }
            out.push((name, buf));
        } else {
            return Err(format!("zip 条目 {} 使用不支持的压缩方法 {}", name, method));
        }
        p += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

#[tauri::command]
pub fn import_skill_archive(app: tauri::AppHandle, db: tauri::State<Db>, data_base64: String, filename: String, on_conflict: Option<String>) -> Result<serde_json::Value, String> {
    use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
    let bytes = B64.decode(data_base64.trim()).map_err(|e| format!("base64 解码失败：{}", e))?;
    let entries = zip_entries(&bytes)?;
    // 找 SKILL.md（最浅路径优先）
    let mut skill_md: Option<&(String, Vec<u8>)> = None;
    for e in &entries {
        let lower = e.0.to_lowercase();
        if lower.ends_with("skill.md") {
            match skill_md {
                None => skill_md = Some(e),
                Some(cur) if e.0.matches('/').count() < cur.0.matches('/').count() => skill_md = Some(e),
                _ => {}
            }
        }
    }
    let (md_path, md_bytes) = skill_md.ok_or("归档里没有 SKILL.md")?;
    let content = String::from_utf8(md_bytes.clone()).map_err(|_| "SKILL.md 不是有效 UTF-8".to_string())?;
    let (mut name, desc) = parse_skill_md(&content);
    if name == "imported-skill" {
        name = filename.trim_end_matches(".zip").trim_end_matches(".skill").to_string();
        if name.is_empty() { name = "imported-skill".to_string(); }
    }
    let _ = md_path;
    let files: Vec<serde_json::Value> = entries.iter().filter_map(|(path, bytes)| {
        if path.ends_with('/') { return None; }
        let text = String::from_utf8(bytes.clone()).ok()?;
        Some(serde_json::json!({ "path": path, "content": text }))
    }).collect();
    let ws = { let conn = db.0.lock().unwrap(); conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default() };
    let conflict = get_str(&serde_json::json!({ "on_conflict": on_conflict.clone() }), "on_conflict").unwrap_or_else(|| "fail".to_string());
    let existing = find_skill_by_name(&db.0.lock().unwrap(), &name);
    let now = now_iso();
    let config = serde_json::json!({ "origin": "archive", "archive_filename": filename });
    if let Some(existing_id) = existing {
        match conflict.as_str() {
            "skip" => { let conn = db.0.lock().unwrap(); return skill_json(&conn, &existing_id); }
            "overwrite" => {
                let conn = db.0.lock().unwrap();
                conn.execute("UPDATE skill SET description = ?1, content = ?2, config = ?3, files = ?4, updated_at = ?5 WHERE id = ?6", params![desc, content, config.to_string(), serde_json::json!(files).to_string(), now, existing_id]).map_err(|e| e.to_string())?;
                drop(conn);
                emit_changed(&app, "skill", &existing_id);
                let conn = db.0.lock().unwrap();
                return skill_json(&conn, &existing_id);
            }
            "rename" => {
                let mut i = 2;
                loop {
                    let cand = format!("{}-{}", name, i);
                    if find_skill_by_name(&db.0.lock().unwrap(), &cand).is_none() { name = cand; break; }
                    i += 1;
                }
            }
            _ => return Err(format!("同名 skill 已存在：{}（on_conflict=fail）", name)),
        }
    }
    let id = new_id();
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO skill (id, workspace_id, name, description, content, config, created_by, created_at, updated_at, files) VALUES (?1,?2,?3,?4,?5,?6,NULL,?7,?7,?8)",
            params![id, ws, name, desc, content, config.to_string(), now, serde_json::json!(files).to_string()],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "skill", &id);
    let conn = db.0.lock().unwrap();
    skill_json(&conn, &id)
}

// ── Cron 预览 ──────────────────────────────────────────────────────────────
fn parse_cron_field(spec: &str, min: i64, max: i64) -> Result<Vec<bool>, String> {
    let mut out = vec![false; (max - min + 1) as usize];
    if spec == "*" { for v in out.iter_mut() { *v = true; } return Ok(out); }
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<i64>().map_err(|_| format!("cron 步长非法：{}", part))?),
            None => (part, 1),
        };
        if step <= 0 { return Err(format!("cron 步长非法：{}", part)); }
        let (lo, hi) = if range == "*" { (min, max) }
            else if let Some((a, b)) = range.split_once('-') {
                (a.parse::<i64>().map_err(|_| format!("cron 字段非法：{}", part))?, b.parse::<i64>().map_err(|_| format!("cron 字段非法：{}", part))?)
            } else {
                let v = range.parse::<i64>().map_err(|_| format!("cron 字段非法：{}", part))?;
                // 带步长的单个值（5/15）按标准 cron 语义 = 5-max/15，不是只取 5。
                if part.contains('/') { (v, max) } else { (v, v) }
            };
        if lo < min || hi > max || lo > hi { return Err(format!("cron 字段超出范围：{}（{}–{}）", part, min, max)); }
        let mut v = lo;
        while v <= hi { out[(v - min) as usize] = true; v += step; }
    }
    Ok(out)
}

/// 解析时区：IANA 名（chrono-tz，含完整 DST 规则）或 UTC/GMT。
/// 未知时区直接报错，不静默按 UTC 处理（旧实现把未知时区当 +0 且把
/// Asia/Kolkata 近似为 +5，预览时间会错半小时以上）。
pub(crate) fn parse_tz(tz: &str) -> Result<chrono_tz::Tz, String> {
    let t = tz.trim();
    if t.is_empty() || t == "UTC" || t == "Etc/UTC" || t == "GMT" || t == "Etc/GMT" {
        return Ok(chrono_tz::UTC);
    }
    t.parse::<chrono_tz::Tz>()
        .map_err(|_| format!("未知时区：{}，请使用 IANA 时区名（如 Asia/Shanghai）", tz))
}

/// 解析本库写入的 started_at（now_iso：RFC3339 Z 格式）。
pub(crate) fn parse_utc(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::TimeZone as _;
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // 兼容旧格式 "YYYY-MM-DD HH:MM:SS"（当作 UTC）。
    chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
        .map(|n| chrono::Utc.from_utc_datetime(&n))
}

#[tauri::command]
pub fn cron_preview(expr: String, tz: String) -> Result<serde_json::Value, String> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 { return Err(format!("cron 表达式须为 5 段（分 时 日 月 周），得到 {} 段", parts.len())); }
    let mins = parse_cron_field(parts[0], 0, 59)?;
    let hours = parse_cron_field(parts[1], 0, 23)?;
    let doms = parse_cron_field(parts[2], 1, 31)?;
    let months = parse_cron_field(parts[3], 1, 12)?;
    let mut dows = parse_cron_field(parts[4], 0, 7)?;
    // 周日 7 归一到 0
    if dows.len() == 8 && dows[7] { dows[0] = true; }
    let tz = parse_tz(&tz)?;
    // 从下一分钟开始；先截断到分钟边界，避免 now 的秒级残差污染输出。
    use chrono::Timelike as _;
    let start = chrono::Utc::now().with_timezone(&tz);
    let start = start.with_second(0).and_then(|d| d.with_nanosecond(0)).unwrap_or(start)
        + chrono::Duration::minutes(1);
    let mut next: Vec<String> = vec![];
    let mut cur = start;
    // 最多扫一年。逐分钟推进的是绝对时间，chrono-tz 在每次转换时重算本地
    // 时间：春切缺失的小时自然跳过，秋切重复的小时会触发两次（与 Vixie cron 一致）。
    for _ in 0..(366 * 24 * 60) {
        use chrono::{Datelike, Timelike};
        let dom_star = parts[2] == "*";
        let dow_star = parts[4] == "*";
        let dow_match = dows[cur.weekday().num_days_from_sunday() as usize];
        let day_match = if dom_star && dow_star { true }
            else if dom_star { dow_match }
            else if dow_star { doms[(cur.day() - 1) as usize] }
            else { doms[(cur.day() - 1) as usize] || dow_match };
        if mins[cur.minute() as usize] && hours[cur.hour() as usize] && months[(cur.month() - 1) as usize] && day_match {
            next.push(cur.with_timezone(&chrono::Utc).format("%Y-%m-%dT%H:%M:%SZ").to_string());
            if next.len() >= 5 { break; }
        }
        cur += chrono::Duration::minutes(1);
    }
    Ok(serde_json::json!({ "next_runs": next }))
}

// ── 运行时用量（本地版不计量 token：返回真实任务数，token 均为 0） ───────
#[tauri::command]
pub fn get_runtime_task_activity(db: tauri::State<Db>, runtime_id: String, tz: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let _ = runtime_id;
    let tz = parse_tz(tz.as_deref().unwrap_or("UTC"))?;
    // “今天”按请求时区的本地日期算（旧实现按 UTC 今天，+8 时区凌晨会错一天）。
    let today = chrono::Utc::now().with_timezone(&tz).format("%Y-%m-%d").to_string();
    let conn = db.0.lock().unwrap();
    let mut counts = [0i64; 24];
    {
        let mut stmt = conn.prepare("SELECT started_at FROM run WHERE substr(started_at, 1, 10) >= date('now', '-1 day')").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            if let Some(dt) = parse_utc(&row) {
                let local = dt.with_timezone(&tz);
                use chrono::Datelike;
                if local.format("%Y-%m-%d").to_string() == today {
                    use chrono::Timelike;
                    counts[local.hour() as usize] += 1;
                }
            }
        }
    }
    Ok((0..24).map(|hour| serde_json::json!({ "hour": hour, "count": counts[hour] })).collect())
}

#[tauri::command]
pub fn get_runtime_usage(db: tauri::State<Db>, runtime_id: String, days: Option<i64>, tz: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let days = days.unwrap_or(30).clamp(1, 365);
    let tz = parse_tz(tz.as_deref().unwrap_or("UTC"))?;
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let conn = db.0.lock().unwrap();
    let mut by_date: std::collections::BTreeMap<String, i64> = Default::default();
    {
        let mut stmt = conn.prepare("SELECT started_at FROM run WHERE substr(started_at, 1, 10) >= date('now', ?1)").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![format!("-{} days", days + 1)], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            if let Some(dt) = parse_utc(&row) {
                if dt >= cutoff {
                    by_date.entry(dt.with_timezone(&tz).format("%Y-%m-%d").to_string()).and_modify(|c| *c += 1).or_insert(1);
                }
            }
        }
    }
    Ok(by_date.into_iter().map(|(date, count)| serde_json::json!({ "runtime_id": runtime_id, "date": date, "provider": "", "model": "", "input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0, "task_count": count })).collect())
}

#[tauri::command]
pub fn get_runtime_usage_by_agent(db: tauri::State<Db>, runtime_id: String, days: Option<i64>, tz: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    // 按 agent 分组，与时区无关（tz 仅保留签名兼容）。
    let _ = tz;
    let _ = runtime_id;
    let days = days.unwrap_or(30).clamp(1, 365);
    let conn = db.0.lock().unwrap();
    let mut out = vec![];
    let mut stmt = conn.prepare("SELECT agent_id, COUNT(*) FROM run WHERE started_at >= datetime('now', ?1) GROUP BY agent_id ORDER BY 2 DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![format!("-{} days", days)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?;
    for (agent_id, count) in rows.flatten() {
        out.push(serde_json::json!({ "agent_id": agent_id, "provider": "", "model": "", "input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0, "task_count": count }));
    }
    Ok(out)
}

#[tauri::command]
pub fn get_runtime_usage_by_hour(db: tauri::State<Db>, runtime_id: String, days: Option<i64>, tz: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let _ = runtime_id;
    let days = days.unwrap_or(30).clamp(1, 365);
    let tz = parse_tz(tz.as_deref().unwrap_or("UTC"))?;
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days);
    let conn = db.0.lock().unwrap();
    let mut by_hour = [0i64; 24];
    {
        let mut stmt = conn.prepare("SELECT started_at FROM run WHERE substr(started_at, 1, 10) >= date('now', ?1)").map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![format!("-{} days", days + 1)], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            if let Some(dt) = parse_utc(&row) {
                if dt >= cutoff {
                    use chrono::Timelike;
                    by_hour[dt.with_timezone(&tz).hour() as usize] += 1;
                }
            }
        }
    }
    Ok((0..24).map(|hour| serde_json::json!({ "hour": hour, "model": "", "input_tokens": 0, "output_tokens": 0, "cache_read_tokens": 0, "cache_write_tokens": 0, "task_count": by_hour[hour] })).collect())
}

// ── 配额（本地版无云端配额） ───────────────────────────────────────────────
#[tauri::command]
pub fn get_autopilot_quota_usage() -> Result<serde_json::Value, String> {
    // 本地版不设配额：action=off，其余全部 null。
    Ok(serde_json::json!({ "action": "off", "used": null, "reserved": null, "total": null, "limit": null, "reached": false, "period_start": null, "period_end": null, "reset_at": null, "blocked_counts": null }))
}

#[tauri::command]
pub fn get_issue_limit_usage() -> Result<Option<serde_json::Value>, String> {
    // 返回 null = 无云端套餐上限（IssueLimitUsage | null）。
    Ok(None)
}

#[tauri::command]
pub fn get_issue_usage(db: tauri::State<Db>, issue_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM run WHERE issue_id = ?1", params![issue_id], |r| r.get(0)).unwrap_or(0);
    let terminal: i64 = conn.query_row("SELECT COUNT(*) FROM run WHERE issue_id = ?1 AND status IN ('succeeded','failed','cancelled')", params![issue_id], |r| r.get(0)).unwrap_or(0);
    Ok(serde_json::json!({
        "total_input_tokens": 0, "total_output_tokens": 0, "total_cache_read_tokens": 0, "total_cache_write_tokens": 0,
        "task_count": total, "terminal_task_count": terminal, "metered_task_count": 0, "unreported_task_count": 0,
    }))
}

// ── Models / Local Skills 异步任务（本地同步完成） ────────────────────────
fn store_job(conn: &rusqlite::Connection, runtime_id: &str, kind: &str, payload: &serde_json::Value) -> String {
    let id = new_id();
    let now = now_iso();
    let _ = conn.execute(
        "INSERT INTO runtime_job (id, runtime_id, kind, status, payload, created_at, updated_at) VALUES (?1,?2,?3,'completed',?4,?5,?5)",
        params![id, runtime_id, kind, payload.to_string(), now],
    );
    id
}
fn get_job(conn: &rusqlite::Connection, runtime_id: &str, request_id: &str, kind: &str) -> Result<serde_json::Value, String> {
    conn.query_row("SELECT payload FROM runtime_job WHERE id = ?1 AND runtime_id = ?2 AND kind = ?3", params![request_id, runtime_id, kind], |r| r.get::<_, String>(0))
        .map(|raw| parse_json_pub(&raw, serde_json::json!({})))
        .map_err(|_| format!("{} 任务 {} 不存在", kind, request_id))
}

#[tauri::command]
pub fn initiate_list_models(db: tauri::State<Db>, runtime_id: String, force: Option<bool>) -> Result<serde_json::Value, String> {
    // force：云端强制重新发现；本地版每次都实时读 agent 配置，忽略。
    let _ = force;
    // 本地版不做 daemon 发现：models = 各 agent 已配置的 model 去重。
    // supported=false：spawn 目前不把 model 传给 CLI，UI 据此禁用下拉（诚实语义）。
    let conn = db.0.lock().unwrap();
    let mut models: Vec<String> = vec![];
    {
        let mut stmt = conn.prepare("SELECT DISTINCT model FROM agent WHERE model != '' ORDER BY model").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        for row in rows.flatten() { models.push(row); }
    }
    let now = now_iso();
    let model_objs: Vec<serde_json::Value> = models.iter().map(|m| serde_json::json!({ "id": m, "name": m })).collect();
    let payload = serde_json::json!({ "id": "", "runtime_id": runtime_id, "status": "completed", "models": model_objs, "supported": false, "cached": false, "created_at": now, "updated_at": now });
    let id = store_job(&conn, &runtime_id, "list_models", &payload);
    let mut out = payload.clone();
    out["id"] = serde_json::json!(id);
    Ok(out)
}

#[tauri::command]
pub fn get_list_models_result(db: tauri::State<Db>, runtime_id: String, request_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut payload = get_job(&conn, &runtime_id, &request_id, "list_models")?;
    payload["id"] = serde_json::json!(request_id);
    Ok(payload)
}

fn scan_local_skills() -> Vec<serde_json::Value> {
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() { return vec![]; }
    let roots: [(&str, &str, &str); 5] = [
        (".claude/skills", "claude", "provider"),
        (".codex/skills", "codex", "provider"),
        (".config/opencode/skills", "opencode", "provider"),
        (".gemini/skills", "gemini", "provider"),
        (".agents/skills", "universal", "universal"),
    ];
    let mut out = vec![];
    for (rel, provider, root_kind) in roots {
        let dir = std::path::Path::new(&home).join(rel);
        let entries = match std::fs::read_dir(&dir) { Ok(e) => e, Err(_) => continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() { continue; }
            let skill_md = path.join("SKILL.md");
            if !skill_md.is_file() { continue; }
            let content = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let (name, desc) = parse_skill_md(&content);
            let key = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let file_count = walk_count(&path, 0);
            out.push(serde_json::json!({
                "key": key, "name": name, "description": desc,
                "source_path": path.to_string_lossy().to_string(), "provider": provider,
                "root": root_kind, "file_count": file_count,
            }));
        }
    }
    out
}
fn walk_count(dir: &std::path::Path, depth: usize) -> i64 {
    if depth > 4 { return 0; }
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_file() { n += 1; } else if p.is_dir() { n += walk_count(&p, depth + 1); }
        }
    }
    n
}

#[tauri::command]
pub fn initiate_list_local_skills(db: tauri::State<Db>, runtime_id: String) -> Result<serde_json::Value, String> {
    let skills = scan_local_skills();
    let now = now_iso();
    let payload = serde_json::json!({ "id": "", "runtime_id": runtime_id, "status": "completed", "skills": skills, "supported": true, "mcp_servers": [], "mcp_supported": false, "created_at": now, "updated_at": now });
    let conn = db.0.lock().unwrap();
    let id = store_job(&conn, &runtime_id, "list_local_skills", &payload);
    let mut out = payload.clone();
    out["id"] = serde_json::json!(id);
    Ok(out)
}

#[tauri::command]
pub fn get_list_local_skills_result(db: tauri::State<Db>, runtime_id: String, request_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut payload = get_job(&conn, &runtime_id, &request_id, "list_local_skills")?;
    payload["id"] = serde_json::json!(request_id);
    Ok(payload)
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() { copy_dir_recursive(&src_path, &dst_path)?; }
        else { std::fs::copy(&src_path, &dst_path)?; }
    }
    Ok(())
}

#[tauri::command]
pub fn initiate_import_local_skill(app: tauri::AppHandle, db: tauri::State<Db>, runtime_id: String, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let skill_key = get_str(&data, "skill_key").unwrap_or_default();
    let now = now_iso();
    let mut base = serde_json::json!({ "id": "", "runtime_id": runtime_id, "skill_key": skill_key, "status": "completed", "created_at": now, "updated_at": now });
    for (k, v) in [("name", get_str(&data, "name")), ("description", get_str(&data, "description")), ("action", get_str(&data, "action")), ("target_skill_id", get_str(&data, "target_skill_id"))] {
        if let Some(val) = v { base[k] = serde_json::json!(val); }
    }
    if let Some(v) = get_bool(&data, "supports_conflict") { base["supports_conflict"] = serde_json::json!(v); }
    // 找源目录
    let found = scan_local_skills().into_iter().find(|s| s.get("key").and_then(|k| k.as_str()) == Some(skill_key.as_str()));
    let source_path = match found { Some(s) => s.get("source_path").and_then(|v| v.as_str()).unwrap_or("").to_string(), None => String::new() };
    if source_path.is_empty() {
        base["status"] = serde_json::json!("failed");
        base["error"] = serde_json::json!(format!("本地技能未找到：{}（请先在对应 CLI 的 skills 目录下放 SKILL.md）", skill_key));
        let conn = db.0.lock().unwrap();
        let id = store_job(&conn, &runtime_id, "import_local_skill", &base);
        base["id"] = serde_json::json!(id);
        return Ok(base);
    }
    let src_dir = std::path::Path::new(&source_path);
    let md_content = std::fs::read_to_string(src_dir.join("SKILL.md")).unwrap_or_default();
    let (mut name, mut desc) = parse_skill_md(&md_content);
    if let Some(n) = get_str(&data, "name") { if !n.trim().is_empty() { name = n; } }
    if let Some(d) = get_str(&data, "description") { desc = d; }
    let overwrite = get_str(&data, "action").as_deref() == Some("overwrite");
    let target = get_str(&data, "target_skill_id");
    let existing = target.clone().or_else(|| find_skill_by_name(&db.0.lock().unwrap(), &name));
    if let Some(existing_id) = &existing {
        if !overwrite && target.is_none() {
            if get_bool(&data, "supports_conflict") == Some(true) {
                base["status"] = serde_json::json!("conflict");
                base["conflict"] = serde_json::json!({ "existing_skill_id": existing_id, "can_overwrite": true });
                let conn = db.0.lock().unwrap();
                let id = store_job(&conn, &runtime_id, "import_local_skill", &base);
                base["id"] = serde_json::json!(id);
                return Ok(base);
            }
            base["status"] = serde_json::json!("failed");
            base["error"] = serde_json::json!(format!("同名 skill 已存在：{}（传 action=overwrite 覆盖）", name));
            let conn = db.0.lock().unwrap();
            let id = store_job(&conn, &runtime_id, "import_local_skill", &base);
            base["id"] = serde_json::json!(id);
            return Ok(base);
        }
    }
    // 拷贝目录到 app_data/skills/<id>/，建 skill 行
    let ws = { let conn = db.0.lock().unwrap(); conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default() };
    let skill_id = existing.clone().unwrap_or_else(new_id);
    if let Ok(dir) = app.path().app_data_dir() {
        let dst = dir.join("skills").join(&skill_id);
        if let Err(e) = copy_dir_recursive(src_dir, &dst) {
            base["status"] = serde_json::json!("failed");
            base["error"] = serde_json::json!(format!("拷贝技能目录失败：{}", e));
            let conn = db.0.lock().unwrap();
            let id = store_job(&conn, &runtime_id, "import_local_skill", &base);
            base["id"] = serde_json::json!(id);
            return Ok(base);
        }
    }
    let files: Vec<serde_json::Value> = walk_files(src_dir, src_dir).into_iter().filter_map(|(rel, full)| {
        let text = std::fs::read_to_string(&full).ok()?;
        Some(serde_json::json!({ "path": rel, "content": text }))
    }).collect();
    let config = serde_json::json!({ "origin": "local", "source_path": source_path });
    {
        let conn = db.0.lock().unwrap();
        if existing.is_some() {
            conn.execute("UPDATE skill SET name = ?1, description = ?2, content = ?3, config = ?4, files = ?5, updated_at = ?6 WHERE id = ?7", params![name, desc, md_content, config.to_string(), serde_json::json!(files).to_string(), now, skill_id]).map_err(|e| e.to_string())?;
            base["action"] = serde_json::json!("overwrite");
        } else {
            conn.execute("INSERT INTO skill (id, workspace_id, name, description, content, config, created_by, created_at, updated_at, files) VALUES (?1,?2,?3,?4,?5,?6,NULL,?7,?7,?8)", params![skill_id, ws, name, desc, md_content, config.to_string(), now, serde_json::json!(files).to_string()]).map_err(|e| e.to_string())?;
        }
    }
    emit_changed(&app, "skill", &skill_id);
    {
        let conn = db.0.lock().unwrap();
        base["skill"] = skill_json(&conn, &skill_id).unwrap_or(serde_json::json!(null));
        let id = store_job(&conn, &runtime_id, "import_local_skill", &base);
        base["id"] = serde_json::json!(id);
    }
    Ok(base)
}

fn walk_files(root: &std::path::Path, dir: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    let mut out = vec![];
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walk_files(root, &p)); }
            else if p.is_file() {
                let rel = p.strip_prefix(root).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
                out.push((rel, p));
            }
        }
    }
    out
}

#[tauri::command]
pub fn get_import_local_skill_result(db: tauri::State<Db>, runtime_id: String, request_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut payload = get_job(&conn, &runtime_id, &request_id, "import_local_skill")?;
    payload["id"] = serde_json::json!(request_id);
    Ok(payload)
}

// ── App Config（auth-initializer 用；本地版无 CDN/附件公网地址） ───────────
#[tauri::command]
pub fn get_config() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "attachment_public_url": "", "cdn_domain": null, "cdn_signed": false }))
}

// ── Onboarding（本地版：状态存 workspace.settings.onboarding；返回 User 形状，
//    因为 onboarding store 会把 patchOnboarding 的返回值 setUser）。 ─────────
fn local_user_json(conn: &rusqlite::Connection, ws: &str) -> serde_json::Value {
    let settings = workspace_settings(conn, ws);
    let onboarding = settings.get("onboarding").cloned().unwrap_or_else(|| serde_json::json!({}));
    let (user_id, name, email, avatar_url, created_at) = conn.query_row(
        "SELECT user_id, name, email, avatar_url, created_at FROM member WHERE workspace_id = ?1 ORDER BY created_at LIMIT 1",
        params![ws],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, String>(4)?)),
    ).unwrap_or_else(|_| ("u-local".to_string(), "本地用户".to_string(), "local@multica.local".to_string(), None, now_iso()));
    serde_json::json!({
        "id": user_id, "name": name, "email": email, "avatar_url": avatar_url,
        "onboarded_at": onboarding.get("completed_at").and_then(|v| v.as_str()).unwrap_or(&created_at),
        "onboarding_questionnaire": onboarding.get("questionnaire").cloned().unwrap_or_else(|| serde_json::json!({})),
        "starter_content_state": "imported",
    })
}

#[tauri::command]
pub fn mark_onboarding_complete(app: tauri::AppHandle, db: tauri::State<Db>, payload: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let ws = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default()
    };
    {
        let conn = db.0.lock().unwrap();
        let mut settings = workspace_settings(&conn, &ws);
        let mut ob = settings.get("onboarding").cloned().unwrap_or_else(|| serde_json::json!({}));
        ob["completed"] = serde_json::json!(true);
        ob["completed_at"] = serde_json::json!(now_iso());
        if let Some(p) = payload { ob["completion_payload"] = p; }
        settings["onboarding"] = ob;
        conn.execute("UPDATE workspace SET settings = ?1 WHERE id = ?2", params![settings.to_string(), ws]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "workspace", &ws);
    let conn = db.0.lock().unwrap();
    Ok(local_user_json(&conn, &ws))
}

#[tauri::command]
pub fn patch_onboarding(app: tauri::AppHandle, db: tauri::State<Db>, data: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT id FROM workspace ORDER BY created_at LIMIT 1", [], |r| r.get::<_, String>(0)).unwrap_or_default()
    };
    {
        let conn = db.0.lock().unwrap();
        let mut settings = workspace_settings(&conn, &ws);
        let mut ob = settings.get("onboarding").cloned().unwrap_or_else(|| serde_json::json!({}));
        if let Some(patch) = data.as_object() {
            for (k, v) in patch { ob[k] = v.clone(); }
        }
        settings["onboarding"] = ob;
        conn.execute("UPDATE workspace SET settings = ?1 WHERE id = ?2", params![settings.to_string(), ws]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "workspace", &ws);
    let conn = db.0.lock().unwrap();
    Ok(local_user_json(&conn, &ws))
}
