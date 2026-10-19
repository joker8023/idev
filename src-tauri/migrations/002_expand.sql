-- 002：扩展表（原 Postgres 的本地子集）。issue/agent/skill 补齐前端详情页直接读的字段；
-- 新增 project / squad / label / pin / view / property / wakeup / inbox / chat /
-- attachment / member / status / autopilot_run 等表，覆盖 26 路由的读路径。

-- ── issue 扩展列 ──
ALTER TABLE issue ADD COLUMN identifier TEXT NOT NULL DEFAULT '';
ALTER TABLE issue ADD COLUMN number INTEGER NOT NULL DEFAULT 0;
ALTER TABLE issue ADD COLUMN description TEXT;
ALTER TABLE issue ADD COLUMN creator_type TEXT;
ALTER TABLE issue ADD COLUMN creator_id TEXT;
ALTER TABLE issue ADD COLUMN parent_issue_id TEXT;
ALTER TABLE issue ADD COLUMN project_id TEXT;
ALTER TABLE issue ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
ALTER TABLE issue ADD COLUMN stage TEXT;
ALTER TABLE issue ADD COLUMN start_date TEXT;
ALTER TABLE issue ADD COLUMN due_date TEXT;
ALTER TABLE issue ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}';
ALTER TABLE issue ADD COLUMN properties TEXT NOT NULL DEFAULT '{}';
ALTER TABLE issue ADD COLUMN status_category TEXT NOT NULL DEFAULT 'unstarted';

-- ── agent 扩展列 ──
ALTER TABLE agent ADD COLUMN owner_id TEXT;
ALTER TABLE agent ADD COLUMN description TEXT NOT NULL DEFAULT '';
ALTER TABLE agent ADD COLUMN instructions TEXT NOT NULL DEFAULT '';
ALTER TABLE agent ADD COLUMN avatar_url TEXT;
ALTER TABLE agent ADD COLUMN skills TEXT NOT NULL DEFAULT '[]';
ALTER TABLE agent ADD COLUMN runtime_mode TEXT NOT NULL DEFAULT 'local';
ALTER TABLE agent ADD COLUMN runtime_config TEXT NOT NULL DEFAULT '{}';
ALTER TABLE agent ADD COLUMN custom_args TEXT NOT NULL DEFAULT '[]';
ALTER TABLE agent ADD COLUMN model TEXT NOT NULL DEFAULT '';
ALTER TABLE agent ADD COLUMN visibility TEXT NOT NULL DEFAULT 'workspace';
ALTER TABLE agent ADD COLUMN max_concurrent_tasks INTEGER NOT NULL DEFAULT 1;
ALTER TABLE agent ADD COLUMN archived_at TEXT;
ALTER TABLE agent ADD COLUMN updated_at TEXT;

-- ── skill 扩展列 ──
ALTER TABLE skill ADD COLUMN config TEXT NOT NULL DEFAULT '{}';
ALTER TABLE skill ADD COLUMN created_by TEXT;
ALTER TABLE skill ADD COLUMN created_at TEXT;
ALTER TABLE skill ADD COLUMN updated_at TEXT;
ALTER TABLE skill ADD COLUMN archived_at TEXT;

-- ── autopilot：按 core Autopilot 类型重建（旧的 name/cron/agent_id 语义不对） ──
DROP TABLE IF EXISTS autopilot;
CREATE TABLE IF NOT EXISTS autopilot (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  title TEXT NOT NULL,
  description TEXT,
  project_id TEXT,
  assignee_type TEXT NOT NULL DEFAULT 'agent',
  assignee_id TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  execution_mode TEXT NOT NULL DEFAULT 'create_issue',
  issue_title_template TEXT,
  created_by_type TEXT NOT NULL DEFAULT 'member',
  created_by_id TEXT NOT NULL DEFAULT '',
  last_run_at TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS autopilot_run (
  id TEXT PRIMARY KEY,
  autopilot_id TEXT NOT NULL REFERENCES autopilot(id),
  status TEXT NOT NULL DEFAULT 'running',
  issue_id TEXT,
  started_at TEXT NOT NULL,
  finished_at TEXT,
  log TEXT NOT NULL DEFAULT ''
);

-- ── project ──
CREATE TABLE IF NOT EXISTS project (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  title TEXT NOT NULL,
  description TEXT,
  icon TEXT,
  status TEXT NOT NULL DEFAULT 'planned',
  priority TEXT NOT NULL DEFAULT 'medium',
  lead_type TEXT,
  lead_id TEXT,
  start_date TEXT,
  due_date TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_resource (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id),
  kind TEXT NOT NULL DEFAULT 'link',
  title TEXT NOT NULL DEFAULT '',
  url TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- ── squad ──
CREATE TABLE IF NOT EXISTS squad (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  instructions TEXT NOT NULL DEFAULT '',
  avatar_url TEXT,
  leader_id TEXT NOT NULL DEFAULT '',
  creator_id TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  archived_at TEXT,
  archived_by TEXT
);

CREATE TABLE IF NOT EXISTS squad_member (
  id TEXT PRIMARY KEY,
  squad_id TEXT NOT NULL REFERENCES squad(id),
  member_type TEXT NOT NULL,
  member_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'member',
  created_at TEXT NOT NULL
);

-- ── label / issue_label ──
CREATE TABLE IF NOT EXISTS label (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  description TEXT,
  color TEXT NOT NULL DEFAULT '#3b82f6',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_label (
  issue_id TEXT NOT NULL REFERENCES issue(id),
  label_id TEXT NOT NULL REFERENCES label(id),
  PRIMARY KEY (issue_id, label_id)
);

-- ── pin / issue_view / property_def / wakeup ──
CREATE TABLE IF NOT EXISTS pin (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  item_type TEXT NOT NULL,
  item_id TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_view (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  scope_type TEXT NOT NULL DEFAULT 'workspace',
  scope_id TEXT,
  visibility TEXT NOT NULL DEFAULT 'private',
  definition_version INTEGER NOT NULL DEFAULT 1,
  query TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS property_def (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  type TEXT NOT NULL DEFAULT 'text',
  config TEXT NOT NULL DEFAULT '{}',
  archived INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wakeup (
  id TEXT PRIMARY KEY,
  issue_id TEXT NOT NULL REFERENCES issue(id),
  kind TEXT NOT NULL DEFAULT 'user',
  status TEXT NOT NULL DEFAULT 'pending',
  run_at TEXT,
  agent_id TEXT,
  created_at TEXT NOT NULL
);

-- ── inbox / chat / attachment / member / status ──
CREATE TABLE IF NOT EXISTS inbox_item (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  recipient_type TEXT NOT NULL DEFAULT 'member',
  recipient_id TEXT NOT NULL DEFAULT '',
  actor_type TEXT,
  actor_id TEXT,
  type TEXT NOT NULL DEFAULT 'info',
  severity TEXT NOT NULL DEFAULT 'info',
  issue_id TEXT,
  title TEXT NOT NULL DEFAULT '',
  body TEXT,
  issue_status TEXT,
  created_at TEXT NOT NULL,
  read_at TEXT
);

CREATE TABLE IF NOT EXISTS chat_session (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  title TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chat_message (
  id TEXT PRIMARY KEY,
  chat_session_id TEXT NOT NULL REFERENCES chat_session(id),
  role TEXT NOT NULL,
  content TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS attachment (
  id TEXT PRIMARY KEY,
  issue_id TEXT,
  filename TEXT NOT NULL DEFAULT '',
  mime_type TEXT NOT NULL DEFAULT 'application/octet-stream',
  size INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS member (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  user_id TEXT NOT NULL,
  name TEXT NOT NULL DEFAULT '',
  email TEXT NOT NULL DEFAULT '',
  avatar_url TEXT,
  role TEXT NOT NULL DEFAULT 'member',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_status (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  key TEXT NOT NULL,
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  category TEXT NOT NULL DEFAULT 'unstarted',
  color TEXT NOT NULL DEFAULT '#94a3b8',
  position INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_issue_project ON issue(project_id);
CREATE INDEX IF NOT EXISTS idx_issue_parent ON issue(parent_issue_id);
CREATE INDEX IF NOT EXISTS idx_squad_member ON squad_member(squad_id);
CREATE INDEX IF NOT EXISTS idx_issue_label_issue ON issue_label(issue_id);
CREATE INDEX IF NOT EXISTS idx_wakeup_issue ON wakeup(issue_id);
CREATE INDEX IF NOT EXISTS idx_inbox_ws ON inbox_item(workspace_id);
CREATE INDEX IF NOT EXISTS idx_chat_msg_session ON chat_message(chat_session_id);
CREATE INDEX IF NOT EXISTS idx_attachment_issue ON attachment(issue_id);
