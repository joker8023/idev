use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── 与原 Multica 对齐的核心实体（字段名/类型与 packages/core/types/* 一致，
//    这样 Rust 返回的 JSON 可直接给前端用，不再靠前端 mock 补字段）。
// 原 Go: server/internal/database/*.sqlc.go + migrations/001_init.up.sql
// 新 Rust: SQLite，表结构见 migrations/001_init.sql + 002_expand.sql。UUID 存为 TEXT，
// JSON 列（metadata/properties/skills/runtime_config/custom_args/config）存 TEXT，读出解析为 Value。

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: String,
    pub workspace_id: String,
    /// claude | codex | cursor | copilot | kimi | opencode | ...（与原 26 个 CLI 同名）
    pub provider: String,
    /// agent CLI 二进制名，如 claude / codex
    pub cli: String,
    pub runtime_id: String,
    pub runtime_bound: bool,
    pub runtime_availability: String, // online | unstable | offline
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub avatar_url: Option<String>,
    pub runtime_mode: String, // local | ...
    #[serde(default = "default_json_obj")]
    pub runtime_config: serde_json::Value,
    #[serde(default = "default_json_arr")]
    pub custom_args: serde_json::Value,
    #[serde(default = "default_json_arr")]
    pub skills: serde_json::Value,
    pub owner_id: Option<String>,
    pub model: String,
    pub visibility: String,
    pub max_concurrent_tasks: i64,
    pub status: String, // online | offline | busy
    pub archived_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub workspace_id: String,
    pub number: i64,
    pub identifier: String, // MUL-<number>
    pub title: String,
    pub body: String,
    pub description: Option<String>,
    pub status: String, // backlog | todo | in_progress | in_review | blocked | done | cancelled
    pub status_category: String, // unstarted | started | done | closed
    pub priority: String, // none | low | medium | high | urgent
    pub assignee_type: Option<String>, // human | agent
    pub assignee_id: Option<String>,
    pub assignee_name: Option<String>,
    pub creator_type: Option<String>,
    pub creator_id: Option<String>,
    pub parent_issue_id: Option<String>,
    pub project_id: Option<String>,
    pub position: i64,
    pub stage: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    #[serde(default = "default_json_arr")]
    pub labels: serde_json::Value,
    #[serde(default = "default_json_obj")]
    pub metadata: serde_json::Value,
    #[serde(default = "default_json_obj")]
    pub properties: serde_json::Value,
    pub comments_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub issue_id: String,
    pub author_type: String, // member | agent | system
    pub author_id: String,
    pub content: String,
    #[serde(rename = "type")]
    pub comment_type: String, // comment | ...
    pub parent_id: Option<String>,
    #[serde(default = "default_json_arr")]
    pub reactions: serde_json::Value,
    #[serde(default = "default_json_arr")]
    pub attachments: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
    pub resolved_by_type: Option<String>,
    pub resolved_by_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub issue_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub status: String, // queued | running | succeeded | failed | cancelled
    pub started_at: String,
    pub finished_at: Option<String>,
    pub log: String,
}

/// 前端 AgentTask 形状（run 的投影）：status 用 completed 而不是 succeeded。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    pub id: String,
    pub agent_id: String,
    pub runtime_id: String,
    pub issue_id: String,
    pub status: String, // queued | dispatched | running | completed | failed | cancelled
    pub priority: i64,
    pub dispatched_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Runtime {
    pub id: String,
    pub name: String,
    /// 本机 daemon 即 local；原 daemon 的 WebSocket 注册改为 Tauri 进程内状态
    pub kind: String, // local
    pub status: String, // online | offline
    pub detected_clis: Vec<String>,
}

/// 自定义运行时 profile（MUL-3284）：与 vendor/core/types/agent.ts RuntimeProfile 对齐。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub id: String,
    pub workspace_id: String,
    pub display_name: String,
    pub protocol_family: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_type: Option<String>,
    pub command_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_json_arr")]
    pub fixed_args: serde_json::Value,
    pub visibility: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default = "default_json_obj")]
    pub config: serde_json::Value,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "default_json_arr")]
    pub files: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Autopilot {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub description: Option<String>,
    pub project_id: Option<String>,
    pub assignee_type: String, // agent | squad
    pub assignee_id: String,
    pub status: String, // active | paused | archived
    pub execution_mode: String, // create_issue | run_only
    pub issue_title_template: Option<String>,
    pub created_by_type: String,
    pub created_by_id: String,
    pub last_run_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutopilotRun {
    pub id: String,
    pub autopilot_id: String,
    pub status: String,
    pub issue_id: Option<String>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub status: String, // planned | in_progress | paused | completed | cancelled
    pub priority: String,
    pub lead_type: Option<String>,
    pub lead_id: Option<String>,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub issue_count: i64,
    pub done_count: i64,
    pub resource_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResource {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub title: String,
    pub url: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Squad {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: String,
    pub instructions: String,
    pub avatar_url: Option<String>,
    pub leader_id: String,
    pub creator_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub archived_by: Option<String>,
    pub member_count: i64,
    #[serde(default = "default_json_arr")]
    pub member_preview: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SquadMember {
    pub id: String,
    pub squad_id: String,
    pub member_type: String, // agent | member
    pub member_id: String,
    pub role: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub workspace_id: String,
    pub user_id: String,
    pub name: String,
    pub email: String,
    pub avatar_url: Option<String>,
    pub role: String, // owner | admin | member
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueStatus {
    pub id: String,
    pub workspace_id: String,
    pub key: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub color: String,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pin {
    pub id: String,
    pub workspace_id: String,
    pub item_type: String,
    pub item_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueView {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub visibility: String,
    pub definition_version: i64,
    #[serde(default = "default_json_obj")]
    pub query: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDef {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub prop_type: String,
    #[serde(default = "default_json_obj")]
    pub config: serde_json::Value,
    pub archived: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wakeup {
    pub id: String,
    pub issue_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub instruction: String,
    pub kind: String, // event | at | every | cron
    pub mode: String, // once | continuous
    #[serde(default = "default_json_arr")]
    pub event_types: serde_json::Value,
    pub filter_agent_id: Option<String>,
    pub filter_task_id: Option<String>,
    pub filter_agent_name: Option<String>,
    pub filter_actor_type: Option<String>,
    pub filter_actor_id: Option<String>,
    pub filter_actor_name: Option<String>,
    pub interval_seconds: Option<i64>,
    pub cron_expression: Option<String>,
    pub timezone: String,
    pub next_fire_at: Option<String>,
    pub enabled: bool,
    pub disabled_at: Option<String>,
    pub last_task_id: Option<String>,
    pub last_error: Option<String>,
    pub last_task_status: Option<String>,
    pub expires_at: Option<String>,
    pub expiry_seconds: Option<i64>,
    pub on_timeout: Option<String>,
    pub timed_out_at: Option<String>,
    pub created_by_agent: bool,
    pub created_by_name: Option<String>,
    pub source_agent_id: Option<String>,
    pub source_agent_name: Option<String>,
    pub condition: Option<serde_json::Value>,
    pub max_fires: Option<i64>,
    pub fire_count: i64,
    pub paused_reason: Option<String>,
    /// 本地扩展：user/system（区分用户 wakeup 与系统 wakeup），不序列化。
    #[serde(skip_serializing)]
    pub origin: String,
    /// 本地扩展：暂停标记（core 用 enabled/disabled_at 表达；保留供查询）。
    #[serde(skip_serializing)]
    pub paused: bool,
    /// 本地扩展：run_at 与 status 是 002 时期的字段，core 无此概念，保留列但不序列化。
    #[serde(skip_serializing)]
    pub run_at: Option<String>,
    #[serde(skip_serializing)]
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub workspace_id: String,
    pub recipient_type: String,
    pub recipient_id: String,
    pub actor_type: Option<String>,
    pub actor_id: Option<String>,
    #[serde(rename = "type")]
    pub item_type: String,
    pub severity: String,
    pub issue_id: Option<String>,
    pub title: String,
    pub body: Option<String>,
    pub issue_status: Option<String>,
    pub created_at: String,
    pub read_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub creator_id: String,
    pub title: String,
    pub status: String, // active | archived
    pub has_unread: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub chat_session_id: String,
    pub role: String,
    pub content: String,
    pub task_id: Option<String>,
    #[serde(default = "default_json_arr")]
    pub attachments: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub issue_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub size: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub entry_type: String, // activity | comment
    pub actor_type: String,
    pub actor_id: String,
    pub actor_name: Option<String>,
    pub content: Option<String>,
    pub issue_id: String,
    pub created_at: String,
}

// ── 响应包装（与 core 类型一致） ──
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTableRow {
    pub issue: Issue,
    pub direct_child_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTableRowsResponse {
    pub query_fingerprint: String,
    pub group_key: Option<String>,
    pub parent_id: Option<String>,
    pub total: i64,
    pub rows: Vec<IssueTableRow>,
    pub branch_total: i64,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueTableFacetsResponse {
    pub query_fingerprint: String,
    pub total: i64,
    pub facets: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAutopilotsResponse {
    pub autopilots: Vec<Autopilot>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetAutopilotResponse {
    pub autopilot: Autopilot,
    pub triggers: Vec<serde_json::Value>,
    pub collaborators: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListAutopilotRunsResponse {
    pub runs: Vec<AutopilotRun>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProjectResourcesResponse {
    pub resources: Vec<ProjectResource>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildIssueProgressResponse {
    pub progress: Vec<ChildIssueProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildIssueProgress {
    pub parent_issue_id: String,
    pub total: i64,
    pub done: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueViewPreference {
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub prefs: serde_json::Value,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPropertiesResponse {
    pub properties: Vec<PropertyDef>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTasksPageResponse {
    pub tasks: Vec<AgentTask>,
    #[serde(rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListProjectsResponse {
    pub projects: Vec<Project>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListLabelsResponse {
    pub labels: Vec<Label>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListIssueStatusesResponse {
    pub statuses: Vec<IssueStatus>,
    pub categories: Vec<String>,
    pub total: i64,
}

fn default_json_obj() -> serde_json::Value {
    serde_json::json!({})
}

fn default_json_arr() -> serde_json::Value {
    serde_json::json!([])
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn parse_json(text: &str, fallback: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or(fallback)
}
