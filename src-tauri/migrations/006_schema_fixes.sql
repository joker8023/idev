-- 006：修正 001/005 留下的三处 NULL 语义缺陷。
-- SQLite 的 UNIQUE 对 NULL 不视为冲突，主键含 NULL 列同样不冲突，
-- 因此 workspace 级 system_wakeup（issue_id=NULL）与 view preference（scope_id=NULL）
-- 的 upsert 会插入重复行；run.issue_id 的 NOT NULL + '' 占位则让 chat run 引用了不存在的 issue。

-- 1) system_wakeup.issue_id 改为 NOT NULL DEFAULT ''，workspace 级用 '' 做哨兵；
--    重建时 COALESCE(NULL -> '')，INSERT OR IGNORE 去重已存在的重复 workspace 级行。
CREATE TABLE IF NOT EXISTS system_wakeup_new (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  issue_id TEXT NOT NULL DEFAULT '',
  rule TEXT NOT NULL,
  enabled INTEGER NOT NULL DEFAULT 0,
  instruction TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (workspace_id, issue_id, rule)
);
INSERT OR IGNORE INTO system_wakeup_new (id, workspace_id, issue_id, rule, enabled, instruction, created_at, updated_at)
  SELECT id, workspace_id, COALESCE(issue_id, ''), rule, enabled, instruction, created_at, updated_at FROM system_wakeup;
DROP TABLE system_wakeup;
ALTER TABLE system_wakeup_new RENAME TO system_wakeup;

-- 2) issue_view_preference.scope_id 改为 NOT NULL DEFAULT ''，同理去重。
CREATE TABLE IF NOT EXISTS issue_view_preference_new (
  scope_type TEXT NOT NULL,
  scope_id TEXT NOT NULL DEFAULT '',
  prefs TEXT NOT NULL DEFAULT '{"hidden":[],"order":[]}',
  updated_at TEXT NOT NULL,
  PRIMARY KEY (scope_type, scope_id)
);
INSERT OR IGNORE INTO issue_view_preference_new (scope_type, scope_id, prefs, updated_at)
  SELECT scope_type, COALESCE(scope_id, ''), prefs, updated_at FROM issue_view_preference;
DROP TABLE issue_view_preference;
ALTER TABLE issue_view_preference_new RENAME TO issue_view_preference;

-- 3) run.issue_id 改为 nullable：chat run 没有 issue，用 NULL 而非 '' 占位；
--    历史 '' 占位行一并转为 NULL。保留 005 新增的 chat_session_id。
CREATE TABLE IF NOT EXISTS run_new (
  id TEXT PRIMARY KEY,
  issue_id TEXT REFERENCES issue(id),
  agent_id TEXT NOT NULL,
  agent_name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'queued',
  started_at TEXT NOT NULL,
  finished_at TEXT,
  log TEXT NOT NULL DEFAULT '',
  chat_session_id TEXT
);
INSERT OR IGNORE INTO run_new (id, issue_id, agent_id, agent_name, status, started_at, finished_at, log, chat_session_id)
  SELECT id, NULLIF(issue_id, ''), agent_id, agent_name, status, started_at, finished_at, log, chat_session_id FROM run;
DROP TABLE run;
ALTER TABLE run_new RENAME TO run;
CREATE INDEX IF NOT EXISTS idx_run_issue ON run(issue_id);
