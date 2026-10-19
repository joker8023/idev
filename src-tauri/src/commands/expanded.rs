//! 扩展命令：project / squad / member / label / pin / view / property / wakeup /
//! inbox / chat / search / attachment / timeline / status / table / tasks /
//! dashboard 等，覆盖 26 路由的读路径 + 主要写路径。
//! 约定：
//! - 空态返回空数组/空对象（前端渲染空态），不抛错；
//! - 聊天频道安装 `configured: false`（本地版无云端密钥，页面渲染“请运营方开启”）；
//! - Dashboard 用量统计返回空（本地版暂不统计 token 用量，页面渲染 No usage）。

use rusqlite::{params, Row};
use tauri::State;
use crate::db::Db;
use crate::models::*;
use super::{emit_changed, push_set};

fn project_from_row(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?, workspace_id: r.get(1)?, title: r.get(2)?,
        description: r.get(3)?, icon: r.get(4)?, status: r.get(5)?, priority: r.get(6)?,
        lead_type: r.get(7)?, lead_id: r.get(8)?, start_date: r.get(9)?, due_date: r.get(10)?,
        issue_count: r.get(11)?, done_count: r.get(12)?, resource_count: r.get(13)?,
        created_at: r.get(14)?, updated_at: r.get(15)?,
    })
}

const PROJECT_COLS: &str = "p.id, p.workspace_id, p.title, p.description, p.icon, p.status, p.priority, p.lead_type, p.lead_id, p.start_date, p.due_date, (SELECT COUNT(*) FROM issue i WHERE i.project_id = p.id), (SELECT COUNT(*) FROM issue i WHERE i.project_id = p.id AND i.status IN ('done','cancelled')), (SELECT COUNT(*) FROM project_resource r WHERE r.project_id = p.id), p.created_at, p.updated_at";

// ── Project ──
#[tauri::command]
pub fn list_projects(db: State<Db>, workspace_id: String) -> Result<ListProjectsResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM project p WHERE p.workspace_id = ?1 ORDER BY p.created_at", PROJECT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], project_from_row).map_err(|e| e.to_string())?;
    let projects = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = projects.len() as i64;
    Ok(ListProjectsResponse { projects, total })
}

#[tauri::command]
pub fn get_project(db: State<Db>, id: String) -> Result<Option<Project>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM project p WHERE p.id = ?1", PROJECT_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], project_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_project(
    app: tauri::AppHandle, db: State<Db>, workspace_id: String, title: String,
    description: Option<String>, status: Option<String>, priority: Option<String>,
) -> Result<Project, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let p = Project {
        id: new_id(), workspace_id, title, description, icon: None,
        status: status.unwrap_or_else(|| "planned".into()),
        priority: priority.unwrap_or_else(|| "medium".into()),
        lead_type: None, lead_id: None, start_date: None, due_date: None,
        issue_count: 0, done_count: 0, resource_count: 0,
        created_at: now.clone(), updated_at: now,
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO project (id, workspace_id, title, description, status, priority, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
        params![p.id, p.workspace_id, p.title, p.description, p.status, p.priority, p.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "project", &p.id);
    Ok(p)
}

#[tauri::command]
pub fn update_project(
    app: tauri::AppHandle, db: State<Db>, id: String, title: Option<String>,
    description: Option<String>, status: Option<String>, priority: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "title", title);
    push_set(&mut sets, &mut vals, "description", description);
    push_set(&mut sets, &mut vals, "status", status);
    push_set(&mut sets, &mut vals, "priority", priority);
    vals.push(id.clone().into());
    let sql = format!("UPDATE project SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "project", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_project(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM project_resource WHERE project_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("UPDATE issue SET project_id = NULL WHERE project_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM project WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "project", &id);
    Ok(())
}

#[tauri::command]
pub fn list_project_resources(db: State<Db>, project_id: String) -> Result<ListProjectResourcesResponse, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, project_id, kind, title, url, created_at FROM project_resource WHERE project_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([project_id], |r| Ok(ProjectResource {
        id: r.get(0)?, project_id: r.get(1)?, kind: r.get(2)?, title: r.get(3)?, url: r.get(4)?, created_at: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    let resources = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = resources.len() as i64;
    Ok(ListProjectResourcesResponse { resources, total })
}

// ── Squad ──
fn squad_from_row(r: &Row) -> rusqlite::Result<Squad> {
    Ok(Squad {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?,
        description: r.get(3)?, instructions: r.get(4)?, avatar_url: r.get(5)?,
        leader_id: r.get(6)?, creator_id: r.get(7)?, created_at: r.get(8)?,
        updated_at: r.get(9)?, archived_at: r.get(10)?, archived_by: r.get(11)?,
        member_count: r.get(12)?, member_preview: serde_json::json!([]),
    })
}

const SQUAD_COLS: &str = "s.id, s.workspace_id, s.name, s.description, s.instructions, s.avatar_url, s.leader_id, s.creator_id, s.created_at, s.updated_at, s.archived_at, s.archived_by, (SELECT COUNT(*) FROM squad_member m WHERE m.squad_id = s.id)";

#[tauri::command]
pub fn list_squads(db: State<Db>, workspace_id: String) -> Result<Vec<Squad>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM squad s WHERE s.workspace_id = ?1 AND s.archived_at IS NULL ORDER BY s.created_at", SQUAD_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], squad_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_squad(db: State<Db>, id: String) -> Result<Option<Squad>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM squad s WHERE s.id = ?1", SQUAD_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], squad_from_row).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn create_squad(
    app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String,
    description: Option<String>, leader_id: Option<String>,
) -> Result<Squad, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let s = Squad {
        id: new_id(), workspace_id, name, description: description.unwrap_or_default(),
        instructions: String::new(), avatar_url: None,
        leader_id: leader_id.unwrap_or_default(), creator_id: "u-local".into(),
        created_at: now.clone(), updated_at: now, archived_at: None, archived_by: None,
        member_count: 0, member_preview: serde_json::json!([]),
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO squad (id, workspace_id, name, description, instructions, leader_id, creator_id, created_at, updated_at) VALUES (?1,?2,?3,?4,'',?5,?6,?7,?7)",
        params![s.id, s.workspace_id, s.name, s.description, s.leader_id, s.creator_id, s.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &s.id);
    Ok(s)
}

#[tauri::command]
pub fn update_squad(
    app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>,
    description: Option<String>, leader_id: Option<String>, instructions: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "name", name);
    push_set(&mut sets, &mut vals, "description", description);
    push_set(&mut sets, &mut vals, "leader_id", leader_id);
    push_set(&mut sets, &mut vals, "instructions", instructions);
    vals.push(id.clone().into());
    let sql = format!("UPDATE squad SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_squad(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM squad_member WHERE squad_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM squad WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &id);
    Ok(())
}

#[tauri::command]
pub fn list_squad_members(db: State<Db>, squad_id: String) -> Result<Vec<SquadMember>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, squad_id, member_type, member_id, role, created_at FROM squad_member WHERE squad_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([squad_id], |r| Ok(SquadMember {
        id: r.get(0)?, squad_id: r.get(1)?, member_type: r.get(2)?, member_id: r.get(3)?, role: r.get(4)?, created_at: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_squad_member(app: tauri::AppHandle, db: State<Db>, squad_id: String, member_type: String, member_id: String) -> Result<SquadMember, String> {
    let m = SquadMember { id: new_id(), squad_id, member_type, member_id, role: "member".into(), created_at: now_iso() };
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT INTO squad_member (id, squad_id, member_type, member_id, role, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
        params![m.id, m.squad_id, m.member_type, m.member_id, m.role, m.created_at]).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &m.squad_id);
    Ok(m)
}

#[tauri::command]
pub fn remove_squad_member(app: tauri::AppHandle, db: State<Db>, squad_id: String, member_type: String, member_id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM squad_member WHERE squad_id = ?1 AND member_type = ?2 AND member_id = ?3", params![squad_id, member_type, member_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "squad", &squad_id);
    Ok(())
}

#[tauri::command]
pub fn get_squad_member_status(_db: State<Db>, squad_id: String) -> Result<serde_json::Value, String> {
    // 成员在线状态来自 realtime presence；本地版暂无 presence 服务，返回空。
    Ok(serde_json::json!({ "members": [] }))
}

// ── Member ──
#[tauri::command]
pub fn list_members(db: State<Db>, workspace_id: String) -> Result<Vec<Member>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, user_id, name, email, avatar_url, role, created_at FROM member WHERE workspace_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(Member {
        id: r.get(0)?, workspace_id: r.get(1)?, user_id: r.get(2)?, name: r.get(3)?,
        email: r.get(4)?, avatar_url: r.get(5)?, role: r.get(6)?, created_at: r.get(7)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}


// ── Label ──
#[tauri::command]
pub fn list_labels(db: State<Db>, workspace_id: String) -> Result<ListLabelsResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, description, color, created_at, updated_at FROM label WHERE workspace_id = ?1 ORDER BY name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(Label {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?, description: r.get(3)?,
        color: r.get(4)?, created_at: r.get(5)?, updated_at: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    let labels = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = labels.len() as i64;
    Ok(ListLabelsResponse { labels, total })
}

#[tauri::command]
pub fn create_label(app: tauri::AppHandle, db: State<Db>, workspace_id: String, name: String, color: Option<String>) -> Result<Label, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let now = now_iso();
    let l = Label { id: new_id(), workspace_id, name, description: None, color: color.unwrap_or_else(|| "#3b82f6".into()), created_at: now.clone(), updated_at: now };
    let conn = db.0.lock().unwrap();
    conn.execute("INSERT INTO label (id, workspace_id, name, color, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
        params![l.id, l.workspace_id, l.name, l.color, l.created_at]).map_err(|e| e.to_string())?;
    emit_changed(&app, "label", &l.id);
    Ok(l)
}

#[tauri::command]
pub fn update_label(app: tauri::AppHandle, db: State<Db>, id: String, name: Option<String>, color: Option<String>, description: Option<String>) -> Result<Option<Label>, String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "name", name);
    push_set(&mut sets, &mut vals, "color", color);
    push_set(&mut sets, &mut vals, "description", description);
    vals.push(id.clone().into());
    let sql = format!("UPDATE label SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, description, color, created_at, updated_at FROM label WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], |r| Ok(Label {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?, description: r.get(3)?,
        color: r.get(4)?, created_at: r.get(5)?, updated_at: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn delete_label(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM issue_label WHERE label_id = ?1", params![id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM label WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "label", &id);
    Ok(())
}

#[tauri::command]
pub fn list_labels_for_issue(db: State<Db>, issue_id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT l.id, l.workspace_id, l.name, l.description, l.color, l.created_at, l.updated_at FROM label l JOIN issue_label il ON il.label_id = l.id WHERE il.issue_id = ?1 ORDER BY l.name").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], |r| Ok(Label {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?, description: r.get(3)?,
        color: r.get(4)?, created_at: r.get(5)?, updated_at: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    let labels = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "labels": labels }))
}



// ── Issue statuses / table / views / pins / properties ──
fn default_statuses(workspace_id: &str) -> Vec<IssueStatus> {
    let defs = [
        ("backlog", "Backlog", "unstarted", "#94a3b8", 0),
        ("todo", "Todo", "unstarted", "#3b82f6", 1),
        ("in_progress", "In Progress", "started", "#f59e0b", 2),
        ("in_review", "In Review", "started", "#8b5cf6", 3),
        ("blocked", "Blocked", "started", "#ef4444", 4),
        ("done", "Done", "done", "#22c55e", 5),
        ("cancelled", "Cancelled", "closed", "#64748b", 6),
    ];
    defs.iter().map(|(key, name, cat, color, pos)| IssueStatus {
        id: format!("status-{}", key), workspace_id: workspace_id.into(), key: (*key).into(),
        name: (*name).into(), description: String::new(), category: (*cat).into(),
        color: (*color).into(), position: *pos,
    }).collect()
}

#[tauri::command]
pub fn list_issue_statuses(db: State<Db>, workspace_id: String) -> Result<ListIssueStatusesResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM issue_status WHERE workspace_id = ?1",
        params![workspace_id], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    if count == 0 {
        for s in default_statuses(&workspace_id) {
            conn.execute(
                "INSERT INTO issue_status (id, workspace_id, key, name, description, category, color, position) VALUES (?1,?2,?3,?4,'',?5,?6,?7)",
                params![s.id, s.workspace_id, s.key, s.name, s.category, s.color, s.position],
            ).map_err(|e| e.to_string())?;
        }
    }
    let mut stmt = conn.prepare("SELECT id, workspace_id, key, name, description, category, color, position FROM issue_status WHERE workspace_id = ?1 ORDER BY position").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(IssueStatus {
        id: r.get(0)?, workspace_id: r.get(1)?, key: r.get(2)?, name: r.get(3)?,
        description: r.get(4)?, category: r.get(5)?, color: r.get(6)?, position: r.get(7)?,
    })).map_err(|e| e.to_string())?;
    let statuses = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = statuses.len() as i64;
    Ok(ListIssueStatusesResponse {
        statuses,
        categories: vec!["unstarted".into(), "started".into(), "done".into(), "closed".into()],
        total,
    })
}

/// 看板/列表的表行：第一版不解析 query filters（返回全部 issue），
/// 由前端的表模型做展示层过滤；total/branch_total 与行数一致。
#[tauri::command]
pub fn list_issue_table_rows(db: State<Db>, workspace_id: String) -> Result<IssueTableRowsResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let sql = format!(
        "SELECT {}, (SELECT COUNT(*) FROM issue ch WHERE ch.parent_issue_id = i.id) FROM issue i WHERE i.workspace_id = ?1 ORDER BY i.updated_at DESC",
        super::ISSUE_COLS
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| {
        let issue = super::issue_from_row_pub(r)?;
        let child_count: i64 = r.get(25)?;
        Ok(IssueTableRow { issue, direct_child_count: child_count })
    }).map_err(|e| e.to_string())?;
    let rows = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = rows.len() as i64;
    Ok(IssueTableRowsResponse {
        query_fingerprint: "local".into(), group_key: None, parent_id: None,
        total, rows, branch_total: total, next_cursor: None,
    })
}

#[tauri::command]
pub fn list_issue_table_facets(db: State<Db>, workspace_id: String) -> Result<IssueTableFacetsResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM issue WHERE workspace_id = ?1",
        params![workspace_id], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    // facet 桶（状态/优先级/指派）在第一版返回空，过滤面板渲染空态；
    // 看板列本身来自 list_issue_statuses，不依赖 facets。
    Ok(IssueTableFacetsResponse { query_fingerprint: "local".into(), total, facets: vec![] })
}

#[tauri::command]
pub fn list_issue_views(db: State<Db>, workspace_id: String) -> Result<Vec<IssueView>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, scope_type, scope_id, visibility, definition_version, query, created_at, updated_at FROM issue_view WHERE workspace_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(IssueView {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?, scope_type: r.get(3)?,
        scope_id: r.get(4)?, visibility: r.get(5)?, definition_version: r.get(6)?,
        query: super::parse_json_pub(&r.get::<_, String>(7)?, serde_json::json!({})),
        created_at: r.get(8)?, updated_at: r.get(9)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}



#[tauri::command]
pub fn list_pins(db: State<Db>, workspace_id: String) -> Result<Vec<Pin>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, item_type, item_id, created_at FROM pin WHERE workspace_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(Pin {
        id: r.get(0)?, workspace_id: r.get(1)?, item_type: r.get(2)?, item_id: r.get(3)?, created_at: r.get(4)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_properties(db: State<Db>, workspace_id: String) -> Result<ListPropertiesResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, name, type, config, archived, created_at FROM property_def WHERE workspace_id = ?1 AND archived = 0 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(PropertyDef {
        id: r.get(0)?, workspace_id: r.get(1)?, name: r.get(2)?, prop_type: r.get(3)?,
        config: super::parse_json_pub(&r.get::<_, String>(4)?, serde_json::json!({})),
        archived: r.get::<_, i64>(5)? != 0, created_at: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    let properties = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    let total = properties.len() as i64;
    Ok(ListPropertiesResponse { properties, total })
}

#[tauri::command]
pub fn get_child_issue_progress(db: State<Db>, workspace_id: String) -> Result<ChildIssueProgressResponse, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT parent_issue_id, COUNT(*), SUM(CASE WHEN status IN ('done','cancelled') THEN 1 ELSE 0 END) FROM issue WHERE workspace_id = ?1 AND parent_issue_id IS NOT NULL GROUP BY parent_issue_id",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(ChildIssueProgress {
        parent_issue_id: r.get(0)?, total: r.get(1)?, done: r.get(2)?,
    })).map_err(|e| e.to_string())?;
    let progress = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(ChildIssueProgressResponse { progress })
}

#[tauri::command]
pub fn list_child_issues(db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM issue i WHERE i.parent_issue_id = ?1 ORDER BY i.created_at", super::ISSUE_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([id], super::issue_from_row_pub).map_err(|e| e.to_string())?;
    let issues = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "issues": issues }))
}

// ── Wakeup（core IssueWakeup 全字段；system wakeup 规则本地版暂不实现，返回 []） ──
fn wakeup_from_row(r: &Row) -> rusqlite::Result<Wakeup> {
    Ok(Wakeup {
        id: r.get(0)?, issue_id: r.get(1)?, agent_id: r.get(2)?, agent_name: r.get(3)?,
        instruction: r.get(4)?, kind: r.get(5)?, mode: r.get(6)?,
        event_types: super::parse_json_pub(&r.get::<_, String>(7)?, serde_json::json!([])),
        filter_agent_id: r.get(8)?, filter_task_id: r.get(9)?,
        filter_agent_name: r.get(10)?, filter_actor_type: r.get(11)?,
        filter_actor_id: r.get(12)?, filter_actor_name: r.get(13)?,
        interval_seconds: r.get(14)?, cron_expression: r.get(15)?,
        timezone: r.get(16)?, next_fire_at: r.get(17)?,
        enabled: r.get::<_, i64>(18)? != 0, disabled_at: r.get(19)?,
        last_task_id: r.get(20)?, last_error: r.get(21)?, last_task_status: None,
        expires_at: r.get(22)?, expiry_seconds: r.get(23)?, on_timeout: r.get(24)?,
        timed_out_at: r.get(25)?, created_by_agent: false, created_by_name: r.get(26)?,
        source_agent_id: r.get(27)?, source_agent_name: r.get(28)?,
        condition: r.get::<_, Option<String>>(29)?.and_then(|s| serde_json::from_str(&s).ok()),
        max_fires: r.get(30)?, fire_count: r.get(31)?,
        paused_reason: r.get(32)?, origin: r.get(33)?,
        paused: r.get::<_, i64>(34)? != 0, run_at: r.get(35)?, status: r.get(36)?,
        created_at: r.get(37)?, updated_at: r.get::<_, Option<String>>(38)?.unwrap_or_default(),
    })
}

const WAKEUP_COLS: &str = "id, issue_id, agent_id, agent_name, instruction, kind, mode, event_types, filter_agent_id, filter_task_id, filter_agent_name, filter_actor_type, filter_actor_id, filter_actor_name, interval_seconds, cron_expression, timezone, next_fire_at, enabled, disabled_at, last_task_id, last_error, expires_at, expiry_seconds, on_timeout, timed_out_at, created_by_name, source_agent_id, source_agent_name, condition, max_fires, fire_count, paused_reason, origin, paused, run_at, status, created_at, updated_at";

#[tauri::command]
pub fn list_issue_wakeups(db: State<Db>, issue_id: String) -> Result<Vec<Wakeup>, String> {
    list_wakeups_by_issue(db, issue_id, "user")
}



fn list_wakeups_by_issue(db: State<Db>, issue_id: String, origin: &str) -> Result<Vec<Wakeup>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM wakeup WHERE issue_id = ?1 AND origin = ?2 ORDER BY created_at", WAKEUP_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![issue_id, origin], wakeup_from_row).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_issue_wakeup(
    app: tauri::AppHandle, db: State<Db>, issue_id: String, agent_id: String,
    instruction: String, kind: Option<String>, mode: Option<String>,
    event_types: Option<String>, filter_agent_id: Option<String>, filter_task_id: Option<String>,
    filter_actor_type: Option<String>, filter_actor_id: Option<String>,
    at: Option<String>, interval_seconds: Option<i64>, cron_expression: Option<String>,
    timezone: Option<String>, expires_at: Option<String>, condition: Option<String>,
) -> Result<Wakeup, String> {
    let now = now_iso();
    let agent_name = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT name FROM agent WHERE id = ?1", params![agent_id], |r| r.get::<_, String>(0)).unwrap_or_default()
    };
    let w = Wakeup {
        id: new_id(), issue_id, agent_id, agent_name, instruction,
        kind: kind.unwrap_or_else(|| "at".into()), mode: mode.unwrap_or_else(|| "once".into()),
        event_types: super::parse_json_pub(&event_types.unwrap_or_else(|| "[]".into()), serde_json::json!([])),
        filter_agent_id, filter_task_id, filter_agent_name: None,
        filter_actor_type, filter_actor_id, filter_actor_name: None,
        interval_seconds, cron_expression,
        timezone: timezone.unwrap_or_else(|| "Asia/Shanghai".into()),
        next_fire_at: at.clone(), enabled: true, disabled_at: None,
        last_task_id: None, last_error: None, last_task_status: None,
        expires_at, expiry_seconds: None, on_timeout: None, timed_out_at: None,
        created_by_agent: false, created_by_name: None, source_agent_id: None, source_agent_name: None,
        condition: condition.and_then(|s| serde_json::from_str(&s).ok()),
        max_fires: None, fire_count: 0, paused_reason: None, origin: "user".into(),
        paused: false, run_at: at, status: "pending".into(), created_at: now.clone(), updated_at: now,
    };
    let conn = db.0.lock().unwrap();
    conn.execute(
        "INSERT INTO wakeup (id, issue_id, agent_id, agent_name, instruction, kind, mode, event_types, filter_agent_id, filter_task_id, filter_actor_type, filter_actor_id, interval_seconds, cron_expression, timezone, next_fire_at, enabled, expires_at, condition, origin, paused, run_at, status, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,1,?17,?18,?19,0,?20,?21,?22,?22)",
        params![w.id, w.issue_id, w.agent_id, w.agent_name, w.instruction, w.kind, w.mode,
            serde_json::to_string(&w.event_types).unwrap_or_else(|_| "[]".into()),
            w.filter_agent_id, w.filter_task_id, w.filter_actor_type, w.filter_actor_id,
            w.interval_seconds, w.cron_expression, w.timezone, w.next_fire_at, w.expires_at,
            w.condition.as_ref().map(|c| serde_json::to_string(c).unwrap_or_default()),
            w.origin, w.run_at, w.status, w.created_at],
    ).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &w.id);
    Ok(w)
}

#[tauri::command]
pub fn update_issue_wakeup(
    app: tauri::AppHandle, db: State<Db>, id: String, instruction: Option<String>,
    paused: Option<bool>, status: Option<String>,
) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    let mut sets: Vec<String> = vec!["updated_at = ?".into()];
    let mut vals: Vec<rusqlite::types::Value> = vec![now_iso().into()];
    push_set(&mut sets, &mut vals, "instruction", instruction);
    push_set(&mut sets, &mut vals, "status", status);
    if let Some(p) = paused {
        sets.push("paused = ?".into());
        vals.push(if p { 1 } else { 0 }.into());
    }
    vals.push(id.clone().into());
    let sql = format!("UPDATE wakeup SET {} WHERE id = ?", sets.join(", "));
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();
    conn.execute(&sql, params_ref.as_slice()).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &id);
    Ok(())
}

#[tauri::command]
pub fn delete_issue_wakeup(app: tauri::AppHandle, db: State<Db>, id: String) -> Result<(), String> {
    let conn = db.0.lock().unwrap();
    conn.execute("DELETE FROM wakeup WHERE id = ?1", params![id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &id);
    Ok(())
}

/// 立即触发 wakeup：按 wakeup 的 agent + instruction spawn 一次 run。
#[tauri::command]
pub fn trigger_issue_wakeup(app: tauri::AppHandle, db: State<Db>, issue_id: String, wakeup_id: String) -> Result<(), String> {
    let (agent_id, instruction) = {
        let conn = db.0.lock().unwrap();
        conn.query_row(
            "SELECT agent_id, instruction FROM wakeup WHERE id = ?1",
            params![wakeup_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        ).map_err(|e| e.to_string())?
    };
    let (agent_name, cli) = {
        let conn = db.0.lock().unwrap();
        conn.query_row("SELECT name, cli FROM agent WHERE id = ?1", params![agent_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        }).map_err(|e| e.to_string())?
    };
    let _ = super::spawn_run(&app, &db, &issue_id, &agent_id, &agent_name, &cli, &instruction);
    let conn = db.0.lock().unwrap();
    conn.execute("UPDATE wakeup SET fire_count = fire_count + 1, updated_at = ?1 WHERE id = ?2", params![now_iso(), wakeup_id]).map_err(|e| e.to_string())?;
    emit_changed(&app, "wakeup", &wakeup_id);
    Ok(())
}

#[tauri::command]
pub fn list_paused_wakeups(_db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    // 系统暂停的 wakeup（max_fires/loop/rate）本地版不产生：返回 []。
    Ok(vec![])
}

#[tauri::command]
pub fn list_issue_wakeup_summaries(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let sql = format!("SELECT {} FROM wakeup ORDER BY issue_id, created_at", WAKEUP_COLS);
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], wakeup_from_row).map_err(|e| e.to_string())?;
    let wakeups = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    // 按 issue 分组：取最新一条的 preview 字段 + active/event 计数。
    let mut out: Vec<serde_json::Value> = vec![];
    let mut cur: Option<String> = None;
    let mut active = 0i64;
    let mut events = 0i64;
    let mut last: Option<Wakeup> = None;
    for w in wakeups {
        if cur.as_deref() != Some(w.issue_id.as_str()) {
            if let Some(prev) = last.take() {
                out.push(serde_json::json!({
                    "id": prev.id, "issue_id": prev.issue_id, "agent_id": prev.agent_id, "agent_name": prev.agent_name,
                    "kind": prev.kind, "mode": prev.mode, "event_types": prev.event_types,
                    "filter_task_id": prev.filter_task_id, "filter_agent_name": prev.filter_agent_name,
                    "filter_actor_type": prev.filter_actor_type, "filter_actor_id": prev.filter_actor_id,
                    "filter_actor_name": prev.filter_actor_name, "interval_seconds": prev.interval_seconds,
                    "cron_expression": prev.cron_expression, "timezone": prev.timezone,
                    "next_fire_at": prev.next_fire_at, "condition": prev.condition,
                    "active_count": active, "event_count": events,
                }));
            }
            cur = Some(w.issue_id.clone());
            active = 0;
            events = 0;
        }
        if w.enabled { active += 1; }
        if w.kind == "event" { events += 1; }
        last = Some(w);
    }
    if let Some(prev) = last {
        out.push(serde_json::json!({
            "id": prev.id, "issue_id": prev.issue_id, "agent_id": prev.agent_id, "agent_name": prev.agent_name,
            "kind": prev.kind, "mode": prev.mode, "event_types": prev.event_types,
            "filter_task_id": prev.filter_task_id, "filter_agent_name": prev.filter_agent_name,
            "filter_actor_type": prev.filter_actor_type, "filter_actor_id": prev.filter_actor_id,
            "filter_actor_name": prev.filter_actor_name, "interval_seconds": prev.interval_seconds,
            "cron_expression": prev.cron_expression, "timezone": prev.timezone,
            "next_fire_at": prev.next_fire_at, "condition": prev.condition,
            "active_count": active, "event_count": events,
        }));
    }
    Ok(out)
}

// ── Inbox / invitations ──
#[tauri::command]
pub fn list_inbox(db: State<Db>, workspace_id: String) -> Result<Vec<InboxItem>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, recipient_type, recipient_id, actor_type, actor_id, type, severity, issue_id, title, body, issue_status, created_at, read_at FROM inbox_item WHERE workspace_id = ?1 AND archived_at IS NULL ORDER BY created_at DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| Ok(InboxItem {
        id: r.get(0)?, workspace_id: r.get(1)?, recipient_type: r.get(2)?, recipient_id: r.get(3)?,
        actor_type: r.get(4)?, actor_id: r.get(5)?, item_type: r.get(6)?, severity: r.get(7)?,
        issue_id: r.get(8)?, title: r.get(9)?, body: r.get(10)?, issue_status: r.get(11)?,
        created_at: r.get(12)?, read_at: r.get(13)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_inbox_unread_summary(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM inbox_item WHERE archived_at IS NULL", [], |r| r.get(0)).unwrap_or(0);
    let unread: i64 = conn.query_row("SELECT COUNT(*) FROM inbox_item WHERE archived_at IS NULL AND read_at IS NULL", [], |r| r.get(0)).unwrap_or(0);
    let archived: i64 = conn.query_row("SELECT COUNT(*) FROM inbox_item WHERE archived_at IS NOT NULL", [], |r| r.get(0)).unwrap_or(0);
    Ok(vec![serde_json::json!({ "total_count": total, "unread_count": unread, "archived_count": archived })])
}

#[tauri::command]
pub fn list_my_invitations(_db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    // SaaS 邀请已删除：本地版无邀请流程。
    Ok(vec![])
}

// ── Chat / agent builder ──
#[tauri::command]
pub fn list_chat_sessions(db: State<Db>, workspace_id: String) -> Result<Vec<serde_json::Value>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM chat_session WHERE workspace_id = ?1 ORDER BY updated_at DESC").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([workspace_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    let mut out = vec![];
    for id in ids {
        let row = conn.query_row(
            "SELECT id, workspace_id, agent_id, creator_id, project_id, title, status, has_unread, pinned, archived, kind, runtime_id, model, created_at, updated_at FROM chat_session WHERE id = ?1",
            params![id],
            |r| Ok(serde_json::json!({
                "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?,
                "agent_id": r.get::<_, String>(2)?, "creator_id": r.get::<_, String>(3)?,
                "project_id": r.get::<_, Option<String>>(4)?, "title": r.get::<_, String>(5)?,
                "status": r.get::<_, String>(6)?, "has_unread": r.get::<_, i64>(7)? != 0,
                "pinned": r.get::<_, i64>(8)? != 0, "archived": r.get::<_, i64>(9)? != 0,
                "kind": r.get::<_, String>(10)?, "runtime_id": r.get::<_, Option<String>>(11)?,
                "model": r.get::<_, Option<String>>(12)?,
                "created_at": r.get::<_, String>(13)?, "updated_at": r.get::<_, String>(14)?,
            })),
        ).map_err(|e| e.to_string())?;
        let mut v = row;
        let last = conn.query_row(
            "SELECT content, role, created_at FROM chat_message WHERE chat_session_id = ?1 ORDER BY created_at DESC LIMIT 1",
            params![id],
            |r| Ok(serde_json::json!({ "content": r.get::<_, String>(0)?, "role": r.get::<_, String>(1)?, "created_at": r.get::<_, String>(2)? })),
        ).ok();
        v["last_message"] = serde_json::json!(last);
        out.push(v);
    }
    Ok(out)
}

#[tauri::command]
pub fn list_chat_messages(db: State<Db>, session_id: String) -> Result<Vec<ChatMessage>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, chat_session_id, role, content, task_id, attachments, created_at FROM chat_message WHERE chat_session_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([session_id], |r| Ok(ChatMessage {
        id: r.get(0)?, chat_session_id: r.get(1)?, role: r.get(2)?, content: r.get(3)?,
        task_id: r.get(4)?, attachments: super::parse_json_pub(&r.get::<_, String>(5)?, serde_json::json!([])), created_at: r.get(6)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}













// ── Search index（本地全文索引同步：manifest + snapshot） ──
#[tauri::command]
pub fn get_search_index_manifest(db: State<Db>, workspace_id: String) -> Result<serde_json::Value, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let issue_count: i64 = conn.query_row("SELECT COUNT(*) FROM issue WHERE workspace_id = ?1", params![workspace_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    let comment_count: i64 = conn.query_row("SELECT COUNT(*) FROM comment WHERE issue_id IN (SELECT id FROM issue WHERE workspace_id = ?1)", params![workspace_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    let project_count: i64 = conn.query_row("SELECT COUNT(*) FROM project WHERE workspace_id = ?1", params![workspace_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "cursor": format!("local-{}-{}-{}", issue_count, comment_count, project_count),
        "issue_count": issue_count, "comment_count": comment_count,
        "project_count": project_count, "text_bytes": 0,
    }))
}



// ── Attachment / timeline / tasks ──
#[tauri::command]
pub fn list_attachments(db: State<Db>, issue_id: String) -> Result<Vec<Attachment>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, issue_id, filename, mime_type, size, created_at FROM attachment WHERE issue_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], |r| Ok(Attachment {
        id: r.get(0)?, issue_id: r.get(1)?, filename: r.get(2)?, mime_type: r.get(3)?, size: r.get(4)?, created_at: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_attachment(db: State<Db>, id: String) -> Result<Option<Attachment>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, issue_id, filename, mime_type, size, created_at FROM attachment WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([id], |r| Ok(Attachment {
        id: r.get(0)?, issue_id: r.get(1)?, filename: r.get(2)?, mime_type: r.get(3)?, size: r.get(4)?, created_at: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

#[tauri::command]
pub fn list_timeline(db: State<Db>, issue_id: String) -> Result<Vec<TimelineEntry>, String> {
    // 时间线 = 评论（type=comment）。activity 类条目由服务端事件产生，本地版暂无。
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT c.id, c.issue_id, c.author_id, COALESCE(m.name, c.author_id), c.content, c.created_at FROM comment c JOIN issue i ON i.id = c.issue_id LEFT JOIN member m ON m.user_id = c.author_id AND m.workspace_id = i.workspace_id WHERE c.issue_id = ?1 ORDER BY c.created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], |r| Ok(TimelineEntry {
        id: r.get(0)?, entry_type: "comment".into(), actor_type: "member".into(),
        actor_id: r.get(2)?, actor_name: r.get(3)?, content: r.get(4)?,
        issue_id: r.get(1)?, created_at: r.get(5)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn run_to_task(r: &Row) -> rusqlite::Result<AgentTask> {
    let status: String = r.get(4)?;
    let status = match status.as_str() {
        "succeeded" => "completed".to_string(),
        "queued" => "queued".to_string(),
        "running" => "running".to_string(),
        "failed" => "failed".to_string(),
        "cancelled" => "cancelled".to_string(),
        other => other.to_string(),
    };
    Ok(AgentTask {
        id: r.get(0)?, agent_id: r.get(2)?, runtime_id: String::new(), issue_id: r.get(1)?,
        status, priority: 0, dispatched_at: None,
        started_at: r.get(5)?, finished_at: r.get(6)?,
    })
}

#[tauri::command]
pub fn list_tasks_by_issue(db: State<Db>, issue_id: String) -> Result<Vec<AgentTask>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, COALESCE(issue_id, ''), agent_id, agent_name, status, started_at, finished_at, log FROM run WHERE issue_id = ?1 ORDER BY started_at DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([issue_id], run_to_task).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_active_tasks_for_issue(db: State<Db>, issue_id: String) -> Result<serde_json::Value, String> {
    let tasks = list_tasks_by_issue(db, issue_id)?;
    let active: Vec<AgentTask> = tasks.into_iter().filter(|t| matches!(t.status.as_str(), "queued" | "dispatched" | "running")).collect();
    Ok(serde_json::json!({ "tasks": active }))
}

#[tauri::command]
pub fn list_agent_tasks_page(db: State<Db>, agent_id: String) -> Result<AgentTasksPageResponse, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, COALESCE(issue_id, ''), agent_id, agent_name, status, started_at, finished_at, log FROM run WHERE agent_id = ?1 ORDER BY started_at DESC LIMIT 200").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([agent_id], run_to_task).map_err(|e| e.to_string())?;
    let tasks = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
    Ok(AgentTasksPageResponse { tasks, next_cursor: None })
}

#[tauri::command]
pub fn get_agent_task_snapshot(db: State<Db>) -> Result<Vec<AgentTask>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, COALESCE(issue_id, ''), agent_id, agent_name, status, started_at, finished_at, log FROM run WHERE status IN ('queued','running') ORDER BY started_at DESC").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], run_to_task).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_task(app: tauri::AppHandle, db: State<Db>, task_id: String) -> Result<Option<AgentTask>, String> {
    super::cancel_run_by_id(&app, &db, &task_id)?;
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, COALESCE(issue_id, ''), agent_id, agent_name, status, started_at, finished_at, log FROM run WHERE id = ?1").map_err(|e| e.to_string())?;
    let mut rows = stmt.query_map([task_id], run_to_task).map_err(|e| e.to_string())?;
    match rows.next() { Some(r) => Ok(Some(r.map_err(|e| e.to_string())?)), None => Ok(None) }
}

// ── Workspace working agents / activity / run counts（Issues 页头 + Agents 页） ──
#[tauri::command]
pub fn get_workspace_working_agents(db: State<Db>, workspace_id: String) -> Result<Vec<serde_json::Value>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.avatar_url, COUNT(r.id), GROUP_CONCAT(DISTINCT r.issue_id) FROM agent a JOIN run r ON r.agent_id = a.id WHERE a.workspace_id = ?1 AND r.status = 'running' GROUP BY a.id, a.name, a.avatar_url",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| {
        let ids: Option<String> = r.get(4)?;
        Ok(serde_json::json!({
            "id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?,
            "avatar_url": r.get::<_, Option<String>>(2)?,
            "running_task_count": r.get::<_, i64>(3)?,
            "issue_ids": ids.map(|s| s.split(',').map(|x| x.to_string()).collect::<Vec<_>>()).unwrap_or_default(),
        }))
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workspace_agent_activity_30d(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT agent_id, substr(started_at, 1, 10), COUNT(*), SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END), SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END), SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) FROM run WHERE started_at >= datetime('now', '-30 days') GROUP BY agent_id, substr(started_at, 1, 10) ORDER BY 2",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| {
        Ok(serde_json::json!({
            "agent_id": r.get::<_, String>(0)?, "bucket_at": r.get::<_, String>(1)?,
            "task_count": r.get::<_, i64>(2)?, "failed_count": r.get::<_, i64>(3)?,
            "completed_count": r.get::<_, i64>(4)?, "cancelled_count": r.get::<_, i64>(5)?,
        }))
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_workspace_agent_run_counts(db: State<Db>) -> Result<Vec<serde_json::Value>, String> {
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT agent_id, COUNT(*) FROM run GROUP BY agent_id ORDER BY 2 DESC",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| {
        Ok(serde_json::json!({ "agent_id": r.get::<_, String>(0)?, "run_count": r.get::<_, i64>(1)? }))
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_agent_tasks(app: tauri::AppHandle, db: State<Db>, agent_id: String) -> Result<serde_json::Value, String> {
    let ids: Vec<String> = {
        let conn = db.0.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id FROM run WHERE agent_id = ?1 AND status = 'running'").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([agent_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?
    };
    let cancelled = ids.len() as i64;
    for id in ids {
        let _ = super::cancel_run_by_id(&app, &db, &id);
    }
    Ok(serde_json::json!({ "cancelled": cancelled }))
}

// ── Runtime profiles / channel installations / quick actions / misc ──
#[tauri::command]
pub fn list_runtime_profiles(db: State<Db>, workspace_id: String) -> Result<Vec<serde_json::Value>, String> {
    let ws = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare("SELECT id, workspace_id, display_name, protocol_family, runtime_type, command_name, description, fixed_args, visibility, created_by, enabled, created_at, updated_at FROM runtime_profile WHERE workspace_id = ?1 ORDER BY created_at").map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![ws], |r| Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?, "workspace_id": r.get::<_, String>(1)?,
        "display_name": r.get::<_, String>(2)?, "protocol_family": r.get::<_, String>(3)?,
        "runtime_type": r.get::<_, Option<String>>(4)?, "command_name": r.get::<_, String>(5)?,
        "description": r.get::<_, Option<String>>(6)?, "fixed_args": super::parse_json_pub(&r.get::<_, String>(7)?, serde_json::json!([])),
        "visibility": r.get::<_, String>(8)?, "created_by": r.get::<_, Option<String>>(9)?,
        "enabled": r.get::<_, i64>(10)? != 0, "created_at": r.get::<_, String>(11)?, "updated_at": r.get::<_, String>(12)?,
    }))).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn unconfigured_installations() -> serde_json::Value {
    // 频道安装（Lark/Slack/钉钉/企微/Telegram）依赖云端密钥；本地版 configured=false。
    serde_json::json!({ "installations": [], "configured": false })
}









#[tauri::command]
pub fn get_assignee_frequency(db: State<Db>, workspace_id: String) -> Result<Vec<serde_json::Value>, String> {
    let workspace_id = { let conn = db.0.lock().unwrap(); super::resolve_workspace_id(&conn, &workspace_id) };
    let conn = db.0.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT assignee_type, assignee_id, assignee_name, COUNT(*) FROM issue WHERE workspace_id = ?1 AND assignee_id IS NOT NULL GROUP BY assignee_type, assignee_id, assignee_name ORDER BY 4 DESC",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([workspace_id], |r| {
        Ok(serde_json::json!({
            "assignee_type": r.get::<_, Option<String>>(0)?,
            "assignee_id": r.get::<_, Option<String>>(1)?,
            "assignee_name": r.get::<_, Option<String>>(2)?,
            "frequency": r.get::<_, i64>(3)?,
        }))
    }).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_issue_duplicates(_db: State<Db>, id: String) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "duplicate_of": null, "duplicates": [] }))
}

#[tauri::command]
pub fn list_issue_pull_requests(_db: State<Db>, issue_id: String) -> Result<serde_json::Value, String> {
    // PR 关联来自 GitHub App；本地版不接 GitHub 云端，返回空。
    Ok(serde_json::json!({ "pull_requests": [], "auto_complete": null }))
}



// ── Dashboard（用量统计：本地版暂不统计，返回空序列，页面渲染 No usage） ──







#[tauri::command]
pub fn list_lark_installations(db: State<Db>) -> Result<serde_json::Value, String> {
    super::write2::channel_installations(&db, "lark")
}

#[tauri::command]
pub fn list_slack_installations(db: State<Db>) -> Result<serde_json::Value, String> {
    super::write2::channel_installations(&db, "slack")
}

#[tauri::command]
pub fn list_dingtalk_installations(db: State<Db>) -> Result<serde_json::Value, String> {
    super::write2::channel_installations(&db, "dingtalk")
}

#[tauri::command]
pub fn list_wecom_installations(db: State<Db>) -> Result<serde_json::Value, String> {
    super::write2::channel_installations(&db, "wecom")
}

#[tauri::command]
pub fn list_telegram_installations(db: State<Db>) -> Result<serde_json::Value, String> {
    super::write2::channel_installations(&db, "telegram")
}
