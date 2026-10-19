-- 007：Batch 3 缺失写路径所需表/列。
-- MCP 服务器库（workspace 级）+ agent 绑定（per-agent 开关）。
CREATE TABLE IF NOT EXISTS workspace_mcp_server (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  config TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_mcp_server (
  agent_id TEXT NOT NULL REFERENCES agent(id),
  server_id TEXT NOT NULL REFERENCES workspace_mcp_server(id),
  enabled INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (agent_id, server_id)
);
-- 自定义运行时 profile（MUL-3284）：workspace 级 CLI 预设。
CREATE TABLE IF NOT EXISTS runtime_profile (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  display_name TEXT NOT NULL,
  protocol_family TEXT NOT NULL DEFAULT 'claude',
  runtime_type TEXT,
  command_name TEXT NOT NULL,
  description TEXT,
  fixed_args TEXT NOT NULL DEFAULT '[]',
  visibility TEXT NOT NULL DEFAULT 'workspace',
  created_by TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
-- 运行时异步任务（models / local-skills / import-local-skill）：
-- 本地版同步完成，仍按 initiate -> get 的轮询契约返回，payload 存完整结果。
CREATE TABLE IF NOT EXISTS runtime_job (
  id TEXT PRIMARY KEY,
  runtime_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'completed',
  payload TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
-- workspace 扩展设置（updateWorkspace 的 description/context/settings/repos/issue_prefix/avatar_url
-- 与运行时自定义名）：workspace 表只有 id/slug/name 三列，多余字段存 JSON。
-- 注意：SQLite 的 ALTER ADD COLUMN 没有 IF NOT EXISTS，settings 列由 db.rs 的
-- has_column 守卫单独添加（本文件只含幂等的 CREATE TABLE IF NOT EXISTS），
-- 否则 partial 迁移 + 版本未记录会在每次启动重试 ALTER 时失败。
