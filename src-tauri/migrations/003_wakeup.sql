-- 003：wakeup 按 core IssueWakeup 全字段扩展；kind 改为 core 语义
-- （event | at | every | cron），user/system 区分改用 origin 列。
ALTER TABLE wakeup ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';
ALTER TABLE wakeup ADD COLUMN agent_name TEXT NOT NULL DEFAULT '';
ALTER TABLE wakeup ADD COLUMN instruction TEXT NOT NULL DEFAULT '';
ALTER TABLE wakeup ADD COLUMN mode TEXT NOT NULL DEFAULT 'once';
ALTER TABLE wakeup ADD COLUMN event_types TEXT NOT NULL DEFAULT '[]';
ALTER TABLE wakeup ADD COLUMN filter_agent_id TEXT;
ALTER TABLE wakeup ADD COLUMN filter_task_id TEXT;
ALTER TABLE wakeup ADD COLUMN interval_seconds INTEGER;
ALTER TABLE wakeup ADD COLUMN cron_expression TEXT;
ALTER TABLE wakeup ADD COLUMN timezone TEXT NOT NULL DEFAULT 'Asia/Shanghai';
ALTER TABLE wakeup ADD COLUMN next_run_at TEXT;
ALTER TABLE wakeup ADD COLUMN last_run_at TEXT;
ALTER TABLE wakeup ADD COLUMN paused INTEGER NOT NULL DEFAULT 0;
ALTER TABLE wakeup ADD COLUMN updated_at TEXT;
