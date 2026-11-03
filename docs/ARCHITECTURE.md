# TDev 架构说明

> 本文档描述当前骨架的架构决策与演进方向，供开发者与 AI Agent 在扩展功能前阅读。

## 一、总体架构

```
┌──────────────────────────┐
│   前端（dist/ 静态页）    │  纯 HTML/CSS/JS，无框架
│   window.__TAURI__.core  │
└────────────┬─────────────┘
             │ IPC invoke()（JSON 序列化）
┌────────────▼─────────────┐
│   Rust 后端（Tauri v2）   │  src-tauri/src/lib.rs
│   #[tauri::command]      │
│   插件：fs / shell / dialog │
└────────────┬─────────────┘
             │ std::fs / std::process
┌────────────▼─────────────┐
│   本地文件系统 / 系统命令  │  受 capabilities 权限约束
└──────────────────────────┘
```

## 二、核心分层职责

| 层 | 目录 | 职责 | 规范约束 |
| --- | --- | --- | --- |
| 展示层 | `dist/`、`src/` | 页面、交互、样式 | 见 AGENTS.md「前端规范」 |
| 桥接层 | `src-tauri/src/lib.rs` | 定义 `#[tauri::command]`，注册插件 | 见「后端规范」 |
| 能力层 | `tauri-plugin-fs` / `tauri-plugin-shell` / `tauri-plugin-dialog` | 文件系统、外部命令、原生对话框 | 见「安全与权限」 |
| 配置层 | `tauri.conf.json` / `capabilities/` | 窗口、打包、权限 | 见「Tauri 专属规则」 |

## 三、关键决策（ADR）

- **前端不用框架**：保持轻量与零构建；若后续需要组件化，接入 Vite，`dist/` 改为构建产物目录。
- **IPC 命令唯一入口**：所有后端能力必须通过 `#[tauri::command]` 暴露，禁止前端直接执行系统命令绕过权限层。
- **权限最小化**：`capabilities/default.json` 只授当前所需；新增能力必须显式加权限并经评审。
- **Debug 保留控制台 / Release 无黑窗**：由 `main.rs` 的 `cfg_attr(not(debug_assertions), windows_subsystem = "windows")` 控制。
- **SQLite 用 rusqlite 而非 tauri-plugin-sql**：纯静态前端无 npm，后端 `rusqlite`（bundled）封装 command。
- **Agent 聊天走 Hermes API Server**：不用 PTY/TUI（界面假活、agent 不跑）、不用 CLI 子进程（黑窗/假死）；HTTP 调 `/v1/chat/completions` + `X-Hermes-Session-Id` 会话隔离最可靠。
- **耗时命令必须 async**：`agent_ask` 用 `tauri::async_runtime::spawn_blocking` 防 UI 冻结；Windows 子进程加 `CREATE_NO_WINDOW` 防黑窗。

## 四、当前能力（已实现）

1. **项目管理**：新增/列表/删除项目；新增时在工作空间创建目录、复制 `src/template` 模板、按项目信息重命名/替换文档（template.md→{项目名}.md、devStandard md 占位符替换）。
2. **配置中心**：工作空间路径、模型配置（8 提供商）、Agent 配置，`cfg_save`/`cfg_load` 存 `app_meta`。
3. **在线聊天**：Agent连接测试页，QQ 风格气泡，`agent_ask` 调 Hermes API Server，`X-Hermes-Session-Id` = 项目 prjGUID 隔离会话；历史 + 思考状态 localStorage 持久化。

## 五、演进方向（开放项）

1. 深色主题（`[data-theme="dark"]` 覆盖变量）。
2. 项目下拉联动右侧内容（按项目加载数据）。
3. 项目目录树展示 + 文档内嵌浏览（用户已规划）。
4. 前端工程化（Vite + TS）——当前纯静态足够，非必需。