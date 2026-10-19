// Route A 传输层：原装前端 ↔ Rust 后端。
//
// 原装前端的所有数据都走 `api.<method>(...)`（@multica/core/api 的 ApiClient，
// 5239 行，约 400 个方法，底层是 fetch + cookie + CSRF）。本地版不重写这 400 个
// 方法，而是用一个 Proxy 接管：
//   - Tauri 运行时 → `invoke`（真 Rust 命令 + SQLite，见 src-tauri）；
//   - 浏览器预览（Playwright 验证）→ mock（shape 按原 ApiClient/类型写）；
//   - 未迁移的方法 → 抛明确错误 `[tauri-api] <method> not yet migrated`，
//     而不是静默返回假数据。
//
// 约定：
// - Tauri invoke 参数用 snake_case（与 Rust 命令签名一致）；
// - workspace 可传 id 或 slug（Rust 侧 resolve_workspace_id 统一解析），
//   本文件在没有调用方传入时传 MOCK_WORKSPACE.slug（"default"）；
// - 读不到的单个实体（get_* 返回 null）→ 抛错，页面进错误态，不返回假数据。

import { invoke } from "@tauri-apps/api/core";
import { setApiInstance } from "@multica/core/api";
import type { ApiClient } from "@multica/core/api/client";
import type { Issue, User, Workspace } from "@multica/core/types";

const inTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// 调试钩子：记录每次 api 调用（方法名 + 结果/错误），Playwright / 浏览器
// 控制台可通过 `window.__tauriApiLog` 查看。迁移期间定位“页面空白是因为哪个
// 方法没接”全靠它；生产包里也保留，开销可忽略。
declare global {
  interface Window {
    __tauriApiLog?: Array<{ method: string; ok: boolean; error?: string }>;
  }
}
if (typeof window !== "undefined") window.__tauriApiLog = [];

// ── Mock（仅浏览器预览；Tauri 运行时一律走 invoke） ─────────────────────────
const NOW = new Date().toISOString();
const MOCK_USER = {
  id: "u-local",
  name: "本地用户",
  email: "local@multica.local",
  avatar_url: null,
  onboarded_at: NOW,
  onboarding_questionnaire: {},
  starter_content_state: "imported",
} as unknown as User;

const MOCK_WORKSPACE = {
  id: "ws-default",
  name: "默认工作区",
  slug: "default",
  description: null,
  context: null,
  settings: {},
  repos: [],
  issue_prefix: "MUL",
  avatar_url: null,
  created_at: NOW,
  updated_at: NOW,
} as unknown as Workspace;

/** 调用方没传 workspace 时用的键：slug "default"（Rust 侧解析为真实 id）。 */
const WS = MOCK_WORKSPACE.slug;

function mockIssue(
  id: string,
  title: string,
  status: string,
  priority: string,
): Issue {
  // 字段按原仓库测试 fixture 的完整 Issue 形状：详情页 / 命令面板会
  // 直接读 identifier（copy-issue-link 的 keywords 调 identifier.toLowerCase()）、
  // status_category 等，缺了就 throw。
  const n = Number(id.replace(/\D/g, "")) || 1;
  return {
    id,
    workspace_id: MOCK_WORKSPACE.id,
    number: n,
    identifier: `MUL-${n}`,
    title,
    body: "",
    description: null,
    status,
    status_category: "unstarted",
    priority,
    assignee_type: null,
    assignee_id: null,
    creator_type: "member",
    creator_id: MOCK_USER.id,
    parent_issue_id: null,
    project_id: null,
    position: n,
    stage: null,
    start_date: null,
    due_date: null,
    labels: [],
    metadata: {},
    properties: {},
    comments_count: 0,
    created_at: NOW,
    updated_at: NOW,
  } as unknown as Issue;
}

let mockIssues: Issue[] = [
  mockIssue("1", "原装前端验证：看板能渲染真实 BoardView", "todo", "medium"),
  mockIssue("2", "原装前端验证：Agent 指派与运行", "in_progress", "high"),
  mockIssue("3", "原装前端验证：拖拽改状态", "backlog", "low"),
];

const MOCK_STATUSES = [
  { id: "s-backlog", key: "backlog", name: "Backlog", category: "backlog", color: "#94a3b8", position: 0 },
  { id: "s-todo", key: "todo", name: "Todo", category: "todo", color: "#3b82f6", position: 1 },
  { id: "s-progress", key: "in_progress", name: "In Progress", category: "in_progress", color: "#f59e0b", position: 2 },
  { id: "s-review", key: "in_review", name: "In Review", category: "in_review", color: "#8b5cf6", position: 3 },
  { id: "s-done", key: "done", name: "Done", category: "done", color: "#22c55e", position: 4 },
];

// ── Handler 表：方法名 → 实现 ──────────────────────────────────────────────
type Handler = (...args: never[]) => unknown;

/** Tauri 模式下 get_* 为 null 时抛错（页面进错误态，不返回假数据）。 */
function orThrow<T>(value: T | null | undefined, what: string): T {
  if (value == null) throw new Error(`[tauri-api] ${what} not found`);
  return value;
}

const MOCK_AGENTS = [
  // Agents 页默认 scope 是 "mine"（owner_id === 当前用户）：一个归本地用户，
  // 一个归他人，这样 Mine/All 计数都真实（Mine 1，All 2）。缺 owner_id 时
  // Mine 为 0 行，页面正确地显示 No matches。
  { id: "a-claude", name: "Claude", provider: "claude", cli: "claude", status: "offline", owner_id: MOCK_USER.id, workspace_id: MOCK_WORKSPACE.id, runtime_id: "", runtime_mode: "local", description: "", instructions: "", avatar_url: null, skills: [], custom_args: [], runtime_config: {}, created_at: NOW, updated_at: NOW },
  { id: "a-codex", name: "Codex", provider: "codex", cli: "codex", status: "offline", owner_id: "u-other", workspace_id: MOCK_WORKSPACE.id, runtime_id: "", runtime_mode: "local", description: "", instructions: "", avatar_url: null, skills: [], custom_args: [], runtime_config: {}, created_at: NOW, updated_at: NOW },
];

const handlers: Record<string, Handler> = {
  getMe: async () => MOCK_USER,

  // ApiClient.getBaseUrl()：原实现返回 HTTP baseUrl（附件/头像拼绝对 URL 用）。
  // 本地版无 HTTP 后端，返回 ""（相对路径），调用方继续用相对地址。
  getBaseUrl: async () => "",

  listWorkspaces: async () => {
    if (inTauri) return invoke("list_workspaces");
    return [MOCK_WORKSPACE];
  },
  getWorkspace: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_workspace", { id }), `workspace ${id}`);
    return MOCK_WORKSPACE;
  },
  createWorkspace: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_workspace", { name: data.name as string, slug: (data.slug as string) ?? null });
    }
    return { ...MOCK_WORKSPACE, name: data.name as string, slug: (data.slug as string) ?? "default" };
  },

  listIssues: async (params?: { workspace_id?: string }) => {
    if (inTauri) {
      const issues = (await invoke("list_issues", {
        workspace_id: params?.workspace_id ?? WS,
      })) as Issue[];
      return { issues, total: issues.length };
    }
    return { issues: mockIssues, total: mockIssues.length };
  },

  getIssue: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_issue", { id }), `issue ${id}`);
    return mockIssues.find((i) => i.id === id) ?? mockIssues[0];
  },

  createIssue: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_issue", {
        workspace_id: (data.workspace_id as string) ?? WS,
        title: data.title as string,
        body: (data.body as string) ?? (data.description as string) ?? null,
        priority: (data.priority as string) ?? null,
        description: (data.description as string) ?? null,
        project_id: (data.project_id as string) ?? null,
        assignee_type: (data.assignee_type as string) ?? null,
        assignee_id: (data.assignee_id as string) ?? null,
        status: (data.status as string) ?? null,
      });
    }
    const it = mockIssue(String(mockIssues.length + 1), String(data.title ?? "Untitled"), "backlog", String(data.priority ?? "medium"));
    mockIssues = [...mockIssues, it];
    return it;
  },

  updateIssue: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_issue", {
        id,
        title: (data.title as string) ?? null,
        body: (data.body as string) ?? (data.description as string) ?? null,
        description: (data.description as string) ?? null,
        priority: (data.priority as string) ?? null,
        status: (data.status as string) ?? null,
        assignee_type: (data.assignee_type as string) ?? null,
        assignee_id: (data.assignee_id as string) ?? null,
        project_id: (data.project_id as string) ?? null,
      });
      // 返回最新行（assignee 切到 agent 时后端已 spawn run，前端失效缓存后可见）。
      return orThrow(await invoke("get_issue", { id }), `issue ${id}`);
    }
    mockIssues = mockIssues.map((i) => (i.id === id ? { ...i, ...data } : i));
    return mockIssues.find((i) => i.id === id);
  },

  moveIssue: async (id: string, data: Record<string, unknown>) => {
    // 看板拖拽：原 ApiClient.moveIssue(id, {status})。
    if (inTauri) {
      await invoke("update_issue_status", { id, status: data.status as string });
      return { id, status: data.status };
    }
    mockIssues = mockIssues.map((i) => (i.id === id ? { ...i, status: data.status as string } : i));
    return mockIssues.find((i) => i.id === id);
  },

  deleteIssue: async (id: string) => {
    if (inTauri) {
      await invoke("delete_issue", { id });
      return;
    }
    mockIssues = mockIssues.filter((i) => i.id !== id);
  },

  listComments: async (issueId: string) => {
    if (inTauri) return invoke("list_comments", { issue_id: issueId });
    return [] as unknown[];
  },
  createComment: async (issueId: string, content: string, type?: string, parentId?: string) => {
    if (inTauri) {
      return invoke("add_comment", {
        issue_id: issueId,
        content,
        comment_type: type ?? null,
        parent_id: parentId ?? null,
      });
    }
    return { id: `c-${Date.now()}`, issue_id: issueId, content } as unknown;
  },
  updateComment: async (commentId: string, content: string) => {
    if (inTauri) return orThrow(await invoke("update_comment", { id: commentId, content }), `comment ${commentId}`);
    return { id: commentId, content } as unknown;
  },
  deleteComment: async (commentId: string) => {
    if (inTauri) {
      await invoke("delete_comment", { id: commentId });
      return;
    }
  },

  listAgents: async (params?: { workspace_id?: string }) => {
    if (inTauri) return invoke("list_agents", { workspace_id: params?.workspace_id ?? WS });
    return MOCK_AGENTS;
  },
  getAgent: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_agent", { id }), `agent ${id}`);
    return MOCK_AGENTS.find((a) => a.id === id) ?? MOCK_AGENTS[0];
  },
  createAgent: async (data: Record<string, unknown>) => {
    if (inTauri) {
      const rc = data.runtime_config as Record<string, unknown> | undefined;
      return invoke("create_agent", {
        workspace_id: WS,
        name: data.name as string,
        runtime_id: (data.runtime_id as string) ?? null,
        description: (data.description as string) ?? null,
        instructions: (data.instructions as string) ?? null,
        avatar_url: (data.avatar_url as string) ?? null,
        model: (data.model as string) ?? null,
        owner_id: MOCK_USER.id,
        cli: (data.cli as string) ?? (rc?.cli as string) ?? null,
        runtime_config: rc ? JSON.stringify(rc) : null,
        max_concurrent_tasks: (data.max_concurrent_tasks as number) ?? null,
      });
    }
    return { ...MOCK_AGENTS[0], id: `a-${Date.now()}`, name: data.name } as unknown;
  },
  updateAgent: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      const rc = data.runtime_config as Record<string, unknown> | undefined;
      await invoke("update_agent", {
        id,
        name: (data.name as string) ?? null,
        description: (data.description as string) ?? null,
        instructions: (data.instructions as string) ?? null,
        runtime_id: (data.runtime_id as string) ?? null,
        model: (data.model as string) ?? null,
        status: (data.status as string) ?? null,
        avatar_url: (data.avatar_url as string) ?? null,
        runtime_config: rc ? JSON.stringify(rc) : null,
        max_concurrent_tasks: (data.max_concurrent_tasks as number) ?? null,
      });
      return orThrow(await invoke("get_agent", { id }), `agent ${id}`);
    }
    return { ...MOCK_AGENTS[0], id, ...data } as unknown;
  },
  deleteAgent: async (id: string) => {
    if (inTauri) {
      await invoke("delete_agent", { id });
      return;
    }
  },
  cancelAgentTasks: async (id: string) => {
    if (inTauri) return invoke("cancel_agent_tasks", { agent_id: id });
    return { cancelled: 0 };
  },

  listRuntimes: async () => {
    if (inTauri) return invoke("list_runtimes");
    return [{ id: "local", name: "本机", kind: "local", status: "online", detected_clis: [] }];
  },
  listRuntimeProfiles: async (workspaceId?: string) => {
    if (inTauri) return invoke("list_runtime_profiles", { workspace_id: workspaceId ?? WS });
    return [] as unknown[];
  },

  listLabels: async () => {
    if (inTauri) return invoke("list_labels", { workspace_id: WS });
    return { labels: [], total: 0 };
  },
  createLabel: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_label", {
        workspace_id: WS,
        name: data.name as string,
        color: (data.color as string) ?? null,
      });
    }
    return { id: `l-${Date.now()}`, name: data.name } as unknown;
  },
  updateLabel: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_label", {
          id,
          name: (data.name as string) ?? null,
          color: (data.color as string) ?? null,
          description: (data.description as string) ?? null,
        }),
        `label ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  deleteLabel: async (id: string) => {
    if (inTauri) {
      await invoke("delete_label", { id });
      return;
    }
  },
  listLabelsForIssue: async (issueId: string) => {
    if (inTauri) return invoke("list_labels_for_issue", { issue_id: issueId });
    return { labels: [] };
  },
  listLabelsForResource: async (resourceType: string, resourceId: string) => {
    if (inTauri) return invoke("list_labels_for_resource", { resource_type: resourceType, resource_id: resourceId });
    return { labels: [] };
  },

  listMembers: async (workspaceId: string) => {
    if (inTauri) return invoke("list_members", { workspace_id: workspaceId ?? WS });
    return [] as unknown[];
  },

  listProjects: async () => {
    if (inTauri) return invoke("list_projects", { workspace_id: WS });
    return { projects: [], total: 0 };
  },
  getProject: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_project", { id }), `project ${id}`);
    // status 必须是 PROJECT_STATUS_CONFIG 的 key（planned/in_progress/paused/
    // completed/cancelled）；"active" 会让详情页读 config[status].dotColor 时 throw。
    return ({ id, workspace_id: MOCK_WORKSPACE.id, title: "示例项目", description: null, icon: null, status: "planned", priority: "medium", lead_type: null, lead_id: null, start_date: null, due_date: null, issue_count: 0, done_count: 0, resource_count: 0, created_at: NOW, updated_at: NOW }) as unknown;
  },
  createProject: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_project", {
        workspace_id: WS,
        title: data.title as string,
        description: (data.description as string) ?? null,
        status: (data.status as string) ?? null,
        priority: (data.priority as string) ?? null,
      });
    }
    return { id: `p-${Date.now()}`, title: data.title } as unknown;
  },
  updateProject: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_project", {
        id,
        title: (data.title as string) ?? null,
        description: (data.description as string) ?? null,
        status: (data.status as string) ?? null,
        priority: (data.priority as string) ?? null,
      });
      return orThrow(await invoke("get_project", { id }), `project ${id}`);
    }
    return { id, ...data } as unknown;
  },
  deleteProject: async (id: string) => {
    if (inTauri) {
      await invoke("delete_project", { id });
      return;
    }
  },
  listProjectResources: async (projectId: string) => {
    if (inTauri) return invoke("list_project_resources", { project_id: projectId });
    return { resources: [], total: 0 };
  },

  listSquads: async () => {
    if (inTauri) return invoke("list_squads", { workspace_id: WS });
    return [] as unknown[];
  },
  getSquad: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_squad", { id }), `squad ${id}`);
    // leader_id 不能为空：详情页 getEntityName(type, leader_id) 会对它调 .slice(0, 8)。
    return ({ id, workspace_id: MOCK_WORKSPACE.id, name: "示例 Squad", description: "", instructions: "", avatar_url: null, leader_id: "a-claude", creator_id: MOCK_USER.id, created_at: NOW, updated_at: NOW, archived_at: null, archived_by: null, member_count: 0, member_preview: [] }) as unknown;
  },
  createSquad: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_squad", {
        workspace_id: WS,
        name: data.name as string,
        description: (data.description as string) ?? null,
        leader_id: (data.leader_id as string) ?? null,
      });
    }
    return { id: `sq-${Date.now()}`, name: data.name } as unknown;
  },
  updateSquad: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_squad", {
        id,
        name: (data.name as string) ?? null,
        description: (data.description as string) ?? null,
        leader_id: (data.leader_id as string) ?? null,
        instructions: (data.instructions as string) ?? null,
      });
      return orThrow(await invoke("get_squad", { id }), `squad ${id}`);
    }
    return { id, ...data } as unknown;
  },
  deleteSquad: async (id: string) => {
    if (inTauri) {
      await invoke("delete_squad", { id });
      return;
    }
  },
  listSquadMembers: async (squadId: string) => {
    if (inTauri) return invoke("list_squad_members", { squad_id: squadId });
    return [] as unknown[];
  },
  getSquadMemberStatus: async (squadId: string) => {
    if (inTauri) return invoke("get_squad_member_status", { squad_id: squadId });
    return { members: [] };
  },

  listIssueViews: async () => {
    if (inTauri) return invoke("list_issue_views", { workspace_id: WS });
    return [] as unknown[];
  },
  // listIssueProperties：旧方法名，无调用方，保留空态。
  listIssueProperties: async () => [] as unknown[],
  listIssueStatuses: async () => {
    if (inTauri) return invoke("list_issue_statuses", { workspace_id: WS });
    return { statuses: MOCK_STATUSES, categories: ["unstarted", "started", "done", "closed"], total: MOCK_STATUSES.length };
  },
  // listIssueStatusCategories：无调用方，保留空态。
  listIssueStatusCategories: async () => [] as unknown[],
  listIssueWakeupSummaries: async () => {
    if (inTauri) return invoke("list_issue_wakeup_summaries");
    return [] as unknown[];
  },
  getWorkspaceWorkingAgents: async () => {
    if (inTauri) return invoke("get_workspace_working_agents", { workspace_id: WS });
    return [] as unknown[];
  },
  getWorkspaceAgentActivity30d: async () => {
    if (inTauri) return invoke("get_workspace_agent_activity_30d");
    return [] as unknown[];
  },
  getWorkspaceAgentRunCounts: async () => {
    if (inTauri) return invoke("get_workspace_agent_run_counts");
    return [] as unknown[];
  },

  // Issues 页实际走表模型（不是 listIssues）：表行 + facet + 视图偏好 + 置顶 + 属性 + 子任务进度。
  listIssueTableRows: async () => {
    if (inTauri) return invoke("list_issue_table_rows", { workspace_id: WS });
    return {
      query_fingerprint: "mock",
      group_key: null,
      parent_id: null,
      total: mockIssues.length,
      rows: mockIssues.map((issue) => ({ issue, direct_child_count: 0 })),
      branch_total: mockIssues.length,
      next_cursor: null,
    };
  },
  listIssueTableFacets: async () => {
    if (inTauri) return invoke("list_issue_table_facets", { workspace_id: WS });
    return {
      query_fingerprint: "mock",
      total: mockIssues.length,
      facets: [],
    };
  },
  getIssueViewPreference: async (params: { scope_type: string; scope_id?: string | null }) => {
    if (inTauri) {
      return invoke("get_issue_view_preference", {
        scope_type: params?.scope_type ?? "workspace",
        scope_id: params?.scope_id ?? null,
      });
    }
    return {
      scope_type: params?.scope_type ?? "workspace",
      scope_id: params?.scope_id ?? null,
      prefs: { hidden: [], order: [] },
      updated_at: "",
    };
  },
  listPins: async () => {
    if (inTauri) return invoke("list_pins", { workspace_id: WS });
    return [] as unknown[];
  },
  listProperties: async () => {
    if (inTauri) return invoke("list_properties", { workspace_id: WS });
    return { properties: [], total: 0 };
  },
  getChildIssueProgress: async () => {
    if (inTauri) return invoke("get_child_issue_progress", { workspace_id: WS });
    return { progress: [] };
  },
  listPausedWakeups: async () => {
    if (inTauri) return invoke("list_paused_wakeups");
    return [] as unknown[];
  },
  getAgentTaskSnapshot: async () => {
    if (inTauri) return invoke("get_agent_task_snapshot");
    return [] as unknown[];
  },
  listAgentTasksPage: async (agentId: string) => {
    if (inTauri) return invoke("list_agent_tasks_page", { agent_id: agentId });
    return { tasks: [], nextCursor: null };
  },
  listTasksByIssue: async (issueId: string) => {
    if (inTauri) return invoke("list_tasks_by_issue", { issue_id: issueId });
    return [] as unknown[];
  },
  getActiveTasksForIssue: async (issueId: string) => {
    if (inTauri) return invoke("get_active_tasks_for_issue", { issue_id: issueId });
    return { tasks: [] };
  },
  cancelTask: async (_issueId: string, taskId: string) => {
    if (inTauri) return orThrow(await invoke("cancel_task", { task_id: taskId }), `task ${taskId}`);
    return { id: taskId, status: "cancelled" } as unknown;
  },

  listMyInvitations: async () => {
    if (inTauri) return invoke("list_my_invitations");
    return [] as unknown[];
  },
  getInboxUnreadSummary: async () => {
    if (inTauri) return invoke("get_inbox_unread_summary");
    return [] as unknown[];
  },
  listInbox: async () => {
    if (inTauri) return invoke("list_inbox", { workspace_id: WS });
    return [] as unknown[];
  },
  listChatSessions: async (_params?: unknown, workspaceSlug?: string) => {
    if (inTauri) return invoke("list_chat_sessions", { workspace_id: (workspaceSlug as string) ?? WS });
    return [] as unknown[];
  },
  listChatMessages: async (sessionId: string) => {
    if (inTauri) return invoke("list_chat_messages", { session_id: sessionId });
    return [] as unknown[];
  },
  listChatDraftRestores: async (sessionId: string) => {
    if (inTauri) return invoke("list_chat_draft_restores", { session_id: sessionId });
    return { restores: [] };
  },
  getPendingChatTask: async (sessionId: string) => {
    if (inTauri) return invoke("get_pending_chat_task", { session_id: sessionId });
    return {} as unknown;
  },
  listPendingChatTasks: async () => {
    if (inTauri) return invoke("list_pending_chat_tasks");
    return { tasks: [] };
  },
  hasAnyPendingChatTasks: async () => {
    if (inTauri) return invoke("has_any_pending_chat_tasks");
    return { has_pending: false };
  },

  listAutopilots: async () => {
    if (inTauri) return invoke("list_autopilots", { workspace_id: WS });
    return { autopilots: [], total: 0 };
  },
  getAutopilot: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_autopilot", { id }), `autopilot ${id}`);
    return ({
      autopilot: { id, title: "示例 Autopilot", enabled: false, created_at: NOW, updated_at: NOW },
      triggers: [],
    }) as unknown;
  },
  createAutopilot: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_autopilot", {
        workspace_id: WS,
        title: data.title as string,
        description: (data.description as string) ?? null,
        project_id: (data.project_id as string) ?? null,
        assignee_type: (data.assignee_type as string) ?? null,
        assignee_id: data.assignee_id as string,
        execution_mode: data.execution_mode as string,
        issue_title_template: (data.issue_title_template as string) ?? null,
      });
    }
    return { id: `ap-${Date.now()}`, title: data.title } as unknown;
  },
  updateAutopilot: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_autopilot", {
        id,
        title: (data.title as string) ?? null,
        description: (data.description as string) ?? null,
        status: (data.status as string) ?? null,
        assignee_id: (data.assignee_id as string) ?? null,
      });
      const res = (await invoke("get_autopilot", { id })) as { autopilot: unknown };
      return res.autopilot;
    }
    return { id, ...data } as unknown;
  },
  deleteAutopilot: async (id: string) => {
    if (inTauri) {
      await invoke("delete_autopilot", { id });
      return;
    }
  },
  listAutopilotRuns: async (id: string) => {
    if (inTauri) return invoke("list_autopilot_runs", { autopilot_id: id });
    return { runs: [], total: 0 };
  },

  listSkills: async () => {
    if (inTauri) return invoke("list_skills", { workspace_id: WS });
    return [] as unknown[];
  },
  getSkill: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_skill", { id }), `skill ${id}`);
    return ({ id, name: "示例 Skill", description: "", created_at: NOW, updated_at: NOW }) as unknown;
  },
  createSkill: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_skill", {
        workspace_id: WS,
        name: data.name as string,
        description: (data.description as string) ?? "",
        content: (data.content as string) ?? "",
      });
    }
    return { id: `sk-${Date.now()}`, name: data.name } as unknown;
  },
  updateSkill: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_skill", {
        id,
        name: (data.name as string) ?? null,
        description: (data.description as string) ?? null,
        content: (data.content as string) ?? null,
      });
      return orThrow(await invoke("get_skill", { id }), `skill ${id}`);
    }
    return { id, ...data } as unknown;
  },
  deleteSkill: async (id: string) => {
    if (inTauri) {
      await invoke("delete_skill", { id });
      return;
    }
  },

  listAgentBuilderSessions: async () => {
    if (inTauri) return invoke("list_agent_builder_sessions");
    return [] as unknown[];
  },
  saveAgentBuilderDraft: async (sessionId: string, draft: unknown) => {
    if (inTauri) {
      await invoke("save_agent_builder_draft", { session_id: sessionId, draft });
      return;
    }
  },

  getAttachment: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_attachment", { id }), `attachment ${id}`);
    return ({ id, filename: "attachment", mime_type: "application/octet-stream" }) as unknown;
  },
  listAttachments: async (issueId: string) => {
    if (inTauri) return invoke("list_attachments", { issue_id: issueId });
    return [] as unknown[];
  },
  listTimeline: async (issueId: string) => {
    if (inTauri) return invoke("list_timeline", { issue_id: issueId });
    return [] as unknown[];
  },
  listChildIssues: async (id: string) => {
    if (inTauri) return invoke("list_child_issues", { id });
    return { issues: [] };
  },
  listIssueWakeups: async (issueId: string) => {
    if (inTauri) return invoke("list_issue_wakeups", { issue_id: issueId });
    return [] as unknown[];
  },
  listIssueSystemWakeups: async (issueId: string) => {
    if (inTauri) return invoke("list_issue_system_wakeups", { issue_id: issueId });
    return [] as unknown[];
  },
  createIssueWakeup: async (issueId: string, input: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_issue_wakeup", {
        issue_id: issueId,
        agent_id: input.agent_id as string,
        instruction: input.instruction as string,
        kind: (input.kind as string) ?? null,
        mode: (input.mode as string) ?? null,
        event_types: input.event_types ? JSON.stringify(input.event_types) : null,
        filter_agent_id: (input.filter_agent_id as string) ?? null,
        filter_task_id: (input.filter_task_id as string) ?? null,
        filter_actor_type: (input.filter_actor_type as string) ?? null,
        filter_actor_id: (input.filter_actor_id as string) ?? null,
        at: (input.at as string) ?? null,
        interval_seconds: (input.interval_seconds as number) ?? null,
        cron_expression: (input.cron_expression as string) ?? null,
        timezone: (input.timezone as string) ?? null,
        expires_at: (input.expires_at as string) ?? null,
        condition: input.condition ? JSON.stringify(input.condition) : null,
      });
    }
    return { id: `w-${Date.now()}` } as unknown;
  },
  updateIssueWakeup: async (_issueId: string, wakeupId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      await invoke("update_issue_wakeup", {
        id: wakeupId,
        instruction: (data.instruction as string) ?? null,
        paused: (data.paused as boolean) ?? null,
        status: (data.status as string) ?? null,
      });
      return;
    }
  },
  deleteIssueWakeup: async (_issueId: string, wakeupId: string) => {
    if (inTauri) {
      await invoke("delete_issue_wakeup", { id: wakeupId });
      return;
    }
  },
  triggerIssueWakeup: async (issueId: string, wakeupId: string) => {
    if (inTauri) {
      await invoke("trigger_issue_wakeup", { issue_id: issueId, wakeup_id: wakeupId });
      return;
    }
  },
  listIssueDuplicates: async (id: string) => {
    if (inTauri) return invoke("list_issue_duplicates", { id });
    return { duplicate_of: null, duplicates: [] };
  },
  listQuickActions: async () => {
    if (inTauri) return invoke("list_quick_actions");
    return { quick_actions: [] };
  },
  getAssigneeFrequency: async () => {
    if (inTauri) return invoke("get_assignee_frequency", { workspace_id: WS });
    return [] as unknown[];
  },
  listIssuePullRequests: async (issueId: string) => {
    if (inTauri) return invoke("list_issue_pull_requests", { issue_id: issueId });
    return { pull_requests: [], auto_complete: null };
  },
  listIssueSubscribers: async (issueId: string) => {
    if (inTauri) return invoke("list_issue_subscribers", { issue_id: issueId });
    return [] as unknown[];
  },

  // Dashboard（Usage 页）：本地版暂无用量统计，返回空序列，页面渲染空图表。
  getDashboardUsageDaily: async () => {
    if (inTauri) return invoke("get_dashboard_usage_daily");
    return [] as unknown[];
  },
  getDashboardUsageByAgent: async () => {
    if (inTauri) return invoke("get_dashboard_usage_by_agent");
    return [] as unknown[];
  },
  getDashboardAgentRunTime: async () => {
    if (inTauri) return invoke("get_dashboard_agent_run_time");
    return [] as unknown[];
  },
  getDashboardRunTimeDaily: async () => {
    if (inTauri) return invoke("get_dashboard_run_time_daily");
    return [] as unknown[];
  },
  getDashboardFailuresDaily: async () => {
    if (inTauri) return invoke("get_dashboard_failures_daily");
    return [] as unknown[];
  },
  getDashboardFailuresByAgent: async () => {
    if (inTauri) return invoke("get_dashboard_failures_by_agent");
    return [] as unknown[];
  },

  getSearchIndexManifest: async (params: { workspaceSlug: string }) => {
    if (inTauri) return invoke("get_search_index_manifest", { workspace_id: params?.workspaceSlug ?? WS });
    return {
      cursor: "mock-0",
      issue_count: 0,
      comment_count: 0,
      project_count: 0,
      text_bytes: 0,
    };
  },
  getSearchIndexSnapshot: async (params: { workspaceSlug: string }) => {
    if (inTauri) return invoke("get_search_index_snapshot", { workspace_id: params?.workspaceSlug ?? WS });
    return { issues: [], comments: [], projects: [], next_after_number: 0 };
  },

  // 聊天频道安装：configured=false → 页面渲染“请运营方开启”空态，不报错。
  listLarkInstallations: async () => {
    if (inTauri) return invoke("list_lark_installations");
    return { installations: [], configured: false } as unknown;
  },
  listSlackInstallations: async () => {
    if (inTauri) return invoke("list_slack_installations");
    return { installations: [], configured: false } as unknown;
  },
  listDingTalkInstallations: async () => {
    if (inTauri) return invoke("list_dingtalk_installations");
    return { installations: [], configured: false } as unknown;
  },
  listWecomInstallations: async () => {
    if (inTauri) return invoke("list_wecom_installations");
    return { installations: [], configured: false } as unknown;
  },
  listTelegramInstallations: async () => {
    if (inTauri) return invoke("list_telegram_installations");
    return { installations: [], configured: false } as unknown;
  },

  // ── 第二批写路径（write2.rs） ──────────────────────────────────────────
  createIssueStatus: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_issue_status", {
        workspace_id: WS,
        key: (data.key as string) ?? null,
        name: data.name as string,
        description: (data.description as string) ?? null,
        category: data.category as string,
        color: data.color as string,
        icon: (data.icon as string) ?? null,
      });
    }
    return { id: `st-${Date.now()}`, name: data.name } as unknown;
  },
  updateIssueStatus: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_issue_status_def", {
          id,
          name: (data.name as string) ?? null,
          description: (data.description as string) ?? null,
          color: (data.color as string) ?? null,
          icon: (data.icon as string) ?? null,
          position: (data.position as number) ?? null,
        }),
        `issue status ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  archiveIssueStatus: async (id: string) => {
    if (inTauri) {
      await invoke("archive_issue_status", { id });
      return;
    }
  },
  reorderIssueStatuses: async (category: string, ids: string[], includeSystem?: boolean) => {
    if (inTauri) {
      await invoke("reorder_issue_statuses", {
        workspace_id: WS,
        category,
        ids,
        include_system: includeSystem ?? null,
      });
      return;
    }
  },

  createIssueView: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_issue_view", {
        workspace_id: WS,
        name: data.name as string,
        scope_type: data.scope_type as string,
        scope_id: (data.scope_id as string) ?? null,
        scope_variant: (data.scope_variant as string) ?? null,
        visibility: data.visibility as string,
        definition_version: (data.definition_version as number) ?? 1,
        query: (data.query as Record<string, unknown>) ?? {},
        display: (data.display as Record<string, unknown>) ?? {},
      });
    }
    return { id: `v-${Date.now()}`, name: data.name } as unknown;
  },
  updateIssueView: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_issue_view", {
          id,
          name: (data.name as string) ?? null,
          visibility: (data.visibility as string) ?? null,
          scope_variant: (data.scope_variant as string) ?? null,
          query: (data.query as Record<string, unknown>) ?? null,
          display: (data.display as Record<string, unknown>) ?? null,
          expected_revision: data.expected_revision as number,
        }),
        `issue view ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  deleteIssueView: async (id: string) => {
    if (inTauri) {
      await invoke("delete_issue_view", { id });
      return;
    }
  },
  getIssueView: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_issue_view", { id }), `issue view ${id}`);
    return { id } as unknown;
  },
  putIssueViewPreference: async (data: { scope_type: string; scope_id?: string | null; prefs: { hidden: string[]; order: string[] } }) => {
    if (inTauri) {
      return invoke("put_issue_view_preference", {
        scope_type: data.scope_type,
        scope_id: data.scope_id ?? null,
        prefs: data.prefs,
      });
    }
    return data as unknown;
  },

  createPin: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_pin", {
        workspace_id: WS,
        item_type: data.item_type as string,
        item_id: data.item_id as string,
      });
    }
    return { id: `pin-${Date.now()}` } as unknown;
  },
  deletePin: async (itemType: string, itemId: string) => {
    if (inTauri) {
      await invoke("delete_pin", { item_type: itemType, item_id: itemId });
      return;
    }
  },
  reorderPins: async (data: { items: { id: string; position: number }[] }) => {
    if (inTauri) {
      await invoke("reorder_pins", { workspace_id: WS, items: data });
      return;
    }
  },

  createProperty: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_property", {
        workspace_id: WS,
        name: data.name as string,
        prop_type: data.type as string,
        description: (data.description as string) ?? null,
        icon: (data.icon as string) ?? null,
        config: (data.config as Record<string, unknown>) ?? null,
      });
    }
    return { id: `prop-${Date.now()}`, name: data.name } as unknown;
  },
  updateProperty: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_property", {
          id,
          name: (data.name as string) ?? null,
          description: (data.description as string) ?? null,
          icon: (data.icon as string) ?? null,
          config: (data.config as Record<string, unknown>) ?? null,
          archived: (data.archived as boolean) ?? null,
        }),
        `property ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  setIssueProperty: async (issueId: string, propertyId: string, value: unknown) => {
    if (inTauri) return invoke("set_issue_property", { issue_id: issueId, property_id: propertyId, value });
    return { issue_id: issueId, property_id: propertyId, value } as unknown;
  },
  unsetIssueProperty: async (issueId: string, propertyId: string) => {
    if (inTauri) {
      await invoke("unset_issue_property", { issue_id: issueId, property_id: propertyId });
      return;
    }
  },

  attachLabel: async (issueId: string, labelId: string) => {
    if (inTauri) return invoke("attach_label", { issue_id: issueId, label_id: labelId });
    return { issue_id: issueId, label_id: labelId } as unknown;
  },
  detachLabel: async (issueId: string, labelId: string) => {
    if (inTauri) {
      await invoke("detach_label", { issue_id: issueId, label_id: labelId });
      return;
    }
  },
  attachLabelToResource: async (resourceType: string, resourceId: string, labelId: string) => {
    if (inTauri) {
      return invoke("attach_label_to_resource", {
        resource_type: resourceType,
        resource_id: resourceId,
        label_id: labelId,
      });
    }
    return { resource_type: resourceType, resource_id: resourceId, label_id: labelId } as unknown;
  },
  detachLabelFromResource: async (resourceType: string, resourceId: string, labelId: string) => {
    if (inTauri) {
      await invoke("detach_label_from_resource", {
        resource_type: resourceType,
        resource_id: resourceId,
        label_id: labelId,
      });
      return;
    }
  },

  createProjectResource: async (projectId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_project_resource", {
        project_id: projectId,
        resource_type: data.resource_type as string,
        resource_ref: (data.resource_ref as Record<string, unknown>) ?? {},
        label: (data.label as string) ?? null,
        position: (data.position as number) ?? null,
      });
    }
    return { id: `pr-${Date.now()}` } as unknown;
  },
  updateProjectResource: async (projectId: string, resourceId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_project_resource", {
          project_id: projectId,
          resource_id: resourceId,
          resource_ref: (data.resource_ref as Record<string, unknown>) ?? null,
          label: (data.label as string) ?? null,
          position: (data.position as number) ?? null,
        }),
        `project resource ${resourceId}`,
      );
    }
    return { id: resourceId, ...data } as unknown;
  },
  deleteProjectResource: async (projectId: string, resourceId: string) => {
    if (inTauri) {
      await invoke("delete_project_resource", { project_id: projectId, resource_id: resourceId });
      return;
    }
  },

  addSquadMember: async (squadId: string, data: { member_type: string; member_id: string; role?: string }) => {
    if (inTauri) {
      return invoke("add_squad_member", {
        squad_id: squadId,
        member_type: data.member_type,
        member_id: data.member_id,
      });
    }
    return { squad_id: squadId, ...data } as unknown;
  },
  removeSquadMember: async (squadId: string, data: { member_type: string; member_id: string }) => {
    if (inTauri) {
      await invoke("remove_squad_member", {
        squad_id: squadId,
        member_type: data.member_type,
        member_id: data.member_id,
      });
      return;
    }
  },
  updateSquadMemberRole: async (squadId: string, data: { member_type: string; member_id: string; role: string }) => {
    if (inTauri) {
      return invoke("update_squad_member_role", {
        squad_id: squadId,
        member_type: data.member_type,
        member_id: data.member_id,
        role: data.role,
      });
    }
    return { squad_id: squadId, ...data } as unknown;
  },

  markInboxRead: async (id: string) => {
    if (inTauri) return invoke("mark_inbox_read", { id });
    return { id } as unknown;
  },
  markInboxUnread: async (id: string) => {
    if (inTauri) return invoke("mark_inbox_unread", { id });
    return { id } as unknown;
  },
  archiveInbox: async (id: string) => {
    if (inTauri) return invoke("archive_inbox", { id });
    return { id } as unknown;
  },
  unarchiveInbox: async (id: string) => {
    if (inTauri) return invoke("unarchive_inbox", { id });
    return { id } as unknown;
  },
  markAllInboxRead: async () => {
    if (inTauri) return invoke("mark_all_inbox_read");
    return { marked: 0 } as unknown;
  },
  archiveAllInbox: async () => {
    if (inTauri) return invoke("archive_all_inbox");
    return { archived: 0 } as unknown;
  },
  archiveAllReadInbox: async () => {
    if (inTauri) return invoke("archive_all_read_inbox");
    return { archived: 0 } as unknown;
  },
  archiveCompletedInbox: async () => {
    if (inTauri) return invoke("archive_completed_inbox");
    return { archived: 0 } as unknown;
  },
  listArchivedInbox: async () => {
    if (inTauri) return invoke("list_archived_inbox", { workspace_id: WS });
    return [] as unknown[];
  },
  retrySourceContextQuickCreate: async (taskId: string) => {
    if (inTauri) return invoke("retry_source_context_quick_create", { task_id: taskId });
    return { task_id: taskId } as unknown;
  },

  createChatSession: async (data: { agent_id: string; title?: string; project_id?: string | null }, workspaceSlug?: string) => {
    if (inTauri) {
      return invoke("create_chat_session", {
        workspace_id: workspaceSlug ?? WS,
        agent_id: data.agent_id,
        title: data.title ?? null,
        project_id: data.project_id ?? null,
      });
    }
    return { id: `cs-${Date.now()}`, agent_id: data.agent_id } as unknown;
  },
  updateChatSession: async (id: string, data: { title?: string; project_id?: string | null }) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_chat_session", {
          id,
          title: (data as { title?: string }).title ?? null,
          project_id: (data as { project_id?: string | null }).project_id ?? null,
        }),
        `chat session ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  deleteChatSession: async (id: string) => {
    if (inTauri) {
      await invoke("delete_chat_session", { id });
      return;
    }
  },
  getChatSession: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_chat_session", { id }), `chat session ${id}`);
    return { id } as unknown;
  },
  sendChatMessage: async (sessionId: string, content: string, attachmentIds?: string[]) => {
    if (inTauri) {
      return invoke("send_chat_message", {
        session_id: sessionId,
        content,
        attachment_ids: attachmentIds ?? null,
      });
    }
    return { message_id: `m-${Date.now()}`, task_id: `t-${Date.now()}`, created_at: new Date().toISOString() } as unknown;
  },
  setChatSessionArchived: async (id: string, archived: boolean) => {
    if (inTauri) return invoke("set_chat_session_archived", { id, archived });
    return { id, archived } as unknown;
  },
  setChatSessionPinned: async (id: string, pinned: boolean) => {
    if (inTauri) return invoke("set_chat_session_pinned", { id, pinned });
    return { id, pinned } as unknown;
  },
  markChatSessionRead: async (sessionId: string) => {
    if (inTauri) return invoke("mark_chat_session_read", { session_id: sessionId });
    return { id: sessionId } as unknown;
  },
  clearQueuedChatTasks: async (sessionId: string) => {
    if (inTauri) return invoke("clear_queued_chat_tasks", { session_id: sessionId });
    return { cancelled: 0 } as unknown;
  },
  cancelTaskById: async (taskId: string, options?: { queuedAction?: "edit" | "remove"; sessionId?: string }) => {
    if (inTauri) {
      return invoke("cancel_task_by_id", {
        task_id: taskId,
        queued_action: options?.queuedAction ?? null,
        session_id: options?.sessionId ?? null,
      });
    }
    return { task_id: taskId, cancelled: true } as unknown;
  },
  listChatMessagesPage: async (sessionId: string, params: { before?: { created_at: string; id: string } | null; limit?: number } = {}) => {
    if (inTauri) {
      return invoke("list_chat_messages_page", {
        session_id: sessionId,
        before: params.before ?? null,
        limit: params.limit ?? null,
      });
    }
    return { messages: [], limit: 50, has_more: false, next_cursor: null } as unknown;
  },
  listTaskMessages: async (taskId: string) => {
    if (inTauri) return invoke("list_task_messages", { task_id: taskId });
    return [] as unknown[];
  },
  pinChatAgent: async (agentId: string) => {
    if (inTauri) return invoke("pin_chat_agent", { agent_id: agentId });
    return { agent_id: agentId, pinned: true } as unknown;
  },
  unpinChatAgent: async (agentId: string) => {
    if (inTauri) return invoke("unpin_chat_agent", { agent_id: agentId });
    return { agent_id: agentId, pinned: false } as unknown;
  },
  listChatPinnedAgents: async () => {
    if (inTauri) return invoke("list_chat_pinned_agents");
    return [] as unknown[];
  },
  regenerateChatQuickActions: async (sessionId: string, messageId: string) => {
    if (inTauri) return invoke("regenerate_chat_quick_actions", { session_id: sessionId, message_id: messageId });
    return { quick_actions: [] } as unknown;
  },
  consumeChatDraftRestore: async (sessionId: string, restoreId: string) => {
    if (inTauri) return invoke("consume_chat_draft_restore", { session_id: sessionId, restore_id: restoreId });
    return { consumed: true } as unknown;
  },

  createAgentBuilderSession: async (data: { runtime_id: string; model?: string }) => {
    if (inTauri) {
      return invoke("create_agent_builder_session", {
        workspace_id: WS,
        runtime_id: data.runtime_id,
        model: data.model ?? null,
      });
    }
    return { id: `bs-${Date.now()}` } as unknown;
  },
  switchAgentBuilderRuntime: async (sessionId: string, data: { runtime_id: string }) => {
    if (inTauri) return invoke("switch_agent_builder_runtime", { session_id: sessionId, runtime_id: data.runtime_id });
    return { id: sessionId } as unknown;
  },

  resolveComment: async (commentId: string) => {
    if (inTauri) return invoke("resolve_comment", { comment_id: commentId });
    return { id: commentId, resolved: true } as unknown;
  },
  unresolveComment: async (commentId: string) => {
    if (inTauri) return invoke("unresolve_comment", { comment_id: commentId });
    return { id: commentId, resolved: false } as unknown;
  },
  addReaction: async (commentId: string, emoji: string) => {
    if (inTauri) return invoke("add_reaction", { comment_id: commentId, emoji });
    return { comment_id: commentId, emoji } as unknown;
  },
  removeReaction: async (commentId: string, emoji: string) => {
    if (inTauri) {
      await invoke("remove_reaction", { comment_id: commentId, emoji });
      return;
    }
  },
  addIssueReaction: async (issueId: string, emoji: string) => {
    if (inTauri) return invoke("add_issue_reaction", { issue_id: issueId, emoji });
    return { issue_id: issueId, emoji } as unknown;
  },
  removeIssueReaction: async (issueId: string, emoji: string) => {
    if (inTauri) {
      await invoke("remove_issue_reaction", { issue_id: issueId, emoji });
      return;
    }
  },
  subscribeToIssue: async (issueId: string, userId?: string, userType?: string) => {
    if (inTauri) {
      return invoke("subscribe_to_issue", {
        issue_id: issueId,
        user_id: userId ?? null,
        user_type: userType ?? null,
      });
    }
    return { issue_id: issueId, subscribed: true } as unknown;
  },
  unsubscribeFromIssue: async (issueId: string, userId?: string, userType?: string) => {
    if (inTauri) {
      await invoke("unsubscribe_from_issue", {
        issue_id: issueId,
        user_id: userId ?? null,
        user_type: userType ?? null,
      });
      return;
    }
  },
  getCommentSubIssuePreview: async (anchorCommentId: string) => {
    if (inTauri) return invoke("get_comment_sub_issue_preview", { anchor_comment_id: anchorCommentId });
    return { anchor_comment_id: anchorCommentId, title: "新子 issue" } as unknown;
  },
  createCommentSubIssue: async (anchorCommentId: string, data: { issue: Record<string, unknown> }) => {
    if (inTauri) {
      return invoke("create_comment_sub_issue", {
        anchor_comment_id: anchorCommentId,
        issue: data.issue,
      });
    }
    return { id: `i-${Date.now()}` } as unknown;
  },

  createQuickAction: async (data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_quick_action", {
        workspace_id: WS,
        name: data.name as string,
        description: (data.description as string) ?? null,
        assignee_type: data.assignee_type as string,
        assignee_id: data.assignee_id as string,
        prompt: data.prompt as string,
        visibility: (data.visibility as string) ?? null,
      });
    }
    return { id: `qa-${Date.now()}`, name: data.name } as unknown;
  },
  updateQuickAction: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_quick_action", {
          id,
          name: (data.name as string) ?? null,
          description: (data.description as string) ?? null,
          assignee_type: (data.assignee_type as string) ?? null,
          assignee_id: (data.assignee_id as string) ?? null,
          prompt: (data.prompt as string) ?? null,
          visibility: (data.visibility as string) ?? null,
          status: (data.status as string) ?? null,
        }),
        `quick action ${id}`,
      );
    }
    return { id, ...data } as unknown;
  },
  deleteQuickAction: async (id: string) => {
    if (inTauri) {
      await invoke("delete_quick_action", { id });
      return;
    }
  },
  runQuickAction: async (issueId: string, quickActionId: string) => {
    if (inTauri) return invoke("run_quick_action", { issue_id: issueId, quick_action_id: quickActionId });
    return { task_id: `t-${Date.now()}` } as unknown;
  },

  batchUpdateIssues: async (issueIds: string[], updates: Record<string, unknown>) => {
    if (inTauri) return invoke("batch_update_issues", { issue_ids: issueIds, updates });
    return { updated: 0 } as unknown;
  },
  batchDeleteIssues: async (issueIds: string[]) => {
    if (inTauri) return invoke("batch_delete_issues", { issue_ids: issueIds });
    return { deleted: 0 } as unknown;
  },
  rerunIssue: async (issueId: string, taskId?: string) => {
    if (inTauri) return invoke("rerun_issue", { issue_id: issueId, task_id: taskId ?? null });
    return { task_id: `t-${Date.now()}` } as unknown;
  },
  retryTaskSupplement: async (issueId: string, taskId: string, commentId: string) => {
    if (inTauri) {
      return invoke("retry_task_supplement", {
        issue_id: issueId,
        task_id: taskId,
        comment_id: commentId,
      });
    }
    return { task_id: taskId } as unknown;
  },

  searchIssues: async (params: { q: string; limit?: number; offset?: number; include_closed?: boolean }) => {
    if (inTauri) {
      return invoke("search_issues", {
        q: params.q,
        limit: params.limit ?? null,
        offset: params.offset ?? null,
        include_closed: params.include_closed ?? null,
      });
    }
    return { issues: [], total: 0 } as unknown;
  },
  searchProjects: async (params: { q: string; limit?: number; offset?: number; include_closed?: boolean }) => {
    if (inTauri) {
      return invoke("search_projects", {
        q: params.q,
        limit: params.limit ?? null,
        offset: params.offset ?? null,
        include_closed: params.include_closed ?? null,
      });
    }
    return { projects: [], total: 0 } as unknown;
  },
  getSearchIndexChanges: async (params: { workspaceSlug: string; cursor: string; limit?: number }) => {
    if (inTauri) {
      return invoke("get_search_index_changes", {
        workspace_id: params.workspaceSlug ?? WS,
        cursor: params.cursor,
        limit: params.limit ?? null,
      });
    }
    return { changes: [], next_cursor: params.cursor } as unknown;
  },

  listWorkspaceWakeups: async (filters: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("list_workspace_wakeups", {
        workspace_id: WS,
        scope: (filters.scope as string) ?? null,
        kind: (filters.kind as string) ?? null,
        source: (filters.source as string) ?? null,
        search: (filters.search as string) ?? null,
        agent_id: (filters.agent_id as string) ?? null,
        offset: (filters.offset as number) ?? null,
        limit: (filters.limit as number) ?? null,
      });
    }
    return { wakeups: [], total: 0 } as unknown;
  },
  listWorkspaceSystemWakeups: async () => {
    if (inTauri) return invoke("list_workspace_system_wakeups", { workspace_id: WS });
    return [] as unknown[];
  },
  updateIssueSystemWakeup: async (issueId: string, rule: string, input: { enabled?: boolean; instruction?: string }) => {
    if (inTauri) {
      return invoke("update_issue_system_wakeup", {
        issue_id: issueId,
        rule,
        enabled: input.enabled ?? null,
        instruction: input.instruction ?? null,
      });
    }
    return { issue_id: issueId, rule } as unknown;
  },
  updateWorkspaceSystemWakeup: async (rule: string, input: { enabled?: boolean; instruction?: string }) => {
    if (inTauri) {
      return invoke("update_workspace_system_wakeup", {
        workspace_id: WS,
        rule,
        enabled: input.enabled ?? null,
        instruction: input.instruction ?? null,
      });
    }
    return { rule } as unknown;
  },
  enableIssueWakeup: async (issueId: string, wakeupId: string, input: { revision: number; at?: string; rearm?: boolean }) => {
    if (inTauri) {
      return invoke("enable_issue_wakeup", {
        issue_id: issueId,
        wakeup_id: wakeupId,
        revision: input.revision ?? null,
        at: input.at ?? null,
        rearm: input.rearm ?? null,
      });
    }
    return { id: wakeupId, enabled: true } as unknown;
  },
  disableIssueWakeup: async (issueId: string, wakeupId: string) => {
    if (inTauri) {
      await invoke("disable_issue_wakeup", { issue_id: issueId, wakeup_id: wakeupId });
      return;
    }
  },
  editIssueWakeupInstruction: async (issueId: string, wakeupId: string, input: { instruction: string; expected_instruction: string; revision: number }) => {
    if (inTauri) {
      return invoke("edit_issue_wakeup_instruction", {
        issue_id: issueId,
        wakeup_id: wakeupId,
        instruction: input.instruction,
        expected_instruction: input.expected_instruction,
        revision: input.revision ?? null,
      });
    }
    return { id: wakeupId } as unknown;
  },
  listIssueWakeupRuns: async (issueId: string, wakeupId: string) => {
    if (inTauri) return invoke("list_issue_wakeup_runs", { issue_id: issueId, wakeup_id: wakeupId });
    return [] as unknown[];
  },
  previewIssueTrigger: async (params: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("preview_issue_trigger", {
        issue_ids: (params.issueIds as string[]) ?? null,
        is_create: (params.isCreate as boolean) ?? null,
        assignee_type: (params.assigneeType as string) ?? null,
        assignee_id: (params.assigneeId as string) ?? null,
        status: (params.status as string) ?? null,
      });
    }
    return { matched_issues: [], total: 0 } as unknown;
  },
  previewCommentTriggers: async (issueId: string, content: string, parentId?: string, editingCommentId?: string) => {
    if (inTauri) {
      return invoke("preview_comment_triggers", {
        issue_id: issueId,
        content,
        parent_id: parentId ?? null,
        editing_comment_id: editingCommentId ?? null,
      });
    }
    return { triggers: [] } as unknown;
  },

  createAutopilotTrigger: async (autopilotId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_autopilot_trigger", {
        autopilot_id: autopilotId,
        kind: data.kind as string,
        cron_expression: (data.cron_expression as string) ?? null,
        timezone: (data.timezone as string) ?? null,
        label: (data.label as string) ?? null,
        event_filters: (data.event_filters as unknown[]) ?? null,
      });
    }
    return { id: `tr-${Date.now()}` } as unknown;
  },
  updateAutopilotTrigger: async (autopilotId: string, triggerId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_autopilot_trigger", {
          autopilot_id: autopilotId,
          trigger_id: triggerId,
          enabled: (data.enabled as boolean) ?? null,
          cron_expression: (data.cron_expression as string) ?? null,
          timezone: (data.timezone as string) ?? null,
          label: (data.label as string) ?? null,
          event_filters: (data.event_filters as unknown[]) ?? null,
        }),
        `autopilot trigger ${triggerId}`,
      );
    }
    return { id: triggerId, ...data } as unknown;
  },
  deleteAutopilotTrigger: async (autopilotId: string, triggerId: string) => {
    if (inTauri) {
      await invoke("delete_autopilot_trigger", { autopilot_id: autopilotId, trigger_id: triggerId });
      return;
    }
  },
  triggerAutopilot: async (id: string) => {
    if (inTauri) return invoke("trigger_autopilot", { id });
    return { autopilot_id: id } as unknown;
  },
  listAutopilotDeliveries: async (autopilotId: string, params?: { limit?: number; offset?: number }) => {
    if (inTauri) {
      return invoke("list_autopilot_deliveries", {
        autopilot_id: autopilotId,
        limit: params?.limit ?? null,
        offset: params?.offset ?? null,
      });
    }
    return { deliveries: [], total: 0 } as unknown;
  },
  getAutopilotRun: async (autopilotId: string, runId: string) => {
    if (inTauri) {
      return orThrow(
        await invoke("get_autopilot_run", { autopilot_id: autopilotId, run_id: runId }),
        `autopilot run ${runId}`,
      );
    }
    return { id: runId } as unknown;
  },
  getAutopilotDelivery: async (autopilotId: string, deliveryId: string) => {
    if (inTauri) {
      return orThrow(
        await invoke("get_autopilot_delivery", { autopilot_id: autopilotId, delivery_id: deliveryId }),
        `autopilot delivery ${deliveryId}`,
      );
    }
    return { id: deliveryId } as unknown;
  },

  setAgentSkills: async (agentId: string, data: { skill_ids: string[] }) => {
    if (inTauri) return invoke("set_agent_skills", { agent_id: agentId, skill_ids: data.skill_ids });
    return { agent_id: agentId, skill_ids: data.skill_ids } as unknown;
  },
  addAgentSkills: async (agentId: string, data: { skill_ids: string[] }) => {
    if (inTauri) return invoke("add_agent_skills", { agent_id: agentId, skill_ids: data.skill_ids });
    return { agent_id: agentId, skill_ids: data.skill_ids } as unknown;
  },
  removeAgentSkill: async (agentId: string, skillId: string) => {
    if (inTauri) return invoke("remove_agent_skill", { agent_id: agentId, skill_id: skillId });
    return { agent_id: agentId } as unknown;
  },
  setAgentSkillEnabled: async (agentId: string, skillId: string, enabled: boolean) => {
    if (inTauri) {
      return invoke("set_agent_skill_enabled", { agent_id: agentId, skill_id: skillId, enabled });
    }
    return { agent_id: agentId, skill_id: skillId, enabled } as unknown;
  },
  setAgentRuntimeSkillEnabled: async (agentId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("set_agent_runtime_skill_enabled", {
        agent_id: agentId,
        runtime_id: data.runtime_id as string,
        root: data.root as string,
        key: data.key as string,
        name: data.name as string,
        plugin: (data.plugin as string) ?? null,
        enabled: data.enabled as boolean,
      });
    }
    return { agent_id: agentId } as unknown;
  },
  updateAgentEnv: async (id: string, data: { custom_env: Record<string, string> }) => {
    if (inTauri) return invoke("update_agent_env", { id, custom_env: data.custom_env });
    return { agent_id: id, custom_env: data.custom_env } as unknown;
  },
  getAgentEnv: async (id: string) => {
    if (inTauri) return invoke("get_agent_env", { id });
    return { agent_id: id, custom_env: {} } as unknown;
  },

  uploadFile: async (file: File, opts?: { issueId?: string; commentId?: string; chatSessionId?: string }) => {
    if (inTauri) {
      const buf = await file.arrayBuffer();
      let binary = "";
      const bytes = new Uint8Array(buf);
      const chunk = 0x8000;
      for (let i = 0; i < bytes.length; i += chunk) {
        binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk) as unknown as number[]);
      }
      const dataBase64 = btoa(binary);
      return invoke("upload_file", {
        filename: file.name,
        mime_type: file.type || "application/octet-stream",
        data_base64: dataBase64,
        issue_id: opts?.issueId ?? null,
        comment_id: opts?.commentId ?? null,
        chat_session_id: opts?.chatSessionId ?? null,
      });
    }
    return { id: `att-${Date.now()}`, filename: file.name, mime_type: file.type } as unknown;
  },
  getAttachmentBlob: async (id: string) => {
    if (inTauri) {
      const res = (await invoke("get_attachment_blob", { id })) as { data_base64: string; mime_type: string };
      const binary = atob(res.data_base64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      return new Blob([bytes], { type: res.mime_type });
    }
    return new Blob([]);
  },
  getAttachmentForDownload: async (id: string) => {
    if (inTauri) {
      const att = (await invoke("get_attachment", { id })) as Record<string, unknown>;
      const blobRes = (await invoke("get_attachment_blob", { id })) as { data_base64: string; mime_type: string };
      const binary = atob(blobRes.data_base64);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const blob = new Blob([bytes], { type: blobRes.mime_type });
      const url = URL.createObjectURL(blob);
      return { ...att, download_url: url, url } as unknown;
    }
    return { id, download_url: "", url: "" } as unknown;
  },
  getAttachmentTextContent: async (id: string) => {
    if (inTauri) {
      const res = (await invoke("get_attachment_text_content", { id })) as { content: string; mime_type: string };
      return { text: res.content, originalContentType: res.mime_type } as unknown;
    }
    return { text: "", originalContentType: "" } as unknown;
  },

  listIssueTableGroups: async (params: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("list_issue_table_groups", {
        workspace_id: WS,
        group_by: (params.group_by as string) ?? null,
      });
    }
    return { groups: [] } as unknown;
  },
  listChildrenByParents: async (parentIds: string[]) => {
    if (inTauri) return invoke("list_children_by_parents", { parent_ids: parentIds });
    return { children: {} } as unknown;
  },

  getNotificationPreferences: async (workspaceSlug?: string) => {
    if (inTauri) return invoke("get_notification_preferences", { workspace_id: workspaceSlug ?? WS });
    return {} as unknown;
  },
  updateNotificationPreferences: async (preferences: Record<string, unknown>, workspaceSlug?: string) => {
    if (inTauri) {
      return invoke("update_notification_preferences", {
        workspace_id: workspaceSlug ?? WS,
        preferences,
      });
    }
    return preferences as unknown;
  },

  getUpdateResult: async (runtimeId: string, updateId: string) => {
    if (inTauri) return invoke("get_update_result", { runtime_id: runtimeId, update_id: updateId });
    return { status: "not_supported" } as unknown;
  },
  initiateUpdate: async (runtimeId: string, targetVersion: string) => {
    if (inTauri) {
      return invoke("initiate_update", { runtime_id: runtimeId, target_version: targetVersion });
    }
    return { status: "not_supported" } as unknown;
  },

  registerSlackBYO: async (workspaceId: string, agentId: string, body: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("register_slack_byo", {
        workspace_id: workspaceId,
        config: { agent_id: agentId, ...body },
      });
    }
    return { provider: "slack", configured: true } as unknown;
  },
  registerTelegramBot: async (workspaceId: string, agentId: string, body: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("register_telegram_bot", {
        workspace_id: workspaceId,
        config: { agent_id: agentId, ...body },
      });
    }
    return { provider: "telegram", configured: true } as unknown;
  },
  registerDingTalkBYO: async (workspaceId: string, agentId: string, body: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("register_dingtalk_byo", {
        workspace_id: workspaceId,
        config: { agent_id: agentId, ...body },
      });
    }
    return { provider: "dingtalk", configured: true } as unknown;
  },
  registerWecomBYO: async (workspaceId: string, agentId: string, body: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("register_wecom_byo", {
        workspace_id: workspaceId,
        config: { agent_id: agentId, ...body },
      });
    }
    return { provider: "wecom", configured: true } as unknown;
  },
  deleteSlackInstallation: async (workspaceId: string, _installationId: string) => {
    if (inTauri) {
      await invoke("delete_channel_installation", { workspace_id: workspaceId, provider: "slack" });
      return;
    }
  },
  deleteTelegramInstallation: async (workspaceId: string, _installationId: string) => {
    if (inTauri) {
      await invoke("delete_channel_installation", { workspace_id: workspaceId, provider: "telegram" });
      return;
    }
  },
  deleteDingTalkInstallation: async (workspaceId: string, _installationId: string) => {
    if (inTauri) {
      await invoke("delete_channel_installation", { workspace_id: workspaceId, provider: "dingtalk" });
      return;
    }
  },
  deleteWecomInstallation: async (workspaceId: string, _installationId: string) => {
    if (inTauri) {
      await invoke("delete_channel_installation", { workspace_id: workspaceId, provider: "wecom" });
      return;
    }
  },
  deleteLarkInstallation: async (workspaceId: string, _installationId: string) => {
    if (inTauri) {
      await invoke("delete_channel_installation", { workspace_id: workspaceId, provider: "lark" });
      return;
    }
  },

  // ── 第三批写路径（write3.rs） ──────────────────────────────────────────
  archiveAgent: async (id: string) => {
    if (inTauri) return invoke("archive_agent", { id });
    return { id, archived_at: new Date().toISOString() } as unknown;
  },
  restoreAgent: async (id: string) => {
    if (inTauri) return invoke("restore_agent", { id });
    return { id, archived_at: null } as unknown;
  },
  deleteAttachment: async (id: string) => {
    if (inTauri) {
      await invoke("delete_attachment", { id });
      return;
    }
  },
  getLabel: async (id: string) => {
    if (inTauri) return orThrow(await invoke("get_label", { id }), `label ${id}`);
    return { id } as unknown;
  },
  getUnreadInboxCount: async () => {
    if (inTauri) return invoke("get_unread_inbox_count");
    return { count: 0 } as unknown;
  },
  listArchivedInboxPage: async (filters: Record<string, unknown>, options: { cursor?: string | null; groupId?: string } = {}) => {
    if (inTauri) {
      return invoke("list_archived_inbox_page", {
        filters,
        cursor: options.cursor ?? null,
        group_id: options.groupId ?? null,
      });
    }
    return { items: [], nextCursor: null, hasMore: false } as unknown;
  },
  getArchivedInboxFacets: async (filters: Record<string, unknown>) => {
    if (inTauri) return invoke("get_archived_inbox_facets", { filters });
    return { statuses: {}, priorities: {}, actors: {}, unreadCount: 0 } as unknown;
  },
  listGroupedIssues: async (params: Record<string, unknown>) => {
    if (inTauri) return invoke("list_grouped_issues", { params });
    return { groups: [] } as unknown;
  },
  quickCreateIssue: async (data: Record<string, unknown>) => {
    if (inTauri) return invoke("quick_create_issue", { data });
    return { task_id: "" } as unknown;
  },
  renderQuickAction: async (issueId: string, quickActionId: string) => {
    if (inTauri) return invoke("render_quick_action", { issue_id: issueId, quick_action_id: quickActionId });
    return "" as unknown;
  },
  replayAutopilotDelivery: async (autopilotId: string, deliveryId: string) => {
    if (inTauri) {
      return invoke("replay_autopilot_delivery", { autopilot_id: autopilotId, delivery_id: deliveryId });
    }
    return { id: `d-${Date.now()}` } as unknown;
  },
  prioritizeQueuedChatTask: async (sessionId: string, taskId: string) => {
    if (inTauri) {
      return invoke("prioritize_queued_chat_task", { session_id: sessionId, task_id: taskId });
    }
    return { task_id: taskId } as unknown;
  },
  unsubscribeFromIssueSubtree: async (issueId: string, userId?: string, userType?: string) => {
    if (inTauri) {
      await invoke("unsubscribe_from_issue_subtree", {
        issue_id: issueId,
        user_id: userId ?? null,
        user_type: userType ?? null,
      });
      return;
    }
  },
  createMember: async (workspaceId: string, data: Record<string, unknown>) => {
    if (inTauri) return invoke("create_member", { workspace_id: workspaceId, data });
    return { id: `m-${Date.now()}`, workspace_id: workspaceId } as unknown;
  },
  updateMember: async (workspaceId: string, memberId: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_member", { workspace_id: workspaceId, member_id: memberId, data }),
        `member ${memberId}`,
      );
    }
    return { id: memberId, ...data } as unknown;
  },
  deleteMember: async (workspaceId: string, memberId: string) => {
    if (inTauri) {
      await invoke("delete_member", { workspace_id: workspaceId, member_id: memberId });
      return;
    }
  },
  updateWorkspace: async (id: string, data: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(await invoke("update_workspace", { id, data }), `workspace ${id}`);
    }
    return { id, ...data } as unknown;
  },
  deleteWorkspace: async (workspaceId: string) => {
    if (inTauri) {
      await invoke("delete_workspace", { workspace_id: workspaceId });
      return;
    }
  },
  updateRuntime: async (runtimeId: string, patch: Record<string, unknown>) => {
    if (inTauri) return invoke("update_runtime", { runtime_id: runtimeId, patch });
    return { id: runtimeId, ...patch } as unknown;
  },
  deleteRuntime: async (runtimeId: string) => {
    if (inTauri) {
      await invoke("delete_runtime", { runtime_id: runtimeId });
      return;
    }
  },
  unbindAgentsAndDeleteRuntime: async (runtimeId: string, expectedActiveAgentIds: string[]) => {
    if (inTauri) {
      return invoke("unbind_agents_and_delete_runtime", {
        runtime_id: runtimeId,
        expected_active_agent_ids: expectedActiveAgentIds,
      });
    }
    return { status: "deleted", agents_unbound: 0, tasks_cancelled: 0 } as unknown;
  },
  getRuntimeProfile: async (workspaceId: string, profileId: string) => {
    if (inTauri) {
      return orThrow(
        await invoke("get_runtime_profile", { workspace_id: workspaceId, profile_id: profileId }),
        `runtime profile ${profileId}`,
      );
    }
    return { id: profileId } as unknown;
  },
  createRuntimeProfile: async (workspaceId: string, body: Record<string, unknown>) => {
    if (inTauri) return invoke("create_runtime_profile", { workspace_id: workspaceId, body });
    return { id: `rp-${Date.now()}`, ...body } as unknown;
  },
  updateRuntimeProfile: async (workspaceId: string, profileId: string, patch: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_runtime_profile", { workspace_id: workspaceId, profile_id: profileId, patch }),
        `runtime profile ${profileId}`,
      );
    }
    return { id: profileId, ...patch } as unknown;
  },
  deleteRuntimeProfile: async (workspaceId: string, profileId: string) => {
    if (inTauri) {
      await invoke("delete_runtime_profile", { workspace_id: workspaceId, profile_id: profileId });
      return;
    }
  },
  listAgentMcpServers: async (agentId: string) => {
    if (inTauri) return invoke("list_agent_mcp_servers", { agent_id: agentId });
    return [] as unknown[];
  },
  addAgentMcpServer: async (agentId: string, serverId: string) => {
    if (inTauri) return invoke("add_agent_mcp_server", { agent_id: agentId, server_id: serverId });
    return [] as unknown[];
  },
  removeAgentMcpServer: async (agentId: string, serverId: string) => {
    if (inTauri) return invoke("remove_agent_mcp_server", { agent_id: agentId, server_id: serverId });
    return [] as unknown[];
  },
  setAgentMcpServerEnabled: async (agentId: string, serverId: string, enabled: boolean) => {
    if (inTauri) {
      return invoke("set_agent_mcp_server_enabled", { agent_id: agentId, server_id: serverId, enabled });
    }
    return [] as unknown[];
  },
  listWorkspaceMcpServers: async (workspaceId: string) => {
    if (inTauri) return invoke("list_workspace_mcp_servers", { workspace_id: workspaceId });
    return [] as unknown[];
  },
  createWorkspaceMcpServer: async (workspaceId: string, name: string, config: Record<string, unknown>) => {
    if (inTauri) {
      return invoke("create_workspace_mcp_server", { workspace_id: workspaceId, name, config });
    }
    return { id: `mcp-${Date.now()}`, name } as unknown;
  },
  updateWorkspaceMcpServer: async (workspaceId: string, serverId: string, update: Record<string, unknown>) => {
    if (inTauri) {
      return orThrow(
        await invoke("update_workspace_mcp_server", { workspace_id: workspaceId, server_id: serverId, update }),
        `mcp server ${serverId}`,
      );
    }
    return { id: serverId, ...update } as unknown;
  },
  deleteWorkspaceMcpServer: async (workspaceId: string, serverId: string) => {
    if (inTauri) {
      await invoke("delete_workspace_mcp_server", { workspace_id: workspaceId, server_id: serverId });
      return;
    }
  },
  listAgentSkills: async (agentId: string) => {
    if (inTauri) return invoke("list_agent_skills", { agent_id: agentId });
    return [] as unknown[];
  },
  importSkill: async (data: { url: string }) => {
    if (inTauri) return invoke("import_skill", { data });
    return { id: `sk-${Date.now()}`, name: "imported" } as unknown;
  },
  refreshSkill: async (id: string) => {
    if (inTauri) return orThrow(await invoke("refresh_skill", { id }), `skill ${id}`);
    return { id } as unknown;
  },
  importSkillArchive: async (file: File, onConflict?: "fail" | "overwrite" | "rename" | "skip") => {
    if (inTauri) {
      const buf = await file.arrayBuffer();
      const bytes = new Uint8Array(buf);
      let binary = "";
      const chunk = 0x8000;
      for (let i = 0; i < bytes.length; i += chunk) {
        binary += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk) as unknown as number[]);
      }
      return invoke("import_skill_archive", {
        data_base64: btoa(binary),
        filename: file.name,
        on_conflict: onConflict ?? null,
      });
    }
    return { id: `sk-${Date.now()}`, name: file.name } as unknown;
  },
  cronPreview: async (params: { expr: string; tz: string }) => {
    if (inTauri) return invoke("cron_preview", { expr: params.expr, tz: params.tz });
    return { next_runs: [] } as unknown;
  },
  getRuntimeTaskActivity: async (runtimeId: string, params?: { tz?: string }) => {
    if (inTauri) return invoke("get_runtime_task_activity", { runtime_id: runtimeId, tz: params?.tz ?? null });
    return [] as unknown[];
  },
  getRuntimeUsage: async (runtimeId: string, params?: { days?: number; tz?: string }) => {
    if (inTauri) {
      return invoke("get_runtime_usage", {
        runtime_id: runtimeId,
        days: params?.days ?? null,
        tz: params?.tz ?? null,
      });
    }
    return [] as unknown[];
  },
  getRuntimeUsageByAgent: async (runtimeId: string, params?: { days?: number; tz?: string }) => {
    if (inTauri) {
      return invoke("get_runtime_usage_by_agent", {
        runtime_id: runtimeId,
        days: params?.days ?? null,
        tz: params?.tz ?? null,
      });
    }
    return [] as unknown[];
  },
  getRuntimeUsageByHour: async (runtimeId: string, params?: { days?: number; tz?: string }) => {
    if (inTauri) {
      return invoke("get_runtime_usage_by_hour", {
        runtime_id: runtimeId,
        days: params?.days ?? null,
        tz: params?.tz ?? null,
      });
    }
    return [] as unknown[];
  },
  getAutopilotQuotaUsage: async () => {
    if (inTauri) return invoke("get_autopilot_quota_usage");
    return { action: "off", reached: false } as unknown;
  },
  getIssueLimitUsage: async () => {
    if (inTauri) return invoke("get_issue_limit_usage");
    return null as unknown;
  },
  getIssueUsage: async (issueId: string) => {
    if (inTauri) return invoke("get_issue_usage", { issue_id: issueId });
    return { task_count: 0 } as unknown;
  },
  initiateListModels: async (runtimeId: string, options?: { force?: boolean }) => {
    if (inTauri) {
      return invoke("initiate_list_models", { runtime_id: runtimeId, force: options?.force ?? null });
    }
    return { id: `job-${Date.now()}`, status: "completed", models: [], supported: false } as unknown;
  },
  getListModelsResult: async (runtimeId: string, requestId: string) => {
    if (inTauri) return invoke("get_list_models_result", { runtime_id: runtimeId, request_id: requestId });
    return { id: requestId, status: "completed", models: [], supported: false } as unknown;
  },
  initiateListLocalSkills: async (runtimeId: string) => {
    if (inTauri) return invoke("initiate_list_local_skills", { runtime_id: runtimeId });
    return { id: `job-${Date.now()}`, status: "completed", skills: [], supported: true } as unknown;
  },
  getListLocalSkillsResult: async (runtimeId: string, requestId: string) => {
    if (inTauri) {
      return invoke("get_list_local_skills_result", { runtime_id: runtimeId, request_id: requestId });
    }
    return { id: requestId, status: "completed", skills: [], supported: true } as unknown;
  },
  initiateImportLocalSkill: async (runtimeId: string, data: Record<string, unknown>) => {
    if (inTauri) return invoke("initiate_import_local_skill", { runtime_id: runtimeId, data });
    return { id: `job-${Date.now()}`, status: "completed" } as unknown;
  },
  getImportLocalSkillResult: async (runtimeId: string, requestId: string) => {
    if (inTauri) {
      return invoke("get_import_local_skill_result", { runtime_id: runtimeId, request_id: requestId });
    }
    return { id: requestId, status: "completed" } as unknown;
  },
  getConfig: async () => {
    if (inTauri) return invoke("get_config");
    return { attachment_public_url: "", cdn_domain: null, cdn_signed: false } as unknown;
  },
  markOnboardingComplete: async (data?: Record<string, unknown>) => {
    if (inTauri) return invoke("mark_onboarding_complete", { payload: data ?? null });
    return { completed: true, ...data } as unknown;
  },
  patchOnboarding: async (data: Record<string, unknown>) => {
    if (inTauri) return invoke("patch_onboarding", { data });
    return data as unknown;
  },
};

// ── 导出：安装为全局 api 单例 ──────────────────────────────────────────────
// 云端功能（已移除，不会实现）：账单/订阅/云端运行时/邀请/分享链接/PAT/
// OAuth 登录/Composio/GitHub App/VCS/插件市场/Mika/反馈上报/用量上报/频道 OAuth 绑定。
const CLOUD_REMOVED = new Set([
  "acceptInvitation", "declineInvitation", "revokeInvitation", "getInvitation",
  "listWorkspaceInvitations", "leaveWorkspace",
  "createShareLink", "listShareLinks", "getShareLinkInfo", "revokeShareLink", "joinByShareLink",
  "createPersonalAccessToken", "listPersonalAccessTokens", "revokePersonalAccessToken",
  "createCloudBillingCheckoutSession", "createCloudBillingPortalSession",
  "getCloudBillingBalance", "getCloudBillingCheckoutSession",
  "listCloudBillingBatches", "listCloudBillingPriceTiers", "listCloudBillingTopups",
  "listCloudBillingTransactions",
  "createWorkspaceSubscriptionCheckout", "createWorkspaceSubscriptionPortal",
  "getWorkspaceSubscriptionPrices", "getWorkspaceSubscriptionSummary",
  "previewWorkspaceSeatPurchase", "purchaseWorkspaceSeats", "reconcileWorkspaceSubscriptionSeats",
  "createCloudRuntimeNode", "listCloudRuntimeNodes", "deleteCloudRuntimeNode",
  "googleLogin", "sendCode", "verifyCode", "refreshSession", "logout", "issueCliToken",
  "createFeedback", "upsertClientUsage", "joinCloudWaitlist",
  "startMikaOnboarding", "createMikaAgent",
  "grantAutopilotAccess", "revokeAutopilotAccess", "rotateAutopilotTriggerWebhookToken",
  "beginLarkInstall", "getLarkInstallStatus",
  "redeemSlackBindingToken", "redeemTelegramBindingToken", "redeemDingTalkBindingToken",
  "redeemLarkBindingToken", "redeemWecomBindingToken",
  "listAgentDingTalkGroups", "listDingTalkGroups", "forgetDingTalkGroup",
  "beginComposioConnect", "listComposioConnections", "deleteComposioConnection", "listComposioToolkits",
  "connectVCS", "listVCSConnections", "deleteVCSConnection", "rotateVCSWebhook",
  "linkIssuePullRequest", "unlinkIssuePullRequest", "setIssuePRAutoComplete",
  "listGitHubInstallations", "listGitHubInstallationRepositories", "deleteGitHubInstallation",
  "getGitHubConnectURL",
  "installPlugin", "uninstallPlugin", "publishPluginPackage", "publishLocalPluginPackage",
  "previewPlugin", "configurePlugin", "setPluginEnabled", "callPluginAction", "invokePluginHook",
  "approvePluginMCPTools", "listPluginMCPTools", "listPluginInvocations", "listPluginPackages",
  "deletePluginPackage", "getPluginSurfaceLaunch", "listPluginInstallations",
  "revokePluginToken", "rotatePluginToken",
  "updateMe",
]);

export function installTauriApi(): void {
  const api = new Proxy({} as ApiClient, {
    get(_target, prop) {
      if (prop === "then") return undefined; // 避免被当作 Promise
      const handler = handlers[prop as string];
      if (!handler) {
        return (..._args: unknown[]) => {
          const err = new Error(
            CLOUD_REMOVED.has(String(prop))
              ? `[tauri-api] ${String(prop)} is a cloud feature removed in the local build`
              : `[tauri-api] ${String(prop)} not yet migrated — see docs/ROUTE_A.md`,
          );
          window.__tauriApiLog?.push({ method: String(prop), ok: false, error: err.message });
          throw err;
        };
      }
      return async (...args: never[]) => {
        try {
          const result = await handler(...args);
          window.__tauriApiLog?.push({ method: String(prop), ok: true });
          return result;
        } catch (e) {
          window.__tauriApiLog?.push({
            method: String(prop),
            ok: false,
            error: e instanceof Error ? e.message : String(e),
          });
          throw e;
        }
      };
    },
  });
  setApiInstance(api);
}

export { MOCK_USER, MOCK_WORKSPACE };
