# AGENTS.md — TDev 项目开发规范（面向 AI 与人类开发者）

> **本文件是项目的最高开发约定。任何 AI Agent 在本仓库内执行开发任务前，必须先完整阅读本文档并逐条遵循。** 它与代码同等重要；当具体指令与本文冲突时，以本文为准，并说明冲突点。

---

## 0. 项目定位与目标

TDev 是一个 **Tauri v2 桌面应用**，目标是做一个**轻量、可扩展、结构清晰**的应用骨架。

- 技术栈：**纯静态前端**（HTML/CSS/JS，无框架）+ **Rust（Tauri v2）** 后端。
- 交互模型：前端通过 `window.__TAURI__.core.invoke` 调用 Rust 侧 `#[tauri::command]`；文件能力走 `tauri-plugin-fs`，外部命令走 `tauri-plugin-shell`。
- 当前进度：已具备**主界面骨架**（上中下三区 + 中部左右分栏 + 左侧双分类菜单，均带图标），UI 采用**北极冰山·深邃蓝**主题；主界面布局与菜单结构见 `README.md` 与 `dist/index.html`。
- 本规范的首要价值：**保障项目结构清晰、架构规范、代码整洁低冗余**，即使长期迭代或换 Agent 也不失稳。

---

## 1. 技术栈与版本基线（硬约束）

| 项 | 约束 |
| --- | --- |
| 前端 | 纯 HTML/CSS/JS。**禁止引入 Vue/React/Svelte 等框架**，除非显式评估后经批准并更新本文档 |
| 后端 | Rust，edition 2021；Tauri `2.x`；`tauri-plugin-fs = "2"`、`tauri-plugin-shell = "2"` |
| 本地数据库 | SQLite，`rusqlite = { version="0.32", features=["bundled"] }`（内嵌，免费开源） |
| 唯一标识 | `uuid = { version="1", features=["v4"] }`（生成项目 `prjGUID`，Apache-2.0/MIT，体积小） |
| 打包 | `tauri.conf.json` 使用 schema 2（Tauri v2 配置） |
| 构建 | `cargo`（本机路径 `C:\Users\Administrator\.cargo\bin\cargo.exe`） |
| 目标平台 | Windows（当前），结构上保持可移植 |

> 新增第三方依赖（Cargo crate / npm 包）必须先说明**用途、许可、体积、风险**，再写入配置。

---

## 2. 目录结构（强制，不得擅自变更）

```
TDev/
├─ AGENTS.md              ← 本规范（最高优先级）
├─ README.md              ← 项目简介
├─ .gitignore             ← 忽略构建产物/依赖
├─ .gitattributes         ← 行尾/文本规范化
├─ dist/                  ← 前端成品（Tauri 运行时加载目录，frontendDist=../dist）
│  └─ index.html          ← 当前为单文件成品（内联 CSS/JS），含主界面布局与菜单
├─ src/                   ← 前端源码（模块化参考：main.js、styles.css）
├─ UI-STYLE.md            ← UI 风格、规范与约束（北极冰山·深邃蓝，页面开发唯一风格依据）
├─ theme/                 ← 设计令牌与视觉示意
│  ├─ tokens.json         ← 机器可读设计令牌（色值/间距/字号/圆角/组件）
│  └─ ui-preview.html     ← 全组件视觉示意页
├─ docs/                  ← 设计/架构文档
│  ├─ ARCHITECTURE.md
│  └─ （新增功能前先在此写设计说明）
└─ src-tauri/             ← Rust 后端
   ├─ src/main.rs         ← 程序入口（含窗口子系统控制）
   ├─ src/lib.rs          ← 插件注册 + 全部 #[tauri::command]
   ├─ capabilities/default.json ← 权限声明（最小化）
   ├─ tauri.conf.json     ← 应用配置（窗口/打包/标识符）
   ├─ Cargo.toml          ← 依赖清单
   ├─ build.rs            ← Tauri 构建钩子（勿改）
   └─ icons/              ← 应用图标
```

**规则**：
- 新代码必须落在上表既有目录；新增目录需在 `AGENTS.md` 与 `docs/ARCHITECTURE.md` 同步登记，不得"顺手新建"。
- `dist/` 是**运行时成品**，`src/` 是**源码**。改动源码后必须同步到 `dist/`（或接入构建工具后由构建产物覆盖）。
- **主界面布局（上中下三区 + 左侧双分类菜单）与菜单结构**：见 `README.md` 与 `dist/index.html`；改动菜单/布局必须同步两者并保持主题规范一致。
- 参考历史：`E:\TauriTest` 为本项目同名已验证可编译的对照组，配置改动前可对照其 `tauri.conf.json`/`Cargo.toml`。

---

## 3. 命名规范

### 3.1 Rust 后端
- 文件、目录、函数、变量、模块：`snake_case`。
- `#[tauri::command]` 命令名：`snake_case`，语义明确（如 `create_folder`）。
- 命令返回值：统一 `Result<T, String>`，错误用 `map_err(|e| e.to_string())` 转字符串。
- 私有辅助函数以 `fn _xxx` 不必要；用可见性（`fn`/`pub fn`）区分，避免下划线前缀。

### 3.2 前端
- JS 变量/函数：`camelCase`；常量：`UPPER_SNAKE`。
- CSS 类名/ID：`kebab-case` 或 `camelCase` 统一其一（本项目用 `camelCase` 于 JS 引用、`kebab` 于纯样式）。
- HTML 语义标签优先（`main/section/button/input`），避免滥用 `div`。

### 3.3 通用
- 文件名：小写 + 连字符（`user-manager.js`）或小写单文件（`lib.rs`）。
- 中文注释用简体中文；关键英文术语保留原文。

---

## 4. Tauri 专属规则（最高风险区）

### 4.1 IPC 命令
- 所有后端能力必须经 `#[tauri::command]` 暴露，并在 `lib.rs` 的 `generate_handler!` 中注册。
- **禁止**前端绕过桥接层直接执行系统命令/文件写入（否则权限模型失效）。
- 命令入参用具名 struct 或 `String` 均可，但**入参必须做校验**（路径、脚本内容），不得信任前端输入。

### 4.2 权限最小化（capabilities）
- `capabilities/default.json` 只授予当前功能所需的最小权限。
- **新增任何插件/能力，必须显式在 permissions 中添加**，禁止 `"core:default"` 之外放宽为全开。
- 高风险能力（任意命令执行、任意路径读写）默认关闭；确需时做**白名单/沙箱化**并在本文档记录。

### 4.3 窗口子系统（黑窗问题）
- `main.rs` 保留 `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`：
  - **Debug 构建有控制台黑窗（正常，便于看日志）**
  - **Release 构建无黑窗（正式形态）**
- 交付/演示一律用 **Release**；调试才用 Debug。不得随意删除该行或改判条件。

### 4.4 配置
- `tauri.conf.json`：改 `productName`/`identifier` 需谨慎（影响打包标识）；窗口属性改动需同步更新 `capabilities` 里的 `windows` 标签。

### 4.5 本地数据库（SQLite）
- 方案固定为 `rusqlite`（内嵌单文件，免费开源）。**不使用官方 `tauri-plugin-sql` 前端**（其 `window.__TAURI__.sql` 需前端 npm 包 `@tauri-apps/plugin-sql`，与本项目纯静态无 npm 冲突）。
- 所有数据库操作必须封装为 `#[tauri::command]`（如 `db_init`），前端一律经 `invoke` 调用；**禁止前端直连库或在前端拼接 SQL**。
- SQL 一律用 rusqlite 的**参数化绑定**（`?` / `params!`），禁止字符串拼接拼 SQL（防注入）。
- 库文件固定存于 AppConfig 目录（`%APPDATA%\com.tdev.app\tdev.db`）；建表用 `CREATE TABLE IF NOT EXISTS` 幂等写法。
- 新增表/字段：先在此记录并在 `docs/` 补充 schema 说明，保持可追溯。
- **已封装命令**：`db_init`（建库建表）、`project_list`（项目列表）、`project_create`（新增项目，自动生成 UUID v4 作 `prjGUID`，参数化插入）。
- **已建表**：`app_meta`、`demo_items`、`projects(prjGUID, prjName, prjType, prjDescription, created_at)`；新增项目功能涉及的表在此登记，后续扩展沿用本条约定。

---

## 5. 代码规范

### 5.1 Rust
- 错误处理：函数内 `?` 传播 + 命令层 `map_err` 转字符串；禁止 `unwrap()`/`expect()` 出现在非 `main`/非测试的运行时路径。
- 变量用 `let` 优先；类型尽量显式可读。
- 不要给简单逻辑过度抽象；每个函数只做一件事。
- 依赖注入与全局状态：先看是否有插件化/参数化方案，避免在 `Builder` 外堆全局 `static mut`。

### 5.2 前端
- 无框架下按**模块化函数 + 事件委托**组织，避免巨型 `onclick` 内联。
- DOM 操作集中；状态尽量单一来源，避免散落全局变量。
- 样式遵循 UI 风格规范（见 §6 与 `UI-STYLE.md`，北极冰山·深邃蓝），色值用 CSS 变量集中管理，不硬编码散落。

---

## 6. UI 风格与样式规范

- **唯一权威**：`UI-STYLE.md`（北极冰山·深邃蓝）+ 机器令牌 `theme/tokens.json` + 视觉示意 `theme/ui-preview.html`。三者缺一不可；变更风格必须四步同步（改 tokens → 改 UI-STYLE → 改示意页 → 标注影响范围）。
- 主色基准：`#0A4D8C`（primary-700，深邃冰蓝）；背景 `#F4F8FC`；文字 `#102A43`；边框 `#D2E0EC`；圆角 `8px`。
- 色值/间距/字号一律从 `theme/tokens.json` 取，用 **CSS 变量**（`:root`）承接；新增页面复用变量，禁止硬编码或另建一套色板。
- 图标：线性风格、`1.5px` 描边、颜色用 `currentColor` 跟随主题；菜单图标已按此规范内联于 `dist/index.html`。
- 主界面布局与左侧双分类菜单见 `README.md` 与 `dist/index.html`；改动必须同步两者并遵循本主题规范。
- 深色主题如未来需要：在 `:root[data-theme="dark"]` 下覆盖变量，不复制选择器。

---

## 7. 构建 / 运行 / 调试命令（实测有效）

```powershell
# Debug 编译（保留控制台看日志）
cargo build --manifest-path "E:\88.project\TDev\src-tauri\Cargo.toml"

# 运行
cd "E:\88.project\TDev\src-tauri"; & "C:\Users\Administrator\.cargo\bin\cargo.exe" run

# Release 编译（正式，无黑窗）
cd "E:\88.project\TDev\src-tauri"; & "C:\Users\Administrator\.cargo\bin\cargo.exe" build --release

# 打包安装器（需先有前端 dist）
cd "E:\88.project\TDev\src-tauri"; & "C:\Users\Administrator\.cargo\bin\cargo.exe" tauri build
```

**验证手段**：
- 编译成功 ≠ 完成：需**运行产物**，观察窗口是否正常、IPC 是否连通、有无黑窗/报错。
- 前端 IPC 连通性：打开应用后看页面底部状态是否显示"已连接 Tauri"。
- 子系统验证（可选）：读取 PE 头 `Subsystem` 字段，Release 应为 2（GUI）、Debug 为 3（控制台）。

---

## 8. 安全红线（触碰即停）

1. **命令执行白名单化**：当前 `run_script` 是演示用途的任意执行，属高风险；生产化前必须改为白名单/参数化，且补权限与日志。**不得默认将其作为通用能力使用。**
2. 路径操作必须防 `../` 目录穿越，禁止直接拼接未校验的用户输入到 `std::fs`/`std::process`。
3. `capabilities` 不放宽；新增权限要可审计（注释说明为什么需要）。
4. 不引入存在已知 CVE 或维护停滞的依赖。

---

## 9. 开发工作流（任何开发任务按此推进）

1. **探查**：先完整读取相关文件（`AGENTS.md`、目标模块、`docs/ARCHITECTURE.md`），不凭记忆假设文件内容。
2. **约束**：守住本文档与技术栈边界；偏离必须说明并获准。
3. **证据**：每个新增数字/事实来自可追溯来源或可复现验证。
4. **执行**：小步修改；改完一处立即验证，不攒着。
5. **验证**：用不同于生成路径的方式回读；编译 + 运行 + 观察 IPC/窗口；关键结论写进交付说明。
6. **交付**：说明改了哪些文件、验证方式、结果、遗留缺口。

---

## 10. 版本与提交

- 本仓库在 `E:\88.project`（git 仓库）下；TDev 提交遵循该仓库惯例。
- 提交信息：`<type>: <summary>`（type ∈ feat/fix/docs/refactor/chore）。
- 提交前 `git status` 确认只含本次改动；`dist/`、`target/`、`node_modules/` 不提交（见 `.gitignore`）。

---

## 11. 禁止事项（Do NOT）

- 不擅自引入前端框架或新增第三方依赖。
- 不改 `main.rs` 的黑窗子系统逻辑（除非经评估并记录）。
- 不绕过 `#[tauri::command]` 让前端直接执行系统命令/写文件。
- 不放宽 `capabilities` 到"全开"。
- 不绕过 `UI-STYLE.md` / `theme/tokens.json` 另建一套色板或硬编码色值。
- 不在前端拼接 SQL 或绕过 command 直连 SQLite；SQL 一律参数化。
- 不把验证停留在"编译通过"。
- 不在 `dist/` 与 `src/` 之间留下不一致（改源码必同步成品）。
- 不覆盖/删除用户已有的其他项目文件（`E:\88.project` 是共享 git 仓库，只动 TDev 范围内）。
