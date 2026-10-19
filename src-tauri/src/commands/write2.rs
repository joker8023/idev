//! 第二批写路径命令（005 迁移）：status / view / pin / property / label /
//! project resource / squad 成员角色 / inbox / chat / comment / reaction /
//! subscribe / quick action / batch / rerun / search / wakeup / autopilot /
//! builder / agent skill / env / attachment / table / notif / update / dashboard。
//!
//! 约定与 expanded.rs 相同：写命令成功后 emit_changed，前端失效缓存。

use rusqlite::{params, Row};
use tauri::{Emitter, State};
use crate::db::Db;
use crate::models::*;
use super::{emit_changed, resolve_workspace_id, parse_json_pub};

fn j(v: serde_json::Value) -> String {
    serde_json::to_string(&v).unwrap_or_else(|_| "{}".into())
}

fn ws_id(conn: &rusqlite::Connection, key: &str) -> String {
    resolve_workspace_id(conn, key)
}

fn default_ws(conn: &rusqlite::Connection) -> String {
    conn.query_row(
        "SELECT id FROM workspace ORDER BY created_at LIMIT 1",
        [],
        |r| r.get::<_, String>(0),
    )
    .unwrap_or_else(|_| "default".to_string())
}

// ── Issue status ──────────────────────────────────────────────────────────

fn status_from_row(r: &Row) -> rusqlite::Result<IssueStatus> {
    Ok(IssueStatus {
        id: r.get(0)?, workspace_id: r.get(1)?, key: r.get(2)?, name: r.get(3)?,
        description: r.get(4)?, category: r.get(5)?, color: r.get(6)?, position: r.get(7)?,
    })
}

#[tauri::command]
pub fn create_issue_status(app: tauri::AppHandle, db: State<Db>, workspace_id: String, key: Option<String>, name: String, description: Option<String>, category: String, color: String, icon: Option<String>) -> Result<IssueStatus, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let _ = icon; // 本地表暂不存 icon，接受参数以匹配前端签名。
    let key = key.unwrap_or_else(|| name.to_lowercase().replace(|c: char| !c.is_alphanumeric(), "-"));
    let description = description.unwrap_or_default();
    let conn = db.0.lock().unwrap();
    let pos: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM issue_status WHERE workspace_id = ?1 AND category = ?2",
        params![ws, category], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let id = new_id();
    conn.execute(
        "INSERT INTO issue_status (id, workspace_id, key, name, description, category, color, position) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, ws, key, name, description.clone(), category, color, pos],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue_status", &id);
    Ok(IssueStatus { id, workspace_id: ws, key, name, description, category, color, position: pos })
}

#[tauri::command]
pub fn update_issue_status_def(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, description: Option<String>, color: Option<String>, icon: Option<String>, position: Option<i64>) -> Result<Option<IssueStatus>, String> {
    let _ = icon;
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    if let Some(v) = name { sets.push("name = ?".into()); vals.push(v.into()); }
    if let Some(v) = description { sets.push("description = ?".into()); vals.push(v.into()); }
    if let Some(v) = color { sets.push("color = ?".into()); vals.push(v.into()); }
    if let Some(v) = position { sets.push("position = ?".into()); vals.push(v.into()); }
    if !sets.is_empty() {
        let sql = format!("UPDATE issue_status SET {} WHERE id = ?{}", sets.join(", "), vals.len() + 1);
        vals.push(id.clone().into());
        conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    }
    let mut stmt = conn.prepare("SELECT id, workspace_id, key, name, description, category, color, position FROM issue_status WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id.clone()], status_from_row).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "issue_status", &id);
    Ok(out)
}

#[tauri::command]
pub fn archive_issue_status(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    // 归档 = 删除状态定义（其 issue 会回落到默认状态；本地版直接删）。
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM issue_status WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue_status", &id);
    Ok(())
}

#[tauri::command]
pub fn reorder_issue_statuses(app: tauri::AppHandle, db: State<Db>, workspace_id: String, category: String, ids: Vec<String>, include_system: Option<bool>) -> Result<(), String> {
    let _ = include_system;
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    for (pos, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE issue_status SET position = ?1 WHERE id = ?2 AND workspace_id = ?3 AND category = ?4",
            params![pos as i64, id, ws, category],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "issue_status", &ws);
    Ok(())
}

// ── Issue view ────────────────────────────────────────────────────────────

fn view_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "workspace_id": r.get::<_, String>(1)?,
        "owner_id": r.get::<_, String>(2)?,
        "name": r.get::<_, String>(3)?,
        "scope_type": r.get::<_, String>(4)?,
        "scope_id": r.get::<_, Option<String>>(5)?,
        "scope_variant": r.get::<_, Option<String>>(6)?,
        "visibility": r.get::<_, String>(7)?,
        "definition_version": r.get::<_, i64>(8)?,
        "query": parse_json_pub(&r.get::<_, String>(9)?, serde_json::json!({})),
        "display": parse_json_pub(&r.get::<_, String>(10)?, serde_json::json!({})),
        "revision": r.get::<_, i64>(11)?,
        "created_at": r.get::<_, String>(12)?,
        "updated_at": r.get::<_, String>(13)?,
    }))
}

const VIEW_COLS: &str = "id, workspace_id, owner_id, name, scope_type, scope_id, scope_variant, visibility, definition_version, query, display, revision, created_at, updated_at";

#[tauri::command]
pub fn create_issue_view(app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String, scope_type: String, scope_id: Option<String>, scope_variant: Option<String>, visibility: String, definition_version: i64, query: serde_json::Value, display: serde_json::Value) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let now = now_iso();
    let id = new_id();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO issue_view (id, workspace_id, owner_id, name, scope_type, scope_id, scope_variant, visibility, definition_version, query, display, revision, created_at, updated_at) VALUES (?1,?2,'u-local',?3,?4,?5,?6,?7,?8,?9,?10,1,?11,?11)",
        params![id, ws, name, scope_type, scope_id, scope_variant, visibility, definition_version, j(query), j(display), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue_view", &id);
    let mut stmt = conn.prepare(&format!("SELECT {} FROM issue_view WHERE id = ?1", VIEW_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], view_json).map_err(|e| e.to_string())?;
    Ok(rows.next().unwrap().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_issue_view(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, visibility: Option<String>, scope_variant: Option<String>, query: Option<serde_json::Value>, display: Option<serde_json::Value>, expected_revision: i64) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let cur: i64 = conn.query_row("SELECT revision FROM issue_view WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or(0);
    if cur != expected_revision {
        return Err(format!("view revision 冲突：当前 {}，期望 {}", cur, expected_revision));
    }
    let mut sets: Vec<String> = vec!["revision = revision + 1".into(), "updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    if let Some(v) = name { sets.push("name = ?".into()); vals.push(v.into()); }
    if let Some(v) = visibility { sets.push("visibility = ?".into()); vals.push(v.into()); }
    if let Some(v) = scope_variant { sets.push("scope_variant = ?".into()); vals.push(v.into()); }
    if let Some(v) = query { sets.push("query = ?".into()); vals.push(j(v).into()); }
    if let Some(v) = display { sets.push("display = ?".into()); vals.push(j(v).into()); }
    let sql = format!("UPDATE issue_view SET {} WHERE id = ?{}", sets.join(", "), vals.len() + 1);
    vals.push(id.clone().into());
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM issue_view WHERE id = ?1", VIEW_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id.clone()], view_json).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "issue_view", &id);
    Ok(out)
}

#[tauri::command]
pub fn delete_issue_view(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM issue_view WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue_view", &id);
    Ok(())
}

#[tauri::command]
pub fn get_issue_view(db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(&format!("SELECT {} FROM issue_view WHERE id = ?1", VIEW_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], view_json).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn put_issue_view_preference(app: tauri::AppHandle, db: State<Db>, scope_type: String, scope_id: Option<String>, prefs: serde_json::Value) -> Result<serde_json::Value, String> {
    let now = now_iso();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO issue_view_preference (scope_type, scope_id, prefs, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(scope_type, scope_id) DO UPDATE SET prefs = excluded.prefs, updated_at = excluded.updated_at",
        params![scope_type, scope_id, j(prefs.clone()), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue_view_preference", &scope_type);
    Ok(serde_json::json!({ "scope_type": scope_type, "scope_id": scope_id, "prefs": prefs, "updated_at": now }))
}

#[tauri::command]
pub fn get_issue_view_preference(db: State<Db>, scope_type: String, scope_id: Option<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(
        "SELECT prefs, updated_at FROM issue_view_preference WHERE scope_type = ?1 AND scope_id IS ?2",
        params![scope_type, scope_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    );
    match row {
        Ok((prefs, updated_at)) => Ok(serde_json::json!({ "scope_type": scope_type, "scope_id": scope_id, "prefs": parse_json_pub(&prefs, serde_json::json!({ "hidden": [], "order": [] })), "updated_at": updated_at })),
        Err(_) => Ok(serde_json::json!({ "scope_type": scope_type, "scope_id": scope_id, "prefs": { "hidden": [], "order": [] }, "updated_at": "" })),
    }
}

// ── Pin ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_pin(app: tauri::AppHandle, db: State<Db>, workspace_id: String, item_type: String, item_id: String) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let now = now_iso();
    let conn = db.0.lock().unwrap();
    // 同一 item 只置顶一次。
    let existing: Option<String> = conn.query_row(
        "SELECT id FROM pin WHERE workspace_id = ?1 AND item_type = ?2 AND item_id = ?3",
        params![ws, item_type, item_id], |r| r.get(0),
    ).ok();
    let id = match existing {
        Some(id) => id,
        None => {
            let pos: i64 = conn.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM pin WHERE workspace_id = ?1", params![ws], |r| r.get(0)).map_err(|e| e.to_string())?;
            let id = new_id();
            conn.execute(
                "INSERT INTO pin (id, workspace_id, item_type, item_id, position, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
                params![id, ws, item_type, item_id, pos, now],
            ).map_err(|e| e.to_string())?;
            id
        }
    };
    emit_changed(&app, "pin", &id);
    Ok(serde_json::json!({ "id": id, "workspace_id": ws, "item_type": item_type, "item_id": item_id, "created_at": now }))
}

#[tauri::command]
pub fn delete_pin(app: tauri::AppHandle, db: State<Db>, item_type: String, item_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM pin WHERE item_type = ?1 AND item_id = ?2", params![item_type, item_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "pin", &item_id);
    Ok(())
}

#[tauri::command]
pub fn reorder_pins(app: tauri::AppHandle, db: State<Db>, workspace_id: String, items: serde_json::Value) -> Result<(), String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    if let Some(arr) = items.get("items").and_then(|v| v.as_array()) {
        for it in arr {
            if let (Some(id), Some(pos)) = (it.get("id").and_then(|v| v.as_str()), it.get("position").and_then(|v| v.as_i64())) {
                conn.execute("UPDATE pin SET position = ?1 WHERE id = ?2 AND workspace_id = ?3", params![pos, id, ws]).map_err(|e| e.to_string())?;
            }
        }
    }
    emit_changed(&app, "pin", &ws);
    Ok(())
}

// ── Property ──────────────────────────────────────────────────────────────

fn property_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "workspace_id": r.get::<_, String>(1)?,
        "name": r.get::<_, String>(2)?,
        "type": r.get::<_, String>(3)?,
        "description": r.get::<_, Option<String>>(4)?,
        "icon": r.get::<_, Option<String>>(5)?,
        "config": parse_json_pub(&r.get::<_, String>(6)?, serde_json::json!({})),
        "archived": r.get::<_, i64>(7)? != 0,
        "created_at": r.get::<_, String>(8)?,
    }))
}

#[tauri::command]
pub fn create_property(app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String, prop_type: String, description: Option<String>, icon: Option<String>, config: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let now = now_iso();
    let id = new_id();
    let conn = db.0.lock().unwrap();
    // property_def 暂无 description/icon 列：并入 config JSON（读时还原）。
    let mut cfg = config.unwrap_or(serde_json::json!({}));
    if let Some(d) = &description { cfg["description"] = serde_json::json!(d); }
    if let Some(i) = &icon { cfg["icon"] = serde_json::json!(i); }
    conn.execute(
        "INSERT INTO property_def (id, workspace_id, name, type, config, archived, created_at) VALUES (?1,?2,?3,?4,?5,0,?6)",
        params![id, ws, name, prop_type, j(cfg.clone()), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "property", &id);
    Ok(serde_json::json!({ "id": id, "workspace_id": ws, "name": name, "type": prop_type, "description": description, "icon": icon, "config": cfg, "archived": false, "created_at": now }))
}

#[tauri::command]
pub fn update_property(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, description: Option<String>, icon: Option<String>, config: Option<serde_json::Value>, archived: Option<bool>) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec![];
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    if let Some(v) = name { sets.push("name = ?".into()); vals.push(v.into()); }
    if let Some(v) = archived { sets.push("archived = ?".into()); vals.push((if v { 1i64 } else { 0i64 }).into()); }
    if let Some(v) = config { sets.push("config = ?".into()); vals.push(j(v).into()); }
    if let Some(v) = description {
        // description 存进现有 config（读时还原）。
        let cur: String = conn.query_row("SELECT config FROM property_def WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or_else(|_| "{}".into());
        let mut cfg = parse_json_pub(&cur, serde_json::json!({}));
        cfg["description"] = serde_json::json!(v);
        sets.push("config = ?".into()); vals.push(j(cfg).into());
    }
    if let Some(v) = icon {
        let cur: String = conn.query_row("SELECT config FROM property_def WHERE id = ?1", params![id], |r| r.get(0)).unwrap_or_else(|_| "{}".into());
        let mut cfg = parse_json_pub(&cur, serde_json::json!({}));
        cfg["icon"] = serde_json::json!(v);
        sets.push("config = ?".into()); vals.push(j(cfg).into());
    }
    if !sets.is_empty() {
        let sql = format!("UPDATE property_def SET {} WHERE id = ?{}", sets.join(", "), vals.len() + 1);
        vals.push(id.clone().into());
        conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    }
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, type, config, archived, created_at FROM property_def WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id.clone()], |r| {
        let cfg = parse_json_pub(&r.get::<_, String>(4)?, serde_json::json!({}));
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?,
            "name": r.get::<_, String>(2)?, "type": r.get::<_, String>(3)?,
            "description": cfg.get("description").cloned(), "icon": cfg.get("icon").cloned(),
            "config": cfg, "archived": r.get::<_, i64>(5)? != 0, "created_at": r.get::<_, String>(6)?,
        }))
    }).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "property", &id);
    Ok(out)
}

#[tauri::command]
pub fn set_issue_property(app: tauri::AppHandle, db: State<Db>, issue_id: String, property_id: String, value: serde_json::Value) -> Result<serde_json::Value, String> {
    let now = now_iso();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO issue_property_value (issue_id, property_id, value, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(issue_id, property_id) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![issue_id, property_id, j(value.clone()), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(serde_json::json!({ "issue_id": issue_id, "property_id": property_id, "value": value, "updated_at": now }))
}

#[tauri::command]
pub fn unset_issue_property(app: tauri::AppHandle, db: State<Db>, issue_id: String, property_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM issue_property_value WHERE issue_id = ?1 AND property_id = ?2", params![issue_id, property_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(())
}

// ── Label 关联 ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn attach_label(app: tauri::AppHandle, db: State<Db>, issue_id: String, label_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT OR IGNORE INTO issue_label (issue_id, label_id) VALUES (?1,?2)", params![issue_id, label_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(serde_json::json!({ "issue_id": issue_id, "label_id": label_id }))
}

#[tauri::command]
pub fn detach_label(app: tauri::AppHandle, db: State<Db>, issue_id: String, label_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM issue_label WHERE issue_id = ?1 AND label_id = ?2", params![issue_id, label_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(())
}

#[tauri::command]
pub fn attach_label_to_resource(app: tauri::AppHandle, db: State<Db>, resource_type: String, resource_id: String, label_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT OR IGNORE INTO resource_label (resource_type, resource_id, label_id) VALUES (?1,?2,?3)", params![resource_type, resource_id, label_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, &resource_type, &resource_id);
    Ok(serde_json::json!({ "resource_type": resource_type, "resource_id": resource_id, "label_id": label_id }))
}

#[tauri::command]
pub fn detach_label_from_resource(app: tauri::AppHandle, db: State<Db>, resource_type: String, resource_id: String, label_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM resource_label WHERE resource_type = ?1 AND resource_id = ?2 AND label_id = ?3", params![resource_type, resource_id, label_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, &resource_type, &resource_id);
    Ok(())
}

#[tauri::command]
pub fn list_labels_for_resource(db: State<Db>, resource_type: String, resource_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT l.id, l.workspace_id, l.name, l.description, l.color, l.created_at, l.updated_at FROM label l JOIN resource_label rl ON rl.label_id = l.id WHERE rl.resource_type = ?1 AND rl.resource_id = ?2 ORDER BY l.name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![resource_type, resource_id], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?, "name": r.get::<_, String>(2)?,
        "description": r.get::<_, Option<String>>(3)?, "color": r.get::<_, String>(4)?,
        "created_at": r.get::<_, String>(5)?, "updated_at": r.get::<_, String>(6)?,
    }))).map_err(|e| e.to_string())?;
    let labels = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "labels": labels }))
}

// ── Project resource ──────────────────────────────────────────────────────

fn resource_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "project_id": r.get::<_, String>(1)?,
        "kind": r.get::<_, String>(2)?,
        "title": r.get::<_, String>(3)?,
        "url": r.get::<_, String>(4)?,
        "created_at": r.get::<_, String>(5)?,
    }))
}

#[tauri::command]
pub fn create_project_resource(app: tauri::AppHandle, db: State<Db>, project_id: String, resource_type: String, resource_ref: serde_json::Value, label: Option<String>, position: Option<i64>) -> Result<serde_json::Value, String> {
    let _ = position;
    // resource_ref 形状：{ url } | { path } | { repo } 等；归一化为 title/url。
    let url = resource_ref.get("url").and_then(|v| v.as_str())
        .or_else(|| resource_ref.get("path").and_then(|v| v.as_str()))
        .or_else(|| resource_ref.get("repo").and_then(|v| v.as_str()))
        .unwrap_or("").to_string();
    let title = label.unwrap_or_else(|| url.clone());
    let now = now_iso();
    let id = new_id();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO project_resource (id, project_id, kind, title, url, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![id, project_id, resource_type, title, url, now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "project", &project_id);
    Ok(serde_json::json!({ "id": id, "project_id": project_id, "kind": resource_type, "title": title, "url": url, "created_at": now }))
}

#[tauri::command]
pub fn update_project_resource(app: tauri::AppHandle, db: State<Db>, project_id: String, resource_id: String, resource_ref: Option<serde_json::Value>, label: Option<String>, position: Option<i64>) -> Result<Option<serde_json::Value>, String> {
    let _ = position;
    let conn = db.0.lock().unwrap();
    if let Some(rr) = &resource_ref {
        let url = rr.get("url").and_then(|v| v.as_str())
            .or_else(|| rr.get("path").and_then(|v| v.as_str()))
            .or_else(|| rr.get("repo").and_then(|v| v.as_str()))
            .unwrap_or("").to_string();
        conn.execute("UPDATE project_resource SET url = ?1 WHERE id = ?2 AND project_id = ?3", params![url, resource_id, project_id]).map_err(|e| e.to_string())?;
    }
    if let Some(t) = label {
        conn.execute("UPDATE project_resource SET title = ?1 WHERE id = ?2 AND project_id = ?3", params![t, resource_id, project_id]).map_err(|e| e.to_string())?;
    }
    let mut stmt = conn.prepare("SELECT id, project_id, kind, title, url, created_at FROM project_resource WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([resource_id.clone()], resource_json).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "project", &project_id);
    Ok(out)
}

#[tauri::command]
pub fn delete_project_resource(app: tauri::AppHandle, db: State<Db>, project_id: String, resource_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM project_resource WHERE id = ?1 AND project_id = ?2", params![resource_id, project_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "project", &project_id);
    Ok(())
}

// ── Squad 成员角色 ────────────────────────────────────────────────────────

#[tauri::command]
pub fn update_squad_member_role(app: tauri::AppHandle, db: State<Db>, squad_id: String, member_type: String, member_id: String, role: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "UPDATE squad_member SET role = ?1 WHERE squad_id = ?2 AND member_type = ?3 AND member_id = ?4",
        params![role, squad_id, member_type, member_id],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &squad_id);
    Ok(serde_json::json!({ "squad_id": squad_id, "member_type": member_type, "member_id": member_id, "role": role }))
}

// ── Inbox ─────────────────────────────────────────────────────────────────

fn inbox_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "workspace_id": r.get::<_, String>(1)?,
        "recipient_type": r.get::<_, String>(2)?,
        "recipient_id": r.get::<_, String>(3)?,
        "actor_type": r.get::<_, Option<String>>(4)?,
        "actor_id": r.get::<_, Option<String>>(5)?,
        "type": r.get::<_, String>(6)?,
        "severity": r.get::<_, String>(7)?,
        "issue_id": r.get::<_, Option<String>>(8)?,
        "title": r.get::<_, String>(9)?,
        "body": r.get::<_, Option<String>>(10)?,
        "issue_status": r.get::<_, Option<String>>(11)?,
        "created_at": r.get::<_, String>(12)?,
        "read_at": r.get::<_, Option<String>>(13)?,
        "archived_at": r.get::<_, Option<String>>(14)?,
    }))
}

const INBOX_COLS: &str = "id, workspace_id, recipient_type, recipient_id, actor_type, actor_id, type, severity, issue_id, title, body, issue_status, created_at, read_at, archived_at";

fn inbox_one(conn: &rusqlite::Connection, id: &str) -> Result<Option<serde_json::Value>, String> {
    let mut stmt = conn.prepare(&format!("SELECT {} FROM inbox_item WHERE id = ?1", INBOX_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], inbox_json).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn mark_inbox_read(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE inbox_item SET read_at = COALESCE(read_at, ?1) WHERE id = ?2", params![now_iso(), id]).map_err(|e| e.to_string())?;
    let out = inbox_one(&conn, &id)?;
    emit_changed(&app, "inbox", &id);
    Ok(out)
}

#[tauri::command]
pub fn mark_inbox_unread(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE inbox_item SET read_at = NULL WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    let out = inbox_one(&conn, &id)?;
    emit_changed(&app, "inbox", &id);
    Ok(out)
}

#[tauri::command]
pub fn archive_inbox(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE inbox_item SET archived_at = COALESCE(archived_at, ?1) WHERE id = ?2", params![now_iso(), id]).map_err(|e| e.to_string())?;
    let out = inbox_one(&conn, &id)?;
    emit_changed(&app, "inbox", &id);
    Ok(out)
}

#[tauri::command]
pub fn unarchive_inbox(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE inbox_item SET archived_at = NULL WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    let out = inbox_one(&conn, &id)?;
    emit_changed(&app, "inbox", &id);
    Ok(out)
}

#[tauri::command]
pub fn mark_all_inbox_read(app: tauri::AppHandle, db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let n = conn.execute("UPDATE inbox_item SET read_at = COALESCE(read_at, ?1) WHERE read_at IS NULL", params![now_iso()]).map_err(|e| e.to_string())?;
    emit_changed(&app, "inbox", "all");
    Ok(serde_json::json!({ "marked": n }))
}

#[tauri::command]
pub fn archive_all_inbox(app: tauri::AppHandle, db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let n = conn.execute("UPDATE inbox_item SET archived_at = COALESCE(archived_at, ?1) WHERE archived_at IS NULL", params![now_iso()]).map_err(|e| e.to_string())?;
    emit_changed(&app, "inbox", "all");
    Ok(serde_json::json!({ "archived": n }))
}

#[tauri::command]
pub fn archive_all_read_inbox(app: tauri::AppHandle, db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let n = conn.execute("UPDATE inbox_item SET archived_at = COALESCE(archived_at, ?1) WHERE archived_at IS NULL AND read_at IS NOT NULL", params![now_iso()]).map_err(|e| e.to_string())?;
    emit_changed(&app, "inbox", "all");
    Ok(serde_json::json!({ "archived": n }))
}

#[tauri::command]
pub fn archive_completed_inbox(app: tauri::AppHandle, db: State<Db>) -> Result<serde_json::Value, String> {
    // 已完成的通知：关联 issue 已 done/cancelled 的归档。
    let conn = db.0.lock().unwrap();
    let n = conn.execute(
        "UPDATE inbox_item SET archived_at = COALESCE(archived_at, ?1) WHERE archived_at IS NULL AND issue_id IN (SELECT id FROM issue WHERE status IN ('done','cancelled'))",
        params![now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "inbox", "all");
    Ok(serde_json::json!({ "archived": n }))
}

#[tauri::command]
pub fn list_archived_inbox(db: State<Db>, workspace_id: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let resolved = workspace_id.as_deref().map(|w| ws_id(&conn, w));
    let mut stmt = match &resolved {
        Some(ws) => conn.prepare(&format!("SELECT {} FROM inbox_item WHERE workspace_id = ?1 AND archived_at IS NOT NULL ORDER BY archived_at DESC", INBOX_COLS)).map_err(|e| e.to_string())?,
        None => conn.prepare(&format!("SELECT {} FROM inbox_item WHERE archived_at IS NOT NULL ORDER BY archived_at DESC", INBOX_COLS)).map_err(|e| e.to_string())?,
    };
    let rows = match &resolved {
        Some(ws) => stmt.query_map([ws.clone()], inbox_json).map_err(|e| e.to_string())?,
        None => stmt.query_map([], inbox_json).map_err(|e| e.to_string())?,
    };
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

// ── Chat ──────────────────────────────────────────────────────────────────

fn chat_session_json(conn: &rusqlite::Connection, id: &str) -> Result<Option<serde_json::Value>, String> {
    let row = conn.query_row(
        "SELECT id, workspace_id, agent_id, creator_id, project_id, title, status, has_unread, pinned, archived, kind, runtime_id, model, created_at, updated_at FROM chat_session WHERE id = ?1",
        params![id],
        |r| Ok((
            r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?,
            r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, String>(5)?,
            r.get::<_, String>(6)?, r.get::<_, i64>(7)?, r.get::<_, i64>(8)?,
            r.get::<_, i64>(9)?, r.get::<_, String>(10)?, r.get::<_, Option<String>>(11)?,
            r.get::<_, Option<String>>(12)?, r.get::<_, String>(13)?, r.get::<_, String>(14)?,
        )),
    );
    let (id, ws, agent_id, creator_id, project_id, title, status, has_unread, pinned, archived, kind, runtime_id, model, created_at, updated_at) = match row {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };
    let last = conn.query_row(
        "SELECT content, role, created_at FROM chat_message WHERE chat_session_id = ?1 ORDER BY created_at DESC LIMIT 1",
        params![id],
        |r| Ok(serde_json::json!({ "content": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)?, "created_at": r.get::<_, String>(2)? })),
    ).ok();
    Ok(Some(serde_json::json!({
        "id": id, "workspace_id": ws, "agent_id": agent_id, "creator_id": creator_id,
        "project_id": project_id, "title": title, "status": status,
        "has_unread": has_unread != 0, "pinned": pinned != 0, "archived": archived != 0,
        "kind": kind, "runtime_id": runtime_id, "model": model,
        "last_message": last, "created_at": created_at, "updated_at": updated_at,
    })))
}

fn chat_message_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    let atts: String = r.get(5)?;
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "chat_session_id": r.get::<_, String>(1)?,
        "role": r.get::<_, String>(2)?,
        "content": r.get::<_, String>(3)?,
        "task_id": r.get::<_, Option<String>>(4)?,
        "attachments": parse_json_pub(&atts, serde_json::json!([])),
        "created_at": r.get::<_, String>(6)?,
    }))
}

const CHAT_MSG_COLS: &str = "id, chat_session_id, role, content, task_id, attachments, created_at";

#[tauri::command]
pub fn create_chat_session(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, agent_id: String, title: Option<String>, project_id: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) } };
    let now = now_iso();
    let id = new_id();
    let title = title.unwrap_or_else(|| "新会话".to_string());
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO chat_session (id, workspace_id, agent_id, creator_id, project_id, title, status, has_unread, pinned, archived, kind, created_at, updated_at) VALUES (?1,?2,?3,'u-local',?4,?5,'active',0,0,0,'chat',?6,?6)",
        params![id, ws, agent_id, project_id, title, now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &id);
    Ok(chat_session_json(&conn, &id)?.unwrap_or(serde_json::json!({ "id": id })))
}

#[tauri::command]
pub fn get_chat_session(db: State<Db>, id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    chat_session_json(&conn, &id)
}

#[tauri::command]
pub fn update_chat_session(app: tauri::AppHandle, db: State<Db>, id: String, title: Option<String>, project_id: Option<String>) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    if let Some(t) = title {
        conn.execute("UPDATE chat_session SET title = ?1, updated_at = ?2 WHERE id = ?3", params![t, now_iso(), id]).map_err(|e| e.to_string())?;
    }
    if let Some(p) = project_id {
        conn.execute("UPDATE chat_session SET project_id = ?1, updated_at = ?2 WHERE id = ?3", params![p, now_iso(), id]).map_err(|e| e.to_string())?;
    }
    let out = chat_session_json(&conn, &id)?;
    emit_changed(&app, "chat", &id);
    Ok(out)
}

#[tauri::command]
pub fn delete_chat_session(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM chat_message WHERE chat_session_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM chat_session WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &id);
    Ok(())
}

#[tauri::command]
pub fn set_chat_session_archived(app: tauri::AppHandle, db: State<Db>, id: String, archived: bool) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "UPDATE chat_session SET archived = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
        params![if archived { 1i64 } else { 0i64 }, if archived { "archived" } else { "active" }, now_iso(), id],
    ).map_err(|e| e.to_string())?;
    let out = chat_session_json(&conn, &id)?;
    emit_changed(&app, "chat", &id);
    Ok(out)
}

#[tauri::command]
pub fn set_chat_session_pinned(app: tauri::AppHandle, db: State<Db>, id: String, pinned: bool) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE chat_session SET pinned = ?1, updated_at = ?2 WHERE id = ?3", params![if pinned { 1i64 } else { 0i64 }, now_iso(), id]).map_err(|e| e.to_string())?;
    let out = chat_session_json(&conn, &id)?;
    emit_changed(&app, "chat", &id);
    Ok(out)
}

#[tauri::command]
pub fn mark_chat_session_read(app: tauri::AppHandle, db: State<Db>, session_id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE chat_session SET has_unread = 0, updated_at = ?1 WHERE id = ?2", params![now_iso(), session_id]).map_err(|e| e.to_string())?;
    let out = chat_session_json(&conn, &session_id)?;
    emit_changed(&app, "chat", &session_id);
    Ok(out)
}

/// chat 发送：落 user 消息 → 起 CLI run（agent 回复）→ run 结束自动追加 assistant 消息。
#[tauri::command]
pub fn send_chat_message(app: tauri::AppHandle, db: State<Db>, session_id: String, content: String, attachment_ids: Option<Vec<String>>) -> Result<serde_json::Value, String> {
    let now = now_iso();
    let msg_id = new_id();
    let (agent_id, agent_name, cli, kind) = {
        let conn = db.0.lock().unwrap();
        let s = conn.query_row(
            "SELECT agent_id, kind FROM chat_session WHERE id = ?1",
            params![session_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).map_err(|_| format!("chat session {} 不存在", session_id))?;
        let a = conn.query_row(
            "SELECT name, cli FROM agent WHERE id = ?1",
            params![s.0], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).unwrap_or_else(|_| ("Claude".to_string(), "claude".to_string()));
        (s.0, a.0, a.1, s.1)
    };
    // 历史（最近 10 条）拼进 prompt，给 CLI 上下文。
    let history = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT role, content FROM chat_message WHERE chat_session_id = ?1 ORDER BY created_at DESC LIMIT 10").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([session_id.clone()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        let mut v: Vec<(String, String)> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        v.reverse();
        v
    };
    let mut prompt = String::new();
    if kind == "builder" {
        prompt.push_str("你正在帮用户创建一个 Multica Agent。用户会用自然语言描述想要的 Agent，你要问清关键信息（名字、职责、用什么 CLI、指令），并在信息齐全时输出一份 JSON 配置（name/description/instructions/cli/model）。不要编造用户没说的细节。\n\n");
    }
    for (role, c) in &history {
        prompt.push_str(&format!("{}: {}\n", role, c));
    }
    prompt.push_str(&format!("user: {}\n请直接回答用户最新消息。用简洁中文。", content));
    let atts = attachment_ids.clone().unwrap_or_default();
    {
        let conn = db.0.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_message (id, chat_session_id, role, content, task_id, attachments, created_at) VALUES (?1,?2,'user',?3,NULL,?4,?5)",
            params![msg_id, session_id, content, j(serde_json::json!(atts)), now],
        ).map_err(|e| e.to_string())?;
    }
    // 起 run：issue 为空 → chat 专属工作目录；run 结束自动追加 assistant 消息。
    let run = super::spawn_run_with_chat(&app, &db, "", &agent_id, &agent_name, &cli, &prompt, Some(&session_id))?;
    {
        let conn = db.0.lock().unwrap();
        conn.execute("UPDATE chat_message SET task_id = ?1 WHERE id = ?2", params![run.id, msg_id]).map_err(|e| e.to_string())?;
        conn.execute("UPDATE chat_session SET updated_at = ?1 WHERE id = ?2", params![now, session_id]).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "chat", &session_id);
    Ok(serde_json::json!({
        "message_id": msg_id, "task_id": run.id, "supports_queue": true, "queued": false,
        "created_at": now, "attachment_ids": atts,
    }))
}

#[tauri::command]
pub fn list_chat_messages_page(db: State<Db>, session_id: String, before: Option<serde_json::Value>, limit: Option<i64>) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let conn = db.0.lock().unwrap();
    let (rows, has_more, next_cursor) = if let Some(b) = before {
        let created_at = b.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let mut stmt = conn.prepare(&format!("SELECT {} FROM chat_message WHERE chat_session_id = ?1 AND (created_at < ?2 OR (created_at = ?2 AND id < ?3)) ORDER BY created_at DESC, id DESC LIMIT ?4", CHAT_MSG_COLS)).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![session_id, created_at, id, limit + 1], chat_message_json).map_err(|e| e.to_string())?;
        let mut v: Vec<serde_json::Value> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        let has_more = (v.len() as i64) > limit;
        if has_more { v.pop(); }
        let next = v.last().map(|m| serde_json::json!({ "created_at": m["created_at"], "id": m["id"] }));
        v.reverse();
        (v, has_more, next)
    } else {
        let mut stmt = conn.prepare(&format!("SELECT {} FROM chat_message WHERE chat_session_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2", CHAT_MSG_COLS)).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![session_id, limit + 1], chat_message_json).map_err(|e| e.to_string())?;
        let mut v: Vec<serde_json::Value> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        let has_more = (v.len() as i64) > limit;
        if has_more { v.pop(); }
        let next = v.last().map(|m| serde_json::json!({ "created_at": m["created_at"], "id": m["id"] }));
        v.reverse();
        (v, has_more, next)
    };
    Ok(serde_json::json!({ "messages": rows, "limit": limit, "has_more": has_more, "next_cursor": next_cursor }))
}

#[tauri::command]
pub fn list_task_messages(db: State<Db>, task_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(&format!("SELECT {} FROM chat_message WHERE task_id = ?1 ORDER BY created_at", CHAT_MSG_COLS)).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([task_id], chat_message_json).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_queued_chat_tasks(app: tauri::AppHandle, db: State<Db>, session_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM run WHERE chat_session_id = ?1 AND status = 'running'").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([session_id.clone()], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    drop(conn);
    let mut cancelled = 0;
    for id in ids {
        if super::cancel_run_by_id(&app, &db, &id).is_ok() { cancelled += 1; }
    }
    emit_changed(&app, "chat", &session_id);
    Ok(serde_json::json!({ "cancelled": cancelled }))
}

#[tauri::command]
pub fn cancel_task_by_id(app: tauri::AppHandle, db: State<Db>, task_id: String, queued_action: Option<String>, session_id: Option<String>) -> Result<serde_json::Value, String> {
    // queuedAction=edit：被取消的 user 消息恢复到输入框；remove：删除该消息。
    let msg = {
        let conn = db.0.lock().unwrap();
        conn.query_row(
            "SELECT id, chat_session_id, content, attachments FROM chat_message WHERE task_id = ?1 AND role = 'user' ORDER BY created_at DESC LIMIT 1",
            params![task_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        ).ok()
    };
    super::cancel_run_by_id(&app, &db, &task_id)?;
    if let Some((mid, sid, content, atts)) = msg {
        if queued_action.as_deref() == Some("remove") {
            let conn = db.0.lock().unwrap();
            conn.execute("DELETE FROM chat_message WHERE id = ?1", params![mid]).map_err(|e| e.to_string())?;
            emit_changed(&app, "chat", &sid);
            return Ok(serde_json::json!({ "task_id": task_id, "cancelled": true }));
        }
        let restore = queued_action.as_deref() == Some("edit");
        emit_changed(&app, "chat", &sid);
        return Ok(serde_json::json!({
            "task_id": task_id, "cancelled": true,
            "cancelled_chat_message": {
                "chat_session_id": session_id.unwrap_or(sid), "message_id": mid, "content": content,
                "restore_to_input": restore,
                "attachments": parse_json_pub(&atts, serde_json::json!([])),
            },
        }));
    }
    Ok(serde_json::json!({ "task_id": task_id, "cancelled": true }))
}

#[tauri::command]
pub fn pin_chat_agent(app: tauri::AppHandle, db: State<Db>, agent_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT OR IGNORE INTO pinned_chat_agent (agent_id, created_at) VALUES (?1,?2)", params![agent_id, now_iso()]).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "pinned": true }))
}

#[tauri::command]
pub fn unpin_chat_agent(app: tauri::AppHandle, db: State<Db>, agent_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM pinned_chat_agent WHERE agent_id = ?1", params![agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "pinned": false }))
}

#[tauri::command]
pub fn list_chat_pinned_agents(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT agent_id, created_at FROM pinned_chat_agent ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({ "agent_id": r.get::<_, String>(0)?, "created_at": r.get::<_, String>(1)? }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn regenerate_chat_quick_actions(_db: State<Db>, session_id: String, message_id: String) -> Result<serde_json::Value, String> {
    // 快捷操作建议由 CLI 运行时生成；本地版暂无建议模型，返回空（UI 渲染无建议，不报错）。
    Ok(serde_json::json!({ "session_id": session_id, "message_id": message_id, "quick_actions": [] }))
}

#[tauri::command]
pub fn consume_chat_draft_restore(_db: State<Db>, session_id: String, restore_id: String) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "session_id": session_id, "restore_id": restore_id, "consumed": true }))
}

// chat 挂起任务：取该会话 running 的 run。
#[tauri::command]
pub fn get_pending_chat_task(db: State<Db>, session_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(
        "SELECT id, status, started_at FROM run WHERE chat_session_id = ?1 AND status = 'running' ORDER BY started_at DESC LIMIT 1",
        params![session_id],
        |r| Ok(serde_json::json!({ "task_id": r.get::<_, String>(0)?, "session_id": session_id, "status": r.get::<_, String>(1)?, "started_at": r.get::<_, String>(2)? })),
    );
    match row { Ok(v) => Ok(v), Err(_) => Ok(serde_json::json!({})) }
}

#[tauri::command]
pub fn list_pending_chat_tasks(db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, chat_session_id, status, started_at FROM run WHERE chat_session_id IS NOT NULL AND status = 'running' ORDER BY started_at DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "task_id": r.get::<_, String>(0)?, "session_id": r.get::<_, String>(1)?,
        "status": r.get::<_, String>(2)?, "started_at": r.get::<_, String>(3)?,
    }))).map_err(|e| e.to_string())?;
    let tasks = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "tasks": tasks }))
}

#[tauri::command]
pub fn has_any_pending_chat_tasks(db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM run WHERE chat_session_id IS NOT NULL AND status = 'running'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "has_pending": n > 0 }))
}

#[tauri::command]
pub fn list_chat_draft_restores(_db: State<Db>, session_id: String) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "session_id": session_id, "restores": [] }))
}

// ── Agent Builder 会话 ────────────────────────────────────────────────────

#[tauri::command]
pub fn create_agent_builder_session(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, runtime_id: String, model: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) } };
    let now = now_iso();
    let id = new_id();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO chat_session (id, workspace_id, agent_id, creator_id, title, status, has_unread, pinned, archived, kind, runtime_id, model, created_at, updated_at) VALUES (?1,?2,'','u-local','创建 Agent','active',0,0,0,'builder',?3,?4,?5,?5)",
        params![id, ws, runtime_id, model, now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &id);
    Ok(chat_session_json(&conn, &id)?.unwrap_or(serde_json::json!({ "id": id })))
}

#[tauri::command]
pub fn list_agent_builder_sessions(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM chat_session WHERE kind = 'builder' ORDER BY updated_at DESC").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    let mut out = vec![];
    for id in ids {
        if let Some(v) = chat_session_json(&conn, &id)? { out.push(v); }
    }
    Ok(out)
}

#[tauri::command]
pub fn switch_agent_builder_runtime(app: tauri::AppHandle, db: State<Db>, session_id: String, runtime_id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE chat_session SET runtime_id = ?1, updated_at = ?2 WHERE id = ?3 AND kind = 'builder'", params![runtime_id, now_iso(), session_id]).map_err(|e| e.to_string())?;
    let out = chat_session_json(&conn, &session_id)?;
    emit_changed(&app, "chat", &session_id);
    Ok(out)
}

#[tauri::command]
pub fn save_agent_builder_draft(app: tauri::AppHandle, db: State<Db>, session_id: String, draft: serde_json::Value) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO builder_draft (session_id, draft, updated_at) VALUES (?1,?2,?3) ON CONFLICT(session_id) DO UPDATE SET draft = excluded.draft, updated_at = excluded.updated_at",
        params![session_id, j(draft), now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "chat", &session_id);
    Ok(())
}

// ── Comment：resolve / reaction / subscribe / 子 issue ────────────────────

#[tauri::command]
pub fn resolve_comment(app: tauri::AppHandle, db: State<Db>, comment_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE comment SET resolved_at = ?1, resolved_by_type = 'member', resolved_by_id = 'u-local' WHERE id = ?2", params![now_iso(), comment_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &comment_id);
    Ok(serde_json::json!({ "id": comment_id, "resolved": true }))
}

#[tauri::command]
pub fn unresolve_comment(app: tauri::AppHandle, db: State<Db>, comment_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE comment SET resolved_at = NULL, resolved_by_type = NULL, resolved_by_id = NULL WHERE id = ?1", params![comment_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &comment_id);
    Ok(serde_json::json!({ "id": comment_id, "resolved": false }))
}

#[tauri::command]
pub fn add_reaction(app: tauri::AppHandle, db: State<Db>, comment_id: String, emoji: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO reaction (id, target_type, target_id, emoji, user_id, created_at) VALUES (?1,'comment',?2,?3,'u-local',?4)",
        params![new_id(), comment_id, emoji, now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &comment_id);
    Ok(serde_json::json!({ "comment_id": comment_id, "emoji": emoji }))
}

#[tauri::command]
pub fn remove_reaction(app: tauri::AppHandle, db: State<Db>, comment_id: String, emoji: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM reaction WHERE target_type = 'comment' AND target_id = ?1 AND emoji = ?2 AND user_id = 'u-local'", params![comment_id, emoji]).map_err(|e| e.to_string())?;
    emit_changed(&app, "comment", &comment_id);
    Ok(())
}

#[tauri::command]
pub fn add_issue_reaction(app: tauri::AppHandle, db: State<Db>, issue_id: String, emoji: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO reaction (id, target_type, target_id, emoji, user_id, created_at) VALUES (?1,'issue',?2,?3,'u-local',?4)",
        params![new_id(), issue_id, emoji, now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(serde_json::json!({ "issue_id": issue_id, "emoji": emoji }))
}

#[tauri::command]
pub fn remove_issue_reaction(app: tauri::AppHandle, db: State<Db>, issue_id: String, emoji: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM reaction WHERE target_type = 'issue' AND target_id = ?1 AND emoji = ?2 AND user_id = 'u-local'", params![issue_id, emoji]).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(())
}

#[tauri::command]
pub fn subscribe_to_issue(app: tauri::AppHandle, db: State<Db>, issue_id: String, user_id: Option<String>, user_type: Option<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO issue_subscriber (issue_id, user_id, user_type, created_at) VALUES (?1,?2,?3,?4)",
        params![issue_id, user_id.unwrap_or_else(|| "u-local".into()), user_type.unwrap_or_else(|| "member".into()), now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(serde_json::json!({ "issue_id": issue_id, "subscribed": true }))
}

#[tauri::command]
pub fn unsubscribe_from_issue(app: tauri::AppHandle, db: State<Db>, issue_id: String, user_id: Option<String>, user_type: Option<String>) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute(
        "DELETE FROM issue_subscriber WHERE issue_id = ?1 AND user_id = ?2 AND user_type = ?3",
        params![issue_id, user_id.unwrap_or_else(|| "u-local".into()), user_type.unwrap_or_else(|| "member".into())],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &issue_id);
    Ok(())
}

#[tauri::command]
pub fn list_issue_subscribers(db: State<Db>, issue_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT issue_id, user_id, user_type, created_at FROM issue_subscriber WHERE issue_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], |r| Ok(serde_json::json!({
        "issue_id": r.get::<_, String>(0)?, "user_id": r.get::<_, String>(1)?,
        "user_type": r.get::<_, String>(2)?, "created_at": r.get::<_, String>(3)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_comment_sub_issue_preview(db: State<Db>, anchor_comment_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let (content, issue_id) = conn.query_row(
        "SELECT content, issue_id FROM comment WHERE id = ?1",
        params![anchor_comment_id],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ).map_err(|_| format!("comment {} 不存在", anchor_comment_id))?;
    let title: String = content.lines().next().unwrap_or("").chars().take(80).collect();
    Ok(serde_json::json!({
        "anchor_comment_id": anchor_comment_id, "issue_id": issue_id,
        "title": if title.is_empty() { "新子 issue".to_string() } else { title },
        "body": content,
    }))
}

#[tauri::command]
pub fn create_comment_sub_issue(app: tauri::AppHandle, db: State<Db>, anchor_comment_id: String, issue: serde_json::Value) -> Result<serde_json::Value, String> {
    let now = now_iso();
    let (ws, parent_id) = {
        let conn = db.0.lock().unwrap();
        let row = conn.query_row(
            "SELECT i.workspace_id, c.issue_id FROM comment c JOIN issue i ON i.id = c.issue_id WHERE c.id = ?1",
            params![anchor_comment_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).map_err(|_| format!("comment {} 不存在", anchor_comment_id))?;
        row
    };
    let title = issue.get("title").and_then(|v| v.as_str()).unwrap_or("新子 issue").to_string();
    let body = issue.get("body").and_then(|v| v.as_str()).or_else(|| issue.get("description").and_then(|v| v.as_str())).unwrap_or("").to_string();
    let status = issue.get("status").and_then(|v| v.as_str()).unwrap_or("backlog").to_string();
    let priority = issue.get("priority").and_then(|v| v.as_str()).unwrap_or("medium").to_string();
    let conn = db.0.lock().unwrap();
    let n: i64 = conn.query_row("SELECT COALESCE(MAX(number), 0) + 1 FROM issue WHERE workspace_id = ?1", params![ws], |r| r.get(0)).map_err(|e| e.to_string())?;
    let id = new_id();
    conn.execute(
        "INSERT INTO issue (id, workspace_id, number, identifier, title, body, status, status_category, priority, parent_issue_id, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,'unstarted',?8,?9,?10,?10)",
        params![id, ws, n, format!("MUL-{}", n), title, body, status, priority, parent_id, now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", &id);
    Ok(serde_json::json!({ "id": id, "workspace_id": ws, "identifier": format!("MUL-{}", n), "title": title, "parent_issue_id": parent_id }))
}

// ── Quick action ──────────────────────────────────────────────────────────

fn quick_action_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "workspace_id": r.get::<_, String>(1)?,
        "name": r.get::<_, String>(2)?,
        "description": r.get::<_, String>(3)?,
        "assignee_type": r.get::<_, String>(4)?,
        "assignee_id": r.get::<_, String>(5)?,
        "prompt": r.get::<_, String>(6)?,
        "visibility": r.get::<_, String>(7)?,
        "status": r.get::<_, String>(8)?,
        "last_used_at": r.get::<_, Option<String>>(9)?,
        "use_count": r.get::<_, i64>(10)?,
        "created_by_id": r.get::<_, String>(11)?,
        "created_at": r.get::<_, String>(12)?,
        "updated_at": r.get::<_, String>(13)?,
    }))
}

const QA_COLS: &str = "id, workspace_id, name, description, assignee_type, assignee_id, prompt, visibility, status, last_used_at, use_count, created_by_id, created_at, updated_at";

#[tauri::command]
pub fn list_quick_actions(db: State<Db>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(&format!("SELECT {} FROM quick_action WHERE status = 'active' ORDER BY created_at", QA_COLS)).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], quick_action_json).map_err(|e| e.to_string())?;
    let actions = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "quick_actions": actions }))
}

#[tauri::command]
pub fn create_quick_action(app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String, description: Option<String>, assignee_type: String, assignee_id: String, prompt: String, visibility: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let now = now_iso();
    let id = new_id();
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO quick_action (id, workspace_id, name, description, assignee_type, assignee_id, prompt, visibility, status, use_count, created_by_id, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',0,'u-local',?9,?9)",
        params![id, ws, name, description.unwrap_or_default(), assignee_type, assignee_id, prompt, visibility.unwrap_or_else(|| "workspace".into()), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "quick_action", &id);
    let mut stmt = conn.prepare(&format!("SELECT {} FROM quick_action WHERE id = ?1", QA_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], quick_action_json).map_err(|e| e.to_string())?;
    Ok(rows.next().unwrap().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_quick_action(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, description: Option<String>, assignee_type: Option<String>, assignee_id: Option<String>, prompt: Option<String>, visibility: Option<String>, status: Option<String>) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    for (col, v) in [("name", name), ("description", description), ("assignee_type", assignee_type), ("assignee_id", assignee_id), ("prompt", prompt), ("visibility", visibility), ("status", status)] {
        if let Some(v) = v { sets.push(format!("{} = ?", col)); vals.push(v.into()); }
    }
    let sql = format!("UPDATE quick_action SET {} WHERE id = ?{}", sets.join(", "), vals.len() + 1);
    vals.push(id.clone().into());
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM quick_action WHERE id = ?1", QA_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id.clone()], quick_action_json).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "quick_action", &id);
    Ok(out)
}

#[tauri::command]
pub fn delete_quick_action(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM quick_action WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "quick_action", &id);
    Ok(())
}

#[tauri::command]
pub fn run_quick_action(app: tauri::AppHandle, db: State<Db>, issue_id: String, quick_action_id: String) -> Result<serde_json::Value, String> {
    let (assignee_type, assignee_id, prompt, name) = {
        let conn = db.0.lock().unwrap();
        conn.query_row(
            "SELECT assignee_type, assignee_id, prompt, name FROM quick_action WHERE id = ?1",
            params![quick_action_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)),
        ).map_err(|_| format!("quick action {} 不存在", quick_action_id))?
    };
    if assignee_type != "agent" {
        return Err("quick action 只支持 agent 指派".into());
    }
    let (agent_name, cli, title) = {
        let conn = db.0.lock().unwrap();
        let a = conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![assignee_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_else(|_| ("Claude".to_string(), "claude".to_string()));
        let t = conn.query_row("SELECT title FROM issue WHERE id = ?1", params![issue_id], |r| r.get::<_, String>(0)).unwrap_or_default();
        (a.0, a.1, t)
    };
    let full_prompt = format!("快捷操作「{}」\nIssue: {}\n\n{}", name, title, prompt);
    let run = super::spawn_run(&app, &db, &issue_id, &assignee_id, &agent_name, &cli, &full_prompt)?;
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE quick_action SET last_used_at = ?1, use_count = use_count + 1 WHERE id = ?2", params![now_iso(), quick_action_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "quick_action", &quick_action_id);
    Ok(serde_json::json!({ "task_id": run.id, "run_id": run.id, "issue_id": issue_id, "quick_action_id": quick_action_id }))
}

// ── Batch / rerun ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn batch_update_issues(app: tauri::AppHandle, db: State<Db>, issue_ids: Vec<String>, updates: serde_json::Value) -> Result<serde_json::Value, String> {
    if issue_ids.is_empty() { return Ok(serde_json::json!({ "updated": 0 })); }
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    for (col, key) in [("title", "title"), ("status", "status"), ("priority", "priority"), ("project_id", "project_id"), ("assignee_type", "assignee_type"), ("assignee_id", "assignee_id")] {
        if let Some(v) = updates.get(key).and_then(|v| v.as_str()) {
            sets.push(format!("{} = ?", col)); vals.push(v.to_string().into());
        }
    }
    if let Some(v) = updates.get("body").and_then(|v| v.as_str()) {
        sets.push("body = ?".into()); vals.push(v.to_string().into());
    }
    if let Some(v) = updates.get("description").and_then(|v| v.as_str()) {
        sets.push("body = ?".into()); vals.push(v.to_string().into());
    }
    let placeholders = issue_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE issue SET {} WHERE id IN ({})", sets.join(", "), placeholders);
    for id in &issue_ids { vals.push(id.clone().into()); }
    let n = conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", "batch");
    Ok(serde_json::json!({ "updated": n }))
}

#[tauri::command]
pub fn batch_delete_issues(app: tauri::AppHandle, db: State<Db>, issue_ids: Vec<String>) -> Result<serde_json::Value, String> {
    if issue_ids.is_empty() { return Ok(serde_json::json!({ "deleted": 0 })); }
    let conn = db.0.lock().unwrap();
    let placeholders = issue_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let vals: Vec<rusqlite::types::Value> = issue_ids.iter().map(|id| id.clone().into()).collect();
    conn.execute(&format!("DELETE FROM comment WHERE issue_id IN ({})", placeholders), rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    conn.execute(&format!("DELETE FROM issue_label WHERE issue_id IN ({})", placeholders), rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    let n = conn.execute(&format!("DELETE FROM issue WHERE id IN ({})", placeholders), rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    emit_changed(&app, "issue", "batch");
    Ok(serde_json::json!({ "deleted": n }))
}

#[tauri::command]
pub fn rerun_issue(app: tauri::AppHandle, db: State<Db>, issue_id: String, task_id: Option<String>) -> Result<serde_json::Value, String> {
    // 取消旧 run（若指定），按当前指派重新起 run。
    if let Some(t) = task_id {
        let _ = super::cancel_run_by_id(&app, &db, &t);
    }
    let (assignee_type, assignee_id) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT assignee_type, assignee_id FROM issue WHERE id = ?1", params![issue_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))).map_err(|_| format!("issue {} 不存在", issue_id))?
    };
    if assignee_type.as_deref() != Some("agent") || assignee_id.is_none() {
        return Err("issue 未指派给 agent，无法 rerun".into());
    }
    let agent_id = assignee_id.unwrap();
    let (agent_name, cli, title, body) = {
        let conn = db.0.lock().unwrap();
        let a = conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_else(|_| ("Claude".to_string(), "claude".to_string()));
        let i = conn.query_row("SELECT title, body FROM issue WHERE id = ?1", params![issue_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_default();
        (a.0, a.1, i.0, i.1)
    };
    let prompt = format!("Issue: {}\n\n{}\n\n请重新完成该 issue，必要时修改工作目录下的文件，并用简洁中文总结做了什么。", title, body);
    let run = super::spawn_run(&app, &db, &issue_id, &agent_id, &agent_name, &cli, &prompt)?;
    Ok(serde_json::json!({ "task_id": run.id, "run_id": run.id, "issue_id": issue_id }))
}

#[tauri::command]
pub fn retry_task_supplement(app: tauri::AppHandle, db: State<Db>, issue_id: String, task_id: String, comment_id: String) -> Result<serde_json::Value, String> {
    // 用补充评论的内容作为追加上下文，重新起 run。
    let _ = super::cancel_run_by_id(&app, &db, &task_id);
    let supplement = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT content FROM comment WHERE id = ?1", params![comment_id], |r| r.get::<_, String>(0)).unwrap_or_default()
    };
    let (assignee_type, assignee_id) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT assignee_type, assignee_id FROM issue WHERE id = ?1", params![issue_id], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))).map_err(|_| format!("issue {} 不存在", issue_id))?
    };
    if assignee_type.as_deref() != Some("agent") || assignee_id.is_none() {
        return Err("issue 未指派给 agent，无法重试".into());
    }
    let agent_id = assignee_id.unwrap();
    let (agent_name, cli, title) = {
        let conn = db.0.lock().unwrap();
        let a = conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_else(|_| ("Claude".to_string(), "claude".to_string()));
        let t = conn.query_row("SELECT title FROM issue WHERE id = ?1", params![issue_id], |r| r.get::<_, String>(0)).unwrap_or_default();
        (a.0, a.1, t)
    };
    let prompt = format!("Issue: {}\n\n补充说明：\n{}\n\n请按补充说明继续/修正该 issue，并用简洁中文总结做了什么。", title, supplement);
    let run = super::spawn_run(&app, &db, &issue_id, &agent_id, &agent_name, &cli, &prompt)?;
    Ok(serde_json::json!({ "task_id": run.id, "run_id": run.id, "issue_id": issue_id }))
}

#[tauri::command]
pub fn retry_source_context_quick_create(app: tauri::AppHandle, db: State<Db>, task_id: String) -> Result<serde_json::Value, String> {
    // 任务的源上下文快照缺失时重试：本地版无快照，重起该任务的 run。
    let (issue_id, agent_id) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT issue_id, agent_id FROM run WHERE id = ?1", params![task_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|_| format!("task {} 不存在", task_id))?
    };
    let _ = super::cancel_run_by_id(&app, &db, &task_id);
    let (agent_name, cli, title, body) = {
        let conn = db.0.lock().unwrap();
        let a = conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![agent_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_else(|_| ("Claude".to_string(), "claude".to_string()));
        let i = conn.query_row("SELECT title, body FROM issue WHERE id = ?1", params![issue_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap_or_default();
        (a.0, a.1, i.0, i.1)
    };
    let prompt = format!("Issue: {}\n\n{}\n\n请完成该 issue，必要时修改工作目录下的文件，并用简洁中文总结做了什么。", title, body);
    let run = super::spawn_run(&app, &db, &issue_id, &agent_id, &agent_name, &cli, &prompt)?;
    Ok(serde_json::json!({ "task_id": run.id, "run_id": run.id, "issue_id": issue_id }))
}

// ── Search ────────────────────────────────────────────────────────────────

fn search_cursor(conn: &rusqlite::Connection) -> String {
    let ic: i64 = conn.query_row("SELECT COUNT(*) FROM issue", [], |r| r.get(0)).unwrap_or(0);
    let cc: i64 = conn.query_row("SELECT COUNT(*) FROM comment", [], |r| r.get(0)).unwrap_or(0);
    let pc: i64 = conn.query_row("SELECT COUNT(*) FROM project", [], |r| r.get(0)).unwrap_or(0);
    format!("v1:{}:{}:{}", ic, cc, pc)
}

#[tauri::command]
pub fn get_search_index_snapshot(db: State<Db>, workspace_id: String) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut issues = vec![];
    {
        let mut stmt = conn.prepare("SELECT id, identifier, title, body, status, updated_at FROM issue WHERE workspace_id = ?1 ORDER BY updated_at DESC LIMIT 2000").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([ws.clone()], |r| Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "identifier": r.get::<_, String>(1)?,
            "title": r.get::<_, String>(2)?, "body": r.get::<_, String>(3)?,
            "status": r.get::<_, String>(4)?, "updated_at": r.get::<_, String>(5)?,
        }))).map_err(|e| e.to_string())?;
        issues = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    }
    let mut comments = vec![];
    {
        let mut stmt = conn.prepare("SELECT c.id, c.issue_id, c.content, c.created_at FROM comment c JOIN issue i ON i.id = c.issue_id WHERE i.workspace_id = ?1 ORDER BY c.created_at DESC LIMIT 2000").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([ws.clone()], |r| Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "issue_id": r.get::<_, String>(1)?,
            "content": r.get::<_, String>(2)?, "created_at": r.get::<_, String>(3)?,
        }))).map_err(|e| e.to_string())?;
        comments = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    }
    let mut projects = vec![];
    {
        let mut stmt = conn.prepare("SELECT id, title, description, status FROM project WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT 500").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([ws.clone()], |r| Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
            "description": r.get::<_, Option<String>>(2)?, "status": r.get::<_, String>(3)?,
        }))).map_err(|e| e.to_string())?;
        projects = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    }
    let cursor = search_cursor(&conn);
    Ok(serde_json::json!({
        "issues": issues, "comments": comments, "projects": projects,
        "cursor": cursor, "next_after_number": 0,
    }))
}

#[tauri::command]
pub fn get_search_index_changes(db: State<Db>, workspace_id: String, cursor: String, limit: Option<i64>) -> Result<serde_json::Value, String> {
    let _ = limit;
    let (ws, current) = {
        let conn = db.0.lock().unwrap();
        (ws_id(&conn, &workspace_id), search_cursor(&conn))
    };
    if cursor == current {
        return Ok(serde_json::json!({ "changes": [], "next_cursor": current }));
    }
    // 游标不匹配：返回全量快照作为 upsert 变更（增量同步的正确回退）。
    let snap = get_search_index_snapshot(db, ws)?;
    let changes = {
        let mut v = vec![];
        for it in snap["issues"].as_array().cloned().unwrap_or_default() {
            v.push(serde_json::json!({ "kind": "issue", "op": "upsert", "data": it }));
        }
        for it in snap["comments"].as_array().cloned().unwrap_or_default() {
            v.push(serde_json::json!({ "kind": "comment", "op": "upsert", "data": it }));
        }
        for it in snap["projects"].as_array().cloned().unwrap_or_default() {
            v.push(serde_json::json!({ "kind": "project", "op": "upsert", "data": it }));
        }
        v
    };
    Ok(serde_json::json!({ "changes": changes, "next_cursor": current }))
}

#[tauri::command]
pub fn search_issues(db: State<Db>, q: String, limit: Option<i64>, offset: Option<i64>, include_closed: Option<bool>) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let conn = db.0.lock().unwrap();
    let like = format!("%{}%", q.replace('%', "").replace('_', ""));
    let mut stmt = if include_closed.unwrap_or(false) {
        conn.prepare("SELECT id, identifier, title, body, status FROM issue WHERE title LIKE ?1 OR body LIKE ?1 ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3").map_err(|e| e.to_string())?
    } else {
        conn.prepare("SELECT id, identifier, title, body, status FROM issue WHERE (title LIKE ?1 OR body LIKE ?1) AND status NOT IN ('done','cancelled') ORDER BY updated_at DESC LIMIT ?2 OFFSET ?3").map_err(|e| e.to_string())?
    };
    let rows = stmt.query_map(params![like, limit, offset], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "identifier": r.get::<_, String>(1)?,
        "title": r.get::<_, String>(2)?, "body": r.get::<_, String>(3)?, "status": r.get::<_, String>(4)?,
    }))).map_err(|e| e.to_string())?;
    let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "issues": items, "total": items.len() }))
}

#[tauri::command]
pub fn search_projects(db: State<Db>, q: String, limit: Option<i64>, offset: Option<i64>, include_closed: Option<bool>) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let conn = db.0.lock().unwrap();
    let like = format!("%{}%", q.replace('%', "").replace('_', ""));
    let mut stmt = if include_closed.unwrap_or(false) {
        conn.prepare("SELECT id, title, description, status FROM project WHERE title LIKE ?1 OR description LIKE ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3").map_err(|e| e.to_string())?
    } else {
        conn.prepare("SELECT id, title, description, status FROM project WHERE (title LIKE ?1 OR description LIKE ?1) AND status NOT IN ('completed','cancelled') ORDER BY created_at DESC LIMIT ?2 OFFSET ?3").map_err(|e| e.to_string())?
    };
    let rows = stmt.query_map(params![like, limit, offset], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "title": r.get::<_, String>(1)?,
        "description": r.get::<_, Option<String>>(2)?, "status": r.get::<_, String>(3)?,
    }))).map_err(|e| e.to_string())?;
    let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "projects": items, "total": items.len() }))
}

// ── Wakeup：workspace 列表 / 系统 wakeup ──────────────────────────────────

#[tauri::command]
pub fn list_workspace_wakeups(db: State<Db>, workspace_id: Option<String>, scope: Option<String>, kind: Option<String>, source: Option<String>, search: Option<String>, agent_id: Option<String>, offset: Option<i64>, limit: Option<i64>) -> Result<serde_json::Value, String> {
    let _ = (scope, source);
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let conn = db.0.lock().unwrap();
    let mut sql = "SELECT w.id, w.issue_id, w.agent_id, w.agent_name, w.instruction, w.kind, w.mode, w.next_fire_at, w.enabled, w.created_at FROM wakeup w JOIN issue i ON i.id = w.issue_id WHERE 1=1".to_string();
    let mut vals: Vec<rusqlite::types::Value> = vec![];
    if let Some(ws) = workspace_id {
        let ws = ws_id(&conn, &ws);
        sql.push_str(" AND i.workspace_id = ?");
        vals.push(ws.into());
    }
    if let Some(k) = kind {
        if k != "all" { sql.push_str(" AND w.kind = ?"); vals.push(k.into()); }
    }
    if let Some(a) = agent_id {
        if !a.is_empty() { sql.push_str(" AND w.agent_id = ?"); vals.push(a.into()); }
    }
    if let Some(s) = search {
        if !s.is_empty() { sql.push_str(" AND w.instruction LIKE ?"); vals.push(format!("%{}%", s).into()); }
    }
    sql.push_str(" ORDER BY w.created_at DESC LIMIT ? OFFSET ?");
    vals.push(limit.into());
    vals.push(offset.into());
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "issue_id": r.get::<_, String>(1)?,
        "agent_id": r.get::<_, String>(2)?, "agent_name": r.get::<_, String>(3)?,
        "instruction": r.get::<_, String>(4)?, "kind": r.get::<_, String>(5)?,
        "mode": r.get::<_, String>(6)?, "next_fire_at": r.get::<_, Option<String>>(7)?,
        "enabled": r.get::<_, i64>(8)? != 0, "created_at": r.get::<_, String>(9)?,
    }))).map_err(|e| e.to_string())?;
    let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "wakeups": items, "total": items.len(), "offset": offset, "limit": limit }))
}

/// 系统 wakeup 的内置规则（原服务端内置，与前端 SystemWakeup rule 枚举一致）。
const SYSTEM_WAKEUP_RULES: &[&str] = &[
    "issue_mentioned", "issue_commented", "issue_status_changed",
    "issue_assigned", "pr_opened", "pr_merged", "ci_failed",
];

#[tauri::command]
pub fn list_workspace_system_wakeups(db: State<Db>, workspace_id: Option<String>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let ws = match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) };
    let mut out = vec![];
    for rule in SYSTEM_WAKEUP_RULES {
        let row = conn.query_row(
            "SELECT enabled, instruction FROM system_wakeup WHERE workspace_id = ?1 AND issue_id IS NULL AND rule = ?2",
            params![ws, rule],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ).unwrap_or((0, String::new()));
        out.push(serde_json::json!({ "rule": rule, "enabled": row.0 != 0, "instruction": row.1, "workspace_id": ws }));
    }
    Ok(out)
}

#[tauri::command]
pub fn list_issue_system_wakeups(db: State<Db>, issue_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let ws: String = conn.query_row("SELECT workspace_id FROM issue WHERE id = ?1", params![issue_id], |r| r.get(0)).unwrap_or_default();
    let mut out = vec![];
    for rule in SYSTEM_WAKEUP_RULES {
        let row = conn.query_row(
            "SELECT enabled, instruction FROM system_wakeup WHERE workspace_id = ?1 AND issue_id = ?2 AND rule = ?3",
            params![ws, issue_id, rule],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ).unwrap_or((0, String::new()));
        out.push(serde_json::json!({ "rule": rule, "enabled": row.0 != 0, "instruction": row.1, "issue_id": issue_id }));
    }
    Ok(out)
}

fn upsert_system_wakeup(conn: &rusqlite::Connection, ws: &str, issue_id: Option<&str>, rule: &str, enabled: Option<bool>, instruction: Option<String>) -> Result<(), String> {
    let now = now_iso();
    let (en, ins) = {
        let row = conn.query_row(
            "SELECT enabled, instruction FROM system_wakeup WHERE workspace_id = ?1 AND issue_id IS ?2 AND rule = ?3",
            params![ws, issue_id, rule],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        ).unwrap_or((0, String::new()));
        (enabled.map(|b| if b { 1i64 } else { 0i64 }).unwrap_or(row.0), instruction.unwrap_or(row.1))
    };
    conn.execute(
        "INSERT INTO system_wakeup (id, workspace_id, issue_id, rule, enabled, instruction, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7) ON CONFLICT(workspace_id, issue_id, rule) DO UPDATE SET enabled = excluded.enabled, instruction = excluded.instruction, updated_at = excluded.updated_at",
        params![new_id(), ws, issue_id, rule, en, ins, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn update_issue_system_wakeup(app: tauri::AppHandle, db: State<Db>, issue_id: String, rule: String, enabled: Option<bool>, instruction: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); conn.query_row("SELECT workspace_id FROM issue WHERE id = ?1", params![issue_id], |r| r.get::<_, String>(0)).unwrap_or_default() };
    let conn = db.0.lock().unwrap();
    upsert_system_wakeup(&conn, &ws, Some(&issue_id), &rule, enabled, instruction.clone())?;
    emit_changed(&app, "wakeup", &issue_id);
    Ok(serde_json::json!({ "issue_id": issue_id, "rule": rule, "enabled": enabled, "instruction": instruction }))
}

#[tauri::command]
pub fn update_workspace_system_wakeup(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, rule: String, enabled: Option<bool>, instruction: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) } };
    let conn = db.0.lock().unwrap();
    upsert_system_wakeup(&conn, &ws, None, &rule, enabled, instruction.clone())?;
    emit_changed(&app, "wakeup", &ws);
    Ok(serde_json::json!({ "workspace_id": ws, "rule": rule, "enabled": enabled, "instruction": instruction }))
}

#[tauri::command]
pub fn enable_issue_wakeup(app: tauri::AppHandle, db: State<Db>, issue_id: String, wakeup_id: String, revision: Option<i64>, at: Option<String>, rearm: Option<bool>) -> Result<serde_json::Value, String> {
    let _ = (revision, rearm);
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE wakeup SET enabled = 1, next_fire_at = COALESCE(?1, next_fire_at), updated_at = ?2 WHERE id = ?3 AND issue_id = ?4", params![at, now_iso(), wakeup_id, issue_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &wakeup_id);
    Ok(serde_json::json!({ "id": wakeup_id, "enabled": true }))
}

#[tauri::command]
pub fn disable_issue_wakeup(app: tauri::AppHandle, db: State<Db>, issue_id: String, wakeup_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE wakeup SET enabled = 0, updated_at = ?1 WHERE id = ?2 AND issue_id = ?3", params![now_iso(), wakeup_id, issue_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &wakeup_id);
    Ok(serde_json::json!({ "id": wakeup_id, "enabled": false }))
}

#[tauri::command]
pub fn edit_issue_wakeup_instruction(app: tauri::AppHandle, db: State<Db>, issue_id: String, wakeup_id: String, instruction: String, expected_instruction: String, revision: Option<i64>) -> Result<serde_json::Value, String> {
    let _ = revision;
    let conn = db.0.lock().unwrap();
    let cur: String = conn.query_row("SELECT instruction FROM wakeup WHERE id = ?1 AND issue_id = ?2", params![wakeup_id, issue_id], |r| r.get(0)).map_err(|_| format!("wakeup {} 不存在", wakeup_id))?;
    if cur != expected_instruction {
        return Err("wakeup instruction 已被他人修改，请刷新后重试".into());
    }
    conn.execute("UPDATE wakeup SET instruction = ?1, updated_at = ?2 WHERE id = ?3", params![instruction, now_iso(), wakeup_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &wakeup_id);
    Ok(serde_json::json!({ "id": wakeup_id, "instruction": instruction }))
}

#[tauri::command]
pub fn list_issue_wakeup_runs(db: State<Db>, issue_id: String, wakeup_id: String) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, wakeup_id, status, detail, created_at FROM wakeup_run WHERE wakeup_id = ?1 ORDER BY created_at DESC LIMIT 100").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([wakeup_id], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "wakeup_id": r.get::<_, String>(1)?,
        "issue_id": issue_id, "status": r.get::<_, String>(2)?,
        "detail": r.get::<_, String>(3)?, "created_at": r.get::<_, String>(4)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_issue_trigger(db: State<Db>, issue_ids: Option<Vec<String>>, is_create: Option<bool>, assignee_type: Option<String>, assignee_id: Option<String>, status: Option<String>) -> Result<serde_json::Value, String> {
    let _ = (is_create, assignee_type, assignee_id, status);
    let conn = db.0.lock().unwrap();
    let ids = issue_ids.unwrap_or_default();
    let mut matched = vec![];
    for id in &ids {
        let title: Option<String> = conn.query_row("SELECT title FROM issue WHERE id = ?1", params![id], |r| r.get(0)).ok();
        if let Some(t) = title { matched.push(serde_json::json!({ "id": id, "title": t })); }
    }
    Ok(serde_json::json!({ "matched_issues": matched, "total": matched.len() }))
}

#[tauri::command]
pub fn preview_comment_triggers(db: State<Db>, issue_id: String, content: String, parent_id: Option<String>, editing_comment_id: Option<String>) -> Result<serde_json::Value, String> {
    let _ = (parent_id, editing_comment_id);
    let conn = db.0.lock().unwrap();
    // 评论触发匹配：内容含 @agent 或 /命令 时提示会触发的 wakeup。
    let mut stmt = conn.prepare("SELECT id, agent_name, instruction FROM wakeup WHERE issue_id = ?1 AND enabled = 1 AND kind = 'event'").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id.clone()], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "agent_name": r.get::<_, String>(1)?, "instruction": r.get::<_, String>(2)?,
    }))).map_err(|e| e.to_string())?;
    let triggers = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let mentioned = content.contains('@');
    Ok(serde_json::json!({ "issue_id": issue_id, "mentioned": mentioned, "triggers": triggers }))
}

// ── Autopilot trigger / delivery ──────────────────────────────────────────

fn trigger_json(r: &Row) -> rusqlite::Result<serde_json::Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "autopilot_id": r.get::<_, String>(1)?,
        "kind": r.get::<_, String>(2)?,
        "cron_expression": r.get::<_, Option<String>>(3)?,
        "timezone": r.get::<_, String>(4)?,
        "label": r.get::<_, String>(5)?,
        "event_filters": parse_json_pub(&r.get::<_, String>(6)?, serde_json::json!([])),
        "enabled": r.get::<_, i64>(7)? != 0,
        "webhook_token": r.get::<_, Option<String>>(8)?,
        "created_at": r.get::<_, String>(9)?,
        "updated_at": r.get::<_, String>(10)?,
    }))
}

const TRIGGER_COLS: &str = "id, autopilot_id, kind, cron_expression, timezone, label, event_filters, enabled, webhook_token, created_at, updated_at";

#[tauri::command]
pub fn create_autopilot_trigger(app: tauri::AppHandle, db: State<Db>, autopilot_id: String, kind: String, cron_expression: Option<String>, timezone: Option<String>, label: Option<String>, event_filters: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let now = now_iso();
    let id = new_id();
    let token = if kind == "webhook" { Some(new_id()) } else { None };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO autopilot_trigger (id, autopilot_id, kind, cron_expression, timezone, label, event_filters, enabled, webhook_token, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,?9,?9)",
        params![id, autopilot_id, kind, cron_expression, timezone.unwrap_or_else(|| "Asia/Shanghai".into()), label.unwrap_or_default(), j(event_filters.unwrap_or(serde_json::json!([]))), token, now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &autopilot_id);
    let mut stmt = conn.prepare(&format!("SELECT {} FROM autopilot_trigger WHERE id = ?1", TRIGGER_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], trigger_json).map_err(|e| e.to_string())?;
    Ok(rows.next().unwrap().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_autopilot_trigger(app: tauri::AppHandle, db: State<Db>, autopilot_id: String, trigger_id: String, enabled: Option<bool>, cron_expression: Option<String>, timezone: Option<String>, label: Option<String>, event_filters: Option<serde_json::Value>) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    if let Some(v) = enabled { sets.push("enabled = ?".into()); vals.push((if v { 1i64 } else { 0i64 }).into()); }
    if let Some(v) = cron_expression { sets.push("cron_expression = ?".into()); vals.push(v.into()); }
    if let Some(v) = timezone { sets.push("timezone = ?".into()); vals.push(v.into()); }
    if let Some(v) = label { sets.push("label = ?".into()); vals.push(v.into()); }
    if let Some(v) = event_filters { sets.push("event_filters = ?".into()); vals.push(j(v).into()); }
    let sql = format!("UPDATE autopilot_trigger SET {} WHERE id = ?{} AND autopilot_id = ?{}", sets.join(", "), vals.len() + 1, vals.len() + 2);
    vals.push(trigger_id.clone().into());
    vals.push(autopilot_id.clone().into());
    conn.execute(&sql, rusqlite::params_from_iter(vals.iter())).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(&format!("SELECT {} FROM autopilot_trigger WHERE id = ?1", TRIGGER_COLS)).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([trigger_id.clone()], trigger_json).map_err(|e| e.to_string())?;
    let out = match rows.next() { Some(r) => Some(r.map_err(|e| e.to_string())?), None => None };
    drop(rows);
    emit_changed(&app, "autopilot", &autopilot_id);
    Ok(out)
}

#[tauri::command]
pub fn delete_autopilot_trigger(app: tauri::AppHandle, db: State<Db>, autopilot_id: String, trigger_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM autopilot_trigger WHERE id = ?1 AND autopilot_id = ?2", params![trigger_id, autopilot_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &autopilot_id);
    Ok(())
}

#[tauri::command]
pub fn trigger_autopilot(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    // 手动触发：记一条 delivery + autopilot_run（执行由 autopilot 自身的 run 调度，本地版记 delivered）。
    let now = now_iso();
    let conn = db.0.lock().unwrap();
    let title: String = conn.query_row("SELECT title FROM autopilot WHERE id = ?1", params![id], |r| r.get(0)).map_err(|_| format!("autopilot {} 不存在", id))?;
    let run_id = new_id();
    conn.execute(
        "INSERT INTO autopilot_run (id, autopilot_id, status, created_at) VALUES (?1,?2,'running',?3)",
        params![run_id, id, now],
    ).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO autopilot_delivery (id, autopilot_id, run_id, status, detail, created_at) VALUES (?1,?2,?3,'delivered',?4,?5)",
        params![new_id(), id, run_id, format!("手动触发：{}", title), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "autopilot", &id);
    Ok(serde_json::json!({ "autopilot_id": id, "run_id": run_id, "status": "running" }))
}

#[tauri::command]
pub fn list_autopilot_deliveries(db: State<Db>, autopilot_id: String, limit: Option<i64>, offset: Option<i64>) -> Result<serde_json::Value, String> {
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, autopilot_id, run_id, trigger_id, status, detail, created_at FROM autopilot_delivery WHERE autopilot_id = ?1 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![autopilot_id, limit, offset], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "autopilot_id": r.get::<_, String>(1)?,
        "run_id": r.get::<_, Option<String>>(2)?, "trigger_id": r.get::<_, Option<String>>(3)?,
        "status": r.get::<_, String>(4)?, "detail": r.get::<_, String>(5)?,
        "created_at": r.get::<_, String>(6)?,
    }))).map_err(|e| e.to_string())?;
    let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "deliveries": items, "total": items.len() }))
}

#[tauri::command]
pub fn get_autopilot_run(db: State<Db>, autopilot_id: String, run_id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(
        "SELECT id, autopilot_id, status, created_at FROM autopilot_run WHERE id = ?1 AND autopilot_id = ?2",
        params![run_id, autopilot_id],
        |r| Ok(serde_json::json!({ "id": r.get::<_, String>(0)?, "autopilot_id": r.get::<_, String>(1)?, "status": r.get::<_, String>(2)?, "created_at": r.get::<_, String>(3)? })),
    );
    match row { Ok(v) => Ok(Some(v)), Err(_) => Ok(None) }
}

#[tauri::command]
pub fn get_autopilot_delivery(db: State<Db>, autopilot_id: String, delivery_id: String) -> Result<Option<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let row = conn.query_row(
        "SELECT id, autopilot_id, run_id, trigger_id, status, detail, created_at FROM autopilot_delivery WHERE id = ?1 AND autopilot_id = ?2",
        params![delivery_id, autopilot_id],
        |r| Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "autopilot_id": r.get::<_, String>(1)?,
            "run_id": r.get::<_, Option<String>>(2)?, "trigger_id": r.get::<_, Option<String>>(3)?,
            "status": r.get::<_, String>(4)?, "detail": r.get::<_, String>(5)?,
            "created_at": r.get::<_, String>(6)?,
        })),
    );
    match row { Ok(v) => Ok(Some(v)), Err(_) => Ok(None) }
}

// ── Agent skill / env ─────────────────────────────────────────────────────

#[tauri::command]
pub fn set_agent_skills(app: tauri::AppHandle, db: State<Db>, agent_id: String, skill_ids: Vec<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE agent SET skills = ?1, updated_at = ?2 WHERE id = ?3", params![j(serde_json::json!(skill_ids)), now_iso(), agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "skill_ids": skill_ids }))
}

#[tauri::command]
pub fn add_agent_skills(app: tauri::AppHandle, db: State<Db>, agent_id: String, skill_ids: Vec<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let cur: String = conn.query_row("SELECT skills FROM agent WHERE id = ?1", params![agent_id], |r| r.get(0)).unwrap_or_else(|_| "[]".into());
    let mut arr = parse_json_pub(&cur, serde_json::json!([]));
    if !arr.is_array() { arr = serde_json::json!([]); }
    let list = arr.as_array_mut().unwrap();
    for id in &skill_ids {
        if !list.iter().any(|v| v.as_str() == Some(id)) {
            list.push(serde_json::json!(id));
        }
    }
    conn.execute("UPDATE agent SET skills = ?1, updated_at = ?2 WHERE id = ?3", params![j(arr.clone()), now_iso(), agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "skill_ids": arr }))
}

#[tauri::command]
pub fn remove_agent_skill(app: tauri::AppHandle, db: State<Db>, agent_id: String, skill_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let cur: String = conn.query_row("SELECT skills FROM agent WHERE id = ?1", params![agent_id], |r| r.get(0)).unwrap_or_else(|_| "[]".into());
    let mut arr = parse_json_pub(&cur, serde_json::json!([]));
    if let Some(list) = arr.as_array_mut() {
        list.retain(|v| v.as_str() != Some(skill_id.as_str()));
    }
    conn.execute("UPDATE agent SET skills = ?1, updated_at = ?2 WHERE id = ?3", params![j(arr.clone()), now_iso(), agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "skill_ids": arr }))
}

#[tauri::command]
pub fn set_agent_skill_enabled(app: tauri::AppHandle, db: State<Db>, agent_id: String, skill_id: String, enabled: bool) -> Result<serde_json::Value, String> {
    // skill 启用状态存在 agent.skills 的对象形态里：[{id, enabled}]；字符串形态先转对象形态。
    let conn = db.0.lock().unwrap();
    let cur: String = conn.query_row("SELECT skills FROM agent WHERE id = ?1", params![agent_id], |r| r.get(0)).unwrap_or_else(|_| "[]".into());
    let mut arr = parse_json_pub(&cur, serde_json::json!([]));
    if !arr.is_array() { arr = serde_json::json!([]); }
    let list = arr.as_array_mut().unwrap();
    let mut found = false;
    for v in list.iter_mut() {
        if v.as_str() == Some(skill_id.as_str()) {
            *v = serde_json::json!({ "id": skill_id, "enabled": enabled });
            found = true;
        } else if v.get("id").and_then(|x| x.as_str()) == Some(skill_id.as_str()) {
            v["enabled"] = serde_json::json!(enabled);
            found = true;
        }
    }
    if !found {
        list.push(serde_json::json!({ "id": skill_id, "enabled": enabled }));
    }
    conn.execute("UPDATE agent SET skills = ?1, updated_at = ?2 WHERE id = ?3", params![j(arr.clone()), now_iso(), agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "skill_id": skill_id, "enabled": enabled }))
}

#[tauri::command]
pub fn set_agent_runtime_skill_enabled(app: tauri::AppHandle, db: State<Db>, agent_id: String, runtime_id: String, root: String, key: String, name: String, plugin: Option<String>, enabled: bool) -> Result<serde_json::Value, String> {
    // runtime 技能开关存在 agent.runtime_config.runtime_skills 里。
    let conn = db.0.lock().unwrap();
    let cur: String = conn.query_row("SELECT runtime_config FROM agent WHERE id = ?1", params![agent_id], |r| r.get(0)).unwrap_or_else(|_| "{}".into());
    let mut cfg = parse_json_pub(&cur, serde_json::json!({}));
    if !cfg.is_object() { cfg = serde_json::json!({}); }
    let entry = serde_json::json!({ "runtime_id": runtime_id, "root": root, "key": key, "name": name, "plugin": plugin, "enabled": enabled });
    let skills = cfg.get_mut("runtime_skills").and_then(|v| v.as_array_mut());
    match skills {
        Some(list) => {
            if let Some(pos) = list.iter().position(|v| v.get("runtime_id").and_then(|x| x.as_str()) == Some(runtime_id.as_str()) && v.get("key").and_then(|x| x.as_str()) == Some(key.as_str())) {
                list[pos] = entry;
            } else {
                list.push(entry);
            }
        }
        None => {
            cfg["runtime_skills"] = serde_json::json!([entry]);
        }
    }
    conn.execute("UPDATE agent SET runtime_config = ?1, updated_at = ?2 WHERE id = ?3", params![j(cfg), now_iso(), agent_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "agent", &agent_id);
    Ok(serde_json::json!({ "agent_id": agent_id, "runtime_id": runtime_id, "key": key, "enabled": enabled }))
}

#[tauri::command]
pub fn update_agent_env(app: tauri::AppHandle, db: State<Db>, id: String, custom_env: serde_json::Value) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let obj = custom_env.as_object().cloned().unwrap_or_default();
    for (k, v) in &obj {
        let val = v.as_str().unwrap_or("").to_string();
        conn.execute(
            "INSERT INTO agent_env (agent_id, key, value) VALUES (?1,?2,?3) ON CONFLICT(agent_id, key) DO UPDATE SET value = excluded.value",
            params![id, k, val],
        ).map_err(|e| e.to_string())?;
    }
    emit_changed(&app, "agent", &id);
    Ok(serde_json::json!({ "agent_id": id, "custom_env": custom_env }))
}

#[tauri::command]
pub fn get_agent_env(db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT key, value FROM agent_env WHERE agent_id = ?1 ORDER BY key").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([id.clone()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    for row in rows {
        let (k, v) = row.map_err(|e| e.to_string())?;
        map.insert(k, serde_json::json!(v));
    }
    Ok(serde_json::json!({ "agent_id": id, "custom_env": map }))
}

// ── Attachment：上传 / 下载 ───────────────────────────────────────────────

fn attachments_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.path().app_data_dir().map(|d| d.join("attachments")).unwrap_or_else(|_| std::path::PathBuf::from("."))
}

#[tauri::command]
pub fn upload_file(app: tauri::AppHandle, db: State<Db>, filename: String, mime_type: String, data_base64: String, issue_id: Option<String>, comment_id: Option<String>, chat_session_id: Option<String>) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data_base64.trim()).map_err(|e| format!("base64 解码失败：{}", e))?;
    let _ = comment_id;
    let now = now_iso();
    let id = new_id();
    let dir = attachments_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&id);
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO attachment (id, issue_id, filename, mime_type, size, created_at, path, chat_session_id) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, issue_id, filename, mime_type, bytes.len() as i64, now, id, chat_session_id],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "attachment", &id);
    Ok(serde_json::json!({ "id": id, "issue_id": issue_id, "filename": filename, "mime_type": mime_type, "size": bytes.len(), "created_at": now }))
}

#[tauri::command]
pub fn get_attachment_blob(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    use base64::Engine;
    let (filename, mime_type, path) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT filename, mime_type, path FROM attachment WHERE id = ?1", params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(|_| format!("attachment {} 不存在", id))?
    };
    let path = path.unwrap_or_else(|| id.clone());
    let bytes = std::fs::read(attachments_dir(&app).join(&path)).map_err(|e| format!("附件文件缺失：{}", e))?;
    Ok(serde_json::json!({ "id": id, "filename": filename, "mime_type": mime_type, "size": bytes.len(), "data_base64": base64::engine::general_purpose::STANDARD.encode(&bytes) }))
}


#[tauri::command]
pub fn get_attachment_text_content(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    let (filename, mime_type, path) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT filename, mime_type, path FROM attachment WHERE id = ?1", params![id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(|_| format!("attachment {} 不存在", id))?
    };
    let bytes = std::fs::read(attachments_dir(&app).join(path.unwrap_or_else(|| id.clone()))).map_err(|e| format!("附件文件缺失：{}", e))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(serde_json::json!({ "id": id, "filename": filename, "mime_type": mime_type, "content": text }))
}

// ── Issue table：分组 / 子 issue 批量 ────────────────────────────────────

#[tauri::command]
pub fn list_issue_table_groups(db: State<Db>, workspace_id: String, group_by: Option<String>) -> Result<serde_json::Value, String> {
    let ws = { let conn = db.0.lock().unwrap(); ws_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let col = match group_by.as_deref() {
        Some("status") => "status",
        Some("priority") => "priority",
        Some("assignee_id") => "assignee_id",
        _ => "status",
    };
    let mut stmt = conn.prepare(&format!("SELECT COALESCE({}, ''), COUNT(*) FROM issue WHERE workspace_id = ?1 GROUP BY {} ORDER BY 2 DESC", col, col)).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([ws], |r| Ok(serde_json::json!({ "key": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)? }))).map_err(|e| e.to_string())?;
    let groups = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "groups": groups, "group_by": col }))
}

#[tauri::command]
pub fn list_children_by_parents(db: State<Db>, parent_ids: Vec<String>) -> Result<serde_json::Value, String> {
    if parent_ids.is_empty() { return Ok(serde_json::json!({ "children": {} })); }
    let conn = db.0.lock().unwrap();
    let placeholders = parent_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let vals: Vec<rusqlite::types::Value> = parent_ids.iter().map(|id| id.clone().into()).collect();
    let mut stmt = conn.prepare(&format!("SELECT parent_issue_id, id, title, status FROM issue WHERE parent_issue_id IN ({}) ORDER BY created_at", placeholders)).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(rusqlite::params_from_iter(vals.iter()), |r| Ok(serde_json::json!({
        "parent_issue_id": r.get::<_, String>(0)?, "id": r.get::<_, String>(1)?,
        "title": r.get::<_, String>(2)?, "status": r.get::<_, String>(3)?,
    }))).map_err(|e| e.to_string())?;
    let flat = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let mut map = serde_json::Map::new();
    for it in flat {
        let pid = it["parent_issue_id"].as_str().unwrap_or("").to_string();
        map.entry(pid).or_insert_with(|| serde_json::json!([])).as_array_mut().unwrap().push(it);
    }
    Ok(serde_json::json!({ "children": map }))
}

// ── 通知偏好 / 更新检查 ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_notification_preferences(db: State<Db>, workspace_id: Option<String>) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let ws = match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) };
    let prefs: Option<String> = conn.query_row("SELECT prefs FROM notification_preferences WHERE workspace_id = ?1", params![ws], |r| r.get(0)).ok();
    Ok(prefs.map(|p| parse_json_pub(&p, serde_json::json!({}))).unwrap_or(serde_json::json!({})))
}

#[tauri::command]
pub fn update_notification_preferences(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, preferences: serde_json::Value) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let ws = match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) };
    conn.execute(
        "INSERT INTO notification_preferences (workspace_id, prefs, updated_at) VALUES (?1,?2,?3) ON CONFLICT(workspace_id) DO UPDATE SET prefs = excluded.prefs, updated_at = excluded.updated_at",
        params![ws, j(preferences.clone()), now_iso()],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "notification_preferences", &ws);
    Ok(preferences)
}

#[tauri::command]
pub fn get_update_result(_db: State<Db>, runtime_id: String, update_id: String) -> Result<serde_json::Value, String> {
    // 本地版无更新服务器：CLI 更新由用户自行重装/包管理器完成，始终返回无更新。
    Ok(serde_json::json!({ "runtime_id": runtime_id, "update_id": update_id, "status": "not_supported", "message": "本地版不提供自动更新，请从发布渠道手动更新 CLI。" }))
}

#[tauri::command]
pub fn initiate_update(_db: State<Db>, runtime_id: String, target_version: String) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "runtime_id": runtime_id, "target_version": target_version, "status": "not_supported", "message": "本地版不提供自动更新，请从发布渠道手动更新 CLI。" }))
}

// ── Dashboard：运行时间 / 失败（真实 run 数据；token 用量本地不统计） ────

#[tauri::command]
pub fn get_dashboard_agent_run_time(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT agent_id, COALESCE(SUM(CASE WHEN finished_at IS NOT NULL THEN (julianday(finished_at) - julianday(started_at)) * 86400 ELSE 0 END), 0), COUNT(*), SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) FROM run GROUP BY agent_id ORDER BY 2 DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "agent_id": r.get::<_, String>(0)?,
        "total_seconds": r.get::<_, f64>(1)? as i64,
        "task_count": r.get::<_, i64>(2)?,
        "failed_count": r.get::<_, i64>(3)?,
        "cancelled_count": r.get::<_, i64>(4)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dashboard_run_time_daily(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT substr(started_at, 1, 10), COALESCE(SUM(CASE WHEN finished_at IS NOT NULL THEN (julianday(finished_at) - julianday(started_at)) * 86400 ELSE 0 END), 0), COUNT(*), SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) FROM run GROUP BY 1 ORDER BY 1 DESC LIMIT 30").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "date": r.get::<_, String>(0)?,
        "total_seconds": r.get::<_, f64>(1)? as i64,
        "task_count": r.get::<_, i64>(2)?,
        "failed_count": r.get::<_, i64>(3)?,
        "cancelled_count": r.get::<_, i64>(4)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dashboard_failures_daily(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT substr(started_at, 1, 10), COUNT(*) FROM run WHERE status = 'failed' GROUP BY 1 ORDER BY 1 DESC LIMIT 30").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "date": r.get::<_, String>(0)?, "failure_reason": "unknown", "count": r.get::<_, i64>(1)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dashboard_failures_by_agent(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT agent_id, COUNT(*) FROM run WHERE status = 'failed' GROUP BY agent_id ORDER BY 2 DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(serde_json::json!({
        "agent_id": r.get::<_, String>(0)?, "failure_reason": "unknown", "count": r.get::<_, i64>(1)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_dashboard_usage_daily(_db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    // token 用量本地不统计（run 不记录 token）：返回空，页面渲染 No usage。
    Ok(vec![])
}

#[tauri::command]
pub fn get_dashboard_usage_by_agent(_db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![])
}

// ── 频道 BYO 配置 ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn register_slack_byo(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, config: serde_json::Value) -> Result<serde_json::Value, String> {
    register_channel(&app, &db, workspace_id, "slack", config)
}

#[tauri::command]
pub fn register_telegram_bot(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, config: serde_json::Value) -> Result<serde_json::Value, String> {
    register_channel(&app, &db, workspace_id, "telegram", config)
}

#[tauri::command]
pub fn register_dingtalk_byo(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, config: serde_json::Value) -> Result<serde_json::Value, String> {
    register_channel(&app, &db, workspace_id, "dingtalk", config)
}

#[tauri::command]
pub fn register_wecom_byo(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, config: serde_json::Value) -> Result<serde_json::Value, String> {
    register_channel(&app, &db, workspace_id, "wecom", config)
}

fn register_channel(app: &tauri::AppHandle, db: &Db, workspace_id: Option<String>, provider: &str, config: serde_json::Value) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let ws = match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) };
    let now = now_iso();
    conn.execute(
        "INSERT INTO channel_installation (id, workspace_id, provider, config, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5) ON CONFLICT(workspace_id, provider) DO UPDATE SET config = excluded.config, updated_at = excluded.updated_at",
        params![new_id(), ws, provider, j(config.clone()), now],
    ).map_err(|e| e.to_string())?;
    emit_changed(app, "channel", provider);
    Ok(serde_json::json!({ "provider": provider, "workspace_id": ws, "configured": true, "config": config }))
}

#[tauri::command]
pub fn delete_channel_installation(app: tauri::AppHandle, db: State<Db>, workspace_id: Option<String>, provider: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let ws = match workspace_id { Some(w) => ws_id(&conn, &w), None => default_ws(&conn) };
    conn.execute("DELETE FROM channel_installation WHERE workspace_id = ?1 AND provider = ?2", params![ws, provider]).map_err(|e| e.to_string())?;
    emit_changed(&app, "channel", &provider);
    Ok(())
}

pub fn channel_installations(db: &Db, provider: &str) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, provider, config, created_at, updated_at FROM channel_installation WHERE provider = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([provider], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?,
        "provider": r.get::<_, String>(2)?,
        "config": parse_json_pub(&r.get::<_, String>(3)?, serde_json::json!({})),
        "created_at": r.get::<_, String>(4)?, "updated_at": r.get::<_, String>(5)?,
    }))).map_err(|e| e.to_string())?;
    let installations = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "installations": installations, "configured": !installations.is_empty() }))
}
