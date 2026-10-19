-- Multica-Tauri 本地版：核心表（原 Postgres 001_init.up.sql 的 SQLite 等价子集）
-- 删除：云端/计费/遥测/多租户 SaaS 相关表（billing、entitlement、seatcapacity、
-- instance_telemetry_state、cloud PAT、PostHog、S3/CloudFront 配置）。本地版单机单用户，
-- auth 简化为本地口令（见 auth.rs），不发邮件、不接 Google OAuth、不调 Resend。

CREATE TABLE IF NOT EXISTS workspace (
  id TEXT PRIMARY KEY,
  slug TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  provider TEXT NOT NULL,
  cli TEXT NOT NULL,
  runtime_id TEXT,
  status TEXT NOT NULL DEFAULT 'offline',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'backlog',
  priority TEXT NOT NULL DEFAULT 'medium',
  assignee_type TEXT,
  assignee_id TEXT,
  assignee_name TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS comment (
  id TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL REFERENCES issue(id),
  author_type TEXT NOT NULL DEFAULT 'human',
  author_name TEXT NOT NULL,
  body TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run (
  id TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL REFERENCES issue(id),
  agent_id TEXT NOT NULL,
  agent_name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  started_at TEXT NOT NULL,
  finished_at TEXT,
  log TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS skill (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  content TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS autopilot (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  cron TEXT NOT NULL DEFAULT '0 9 * * *',
  agent_id TEXT,
  enabled INTEGER NOT NULL DEFAULT 1
);

-- 本地口令：SHA-256(password)，首次启动为空（免密进入，设置后才生效）。
-- 原 Go：JWT + 邮箱验证码 + Google OAuth + 90 天 PAT，全部删除。
CREATE TABLE IF NOT EXISTS local_auth (
  singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
  password_hash TEXT
);

CREATE INDEX IF NOT EXISTS idx_issue_workspace ON issue(workspace_id);
CREATE INDEX IF NOT EXISTS idx_issue_status ON issue(status);
CREATE INDEX IF NOT EXISTS idx_comment_issue ON comment(issue_id);
CREATE INDEX IF NOT EXISTS idx_run_issue ON run(issue_id);
