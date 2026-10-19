# Multica — Rust + Tauri 本地版

原项目 Multica（Go + Next.js + Electron）的 Rust + Tauri 重写，**功能与样式原样保留，去除全部云端功能**。

## 与原版的区别

- 后端：Go → Rust（Tauri commands）
- 桌面壳：Electron → Tauri 2
- 数据库：PostgreSQL → SQLite（`app_data_dir/multica.db`，本地文件）
- 删除：云端托管、计费/订阅、遥测、PostHog、邮件验证码、Google OAuth、S3/CloudFront、PAT/JWT、Landing 营销页、移动端
- 保留：Workspace、Issue 看板、Agent、Runtime、Run 执行日志、Skill、Autopilot、Chat、Inbox、VCS/频道（改为用户自填 token 直连，不走云端中转）

详见 [docs/MIGRATION.md](docs/MIGRATION.md)（完整删除清单 + Go→Rust 映射 + 工作量评估）。

## 本目录状态

前端已按路线 A 搬迁：原 `packages/ui + views + core` 原样进 Tauri Webview，26 条路由全部接通，读写走真实 Rust 命令 + Tauri 事件桥接。见 `docs/ROUTE_A.md`。真机 smoke test 见 `docs/SMOKE_TEST.md`（沙箱跑不了 Tauri GUI）。

## 本地运行

```bash
# 1. 安装 Rust + Node（沙箱无 cargo，需在你本机跑）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
npm install -g @tauri-apps/cli

# 2. 前端预览（不经 Tauri，走浏览器 mock，先看 UI）
pnpm install
pnpm --filter @multica-tauri/frontend-a dev

# 3. Tauri 桌面运行（走 Rust + SQLite）
pnpm install
cargo tauri dev
# 发布：cargo tauri build
```

## 安全提醒（与原版一致）

Agent CLI 默认以当前 OS 用户的全部权限运行（Codex `danger-full-access`、Claude `--permission-mode bypassPermissions`），
本程序不做沙箱承诺。如需隔离，请用专用用户 / VM / 容器运行。
