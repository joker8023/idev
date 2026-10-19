-- 005：写路径补齐所需的表与列。
-- 新表：issue_property_value / reaction / issue_subscriber / quick_action / agent_env /
-- builder_draft / autopilot_trigger / autopilot_delivery / system_wakeup / wakeup_run /
-- notification_preferences / channel_installation / issue_view_preference。
-- 新列：run.chat_session_id、chat_session 扩展列、attachment.path、
-- inbox_item.archived_at、pin.position、issue_view 扩展列。

CREATE TABLE IF NOT EXISTS issue_property_value (
  issue_id TEXT NOT NULL REFERENCES issue(id),
  property_id TEXT NOT NULL REFERENCES property_def(id),
  value TEXT NOT NULL DEFAULT '',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (issue_id, property_id)
);

CREATE TABLE IF NOT EXISTS reaction (
  id TEXT PRIMARY KEY,
  target_type TEXT NOT NULL, -- comment | issue
  target_id TEXT NOT NULL,
  emoji TEXT NOT NULL,
  user_id TEXT NOT NULL DEFAULT 'u-local',
  created_at TEXT NOT NULL,
  UNIQUE (target_type, target_id, emoji, user_id)
);

CREATE TABLE IF NOT EXISTS issue_subscriber (
  issue_id TEXT NOT NULL REFERENCES issue(id),
  user_id TEXT NOT NULL,
  user_type TEXT NOT NULL DEFAULT 'member',
  created_at TEXT NOT NULL,
  PRIMARY KEY (issue_id, user_id, user_type)
);

CREATE TABLE IF NOT EXISTS quick_action (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  name TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  assignee_type TEXT NOT NULL DEFAULT 'agent',
  assignee_id TEXT NOT NULL DEFAULT '',
  prompt TEXT NOT NULL DEFAULT '',
  visibility TEXT NOT NULL DEFAULT 'workspace',
  status TEXT NOT NULL DEFAULT 'active',
  last_used_at TEXT,
  use_count INTEGER NOT NULL DEFAULT 0,
  created_by_id TEXT NOT NULL DEFAULT 'u-local',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_env (
  agent_id TEXT NOT NULL REFERENCES agent(id),
  key TEXT NOT NULL,
  value TEXT NOT NULL DEFAULT '',
  PRIMARY KEY (agent_id, key)
);

CREATE TABLE IF NOT EXISTS builder_draft (
  session_id TEXT PRIMARY KEY,
  draft TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS autopilot_trigger (
  id TEXT PRIMARY KEY,
  autopilot_id TEXT NOT NULL REFERENCES autopilot(id),
  kind TEXT NOT NULL DEFAULT 'schedule', -- schedule | webhook | api
  cron_expression TEXT,
  timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai',
  label TEXT NOT NULL DEFAULT '',
  event_filters TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  webhook_token TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS autopilot_delivery (
  id TEXT PRIMARY KEY,
  autopilot_id TEXT NOT NULL REFERENCES autopilot(id),
  run_id TEXT,
  trigger_id TEXT,
  status TEXT NOT NULL DEFAULT 'delivered', -- delivered | failed
  detail TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

-- 系统 wakeup：workspace 级（rule 唯一）与 issue 级（issue_id 有值）。
CREATE TABLE IF NOT EXISTS system_wakeup (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  issue_id TEXT,
  rule TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  instruction TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (workspace_id, issue_id, rule)
);

CREATE TABLE IF NOT EXISTS wakeup_run (
  id TEXT PRIMARY KEY,
  wakeup_id TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'succeeded',
  detail TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS notification_preferences (
  workspace_id TEXT PRIMARY KEY REFERENCES workspace(id),
  prefs TEXT NOT NULL DEFAULT '{}',
  updated_at TEXT NOT NULL
);

-- 频道 BYO 配置：用户自填 token/webhook，本地存储，不走 OAuth、不经云端中转。
CREATE TABLE IF NOT EXISTS channel_installation (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  provider TEXT NOT NULL, -- slack | telegram | dingtalk | wecom | lark
  config TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (workspace_id, provider)
);

CREATE TABLE IF NOT EXISTS resource_label (
  resource_type TEXT NOT NULL, -- agent | skill
  resource_id TEXT NOT NULL,
  label_id TEXT NOT NULL REFERENCES label(id),
  PRIMARY KEY (resource_type, resource_id, label_id)
);

CREATE TABLE IF NOT EXISTS pinned_chat_agent (
  agent_id TEXT PRIMARY KEY,
  created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS issue_view_preference (
  scope_type TEXT NOT NULL,
  scope_id TEXT,
  prefs TEXT NOT NULL DEFAULT '{"hidden":[],"order":[]}',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (scope_type, scope_id)
);

-- run.chat_session_id：chat 发送的 run 结束时把输出追加为 assistant 消息。
ALTER TABLE run ADD COLUMN chat_session_id TEXT;

-- chat_session 扩展：agent 归属、置顶/归档、builder 会话（kind=builder 时 runtime_id/model 有效）。
ALTER TABLE chat_session ADD COLUMN agent_id TEXT NOT NULL DEFAULT '';
ALTER TABLE chat_session ADD COLUMN creator_id TEXT NOT NULL DEFAULT 'u-local';
ALTER TABLE chat_session ADD COLUMN has_unread INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_session ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_session ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;
ALTER TABLE chat_session ADD COLUMN last_read_at TEXT;
ALTER TABLE chat_session ADD COLUMN project_id TEXT;
ALTER TABLE chat_session ADD COLUMN kind TEXT NOT NULL DEFAULT 'chat';
ALTER TABLE chat_session ADD COLUMN runtime_id TEXT;
ALTER TABLE chat_session ADD COLUMN model TEXT;

-- attachment.path：上传文件落盘 app_data/attachments/<id>，path 存相对路径。
ALTER TABLE attachment ADD COLUMN path TEXT;
ALTER TABLE attachment ADD COLUMN chat_session_id TEXT;

-- chat_message.task_id / attachments：消息 ↔ run 关联、消息附件。
ALTER TABLE chat_message ADD COLUMN task_id TEXT;
ALTER TABLE chat_message ADD COLUMN attachments TEXT NOT NULL DEFAULT '[]';

ALTER TABLE inbox_item ADD COLUMN archived_at TEXT;

ALTER TABLE pin ADD COLUMN position INTEGER NOT NULL DEFAULT 0;

ALTER TABLE issue_view ADD COLUMN owner_id TEXT NOT NULL DEFAULT '';
ALTER TABLE issue_view ADD COLUMN scope_variant TEXT;
ALTER TABLE issue_view ADD COLUMN display TEXT NOT NULL DEFAULT '{}';
ALTER TABLE issue_view ADD COLUMN revision INTEGER NOT NULL DEFAULT 1;
