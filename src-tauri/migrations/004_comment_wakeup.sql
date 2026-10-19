-- 004：comment 按 core Comment 全字段；wakeup 按 core IssueWakeup 全字段。
-- 旧 comment 列（author_name / body）保留，写路径改走新列。

-- ── comment ──
ALTER TABLE comment ADD COLUMN author_id TEXT NOT NULL DEFAULT 'u-local';
ALTER TABLE comment ADD COLUMN content TEXT NOT NULL DEFAULT '';
ALTER TABLE comment ADD COLUMN type TEXT NOT NULL DEFAULT 'comment';
ALTER TABLE comment ADD COLUMN parent_id TEXT;
ALTER TABLE comment ADD COLUMN updated_at TEXT;
ALTER TABLE comment ADD COLUMN resolved_at TEXT;
ALTER TABLE comment ADD COLUMN resolved_by_type TEXT;
ALTER TABLE comment ADD COLUMN resolved_by_id TEXT;

-- ── wakeup ──
ALTER TABLE wakeup ADD COLUMN next_fire_at TEXT;
ALTER TABLE wakeup ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE wakeup ADD COLUMN disabled_at TEXT;
ALTER TABLE wakeup ADD COLUMN last_task_id TEXT;
ALTER TABLE wakeup ADD COLUMN last_error TEXT;
ALTER TABLE wakeup ADD COLUMN filter_agent_name TEXT;
ALTER TABLE wakeup ADD COLUMN filter_actor_type TEXT;
ALTER TABLE wakeup ADD COLUMN filter_actor_id TEXT;
ALTER TABLE wakeup ADD COLUMN filter_actor_name TEXT;
ALTER TABLE wakeup ADD COLUMN condition TEXT;
ALTER TABLE wakeup ADD COLUMN max_fires INTEGER;
ALTER TABLE wakeup ADD COLUMN fire_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE wakeup ADD COLUMN paused_reason TEXT;
ALTER TABLE wakeup ADD COLUMN expires_at TEXT;
ALTER TABLE wakeup ADD COLUMN expiry_seconds INTEGER;
ALTER TABLE wakeup ADD COLUMN on_timeout TEXT;
ALTER TABLE wakeup ADD COLUMN timed_out_at TEXT;
ALTER TABLE wakeup ADD COLUMN created_by_name TEXT;
ALTER TABLE wakeup ADD COLUMN source_agent_id TEXT;
ALTER TABLE wakeup ADD COLUMN source_agent_name TEXT;
