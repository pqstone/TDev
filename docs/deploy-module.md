# 部署发布模块 · 设计与数据模型

> 状态：**第一阶段（服务器管理）已实现**；第二阶段（构建 → 上传 → 发布）待接。
> 本文档是部署模块的唯一权威设计，改代码前先改这里。

## 1. 目标与边界

给**没有 IT 背景**的使用者用：先在页面上「添加服务器」，再把项目发布上去。
模块分两阶段：

| 阶段 | 内容 | 状态 |
| --- | --- | --- |
| 一 | 服务器管理：新增 / 编辑 / 删除 / 端口连通性测试，密码加密存储 | ✅ |
| 二 | 部署执行：当前项目 → 选择服务器 → 构建 → 上传 → 启动 → 启动验证 → 发布记录 | ✅ |

## 2. 服务器清单的归属

**按项目维护**（`prj_guid` 维度），与「项目工作」下其他模块保持一致：切项目只看得到本项目的服务器。
理由：不同项目的生产/测试环境不同，避免误发到别的项目服务器；后续若要共享服务器库，再加「导入其他项目服务器」。

## 3. 数据模型

### 3.1 新表 `deploy_servers`

```sql
CREATE TABLE IF NOT EXISTS deploy_servers (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    prj_guid     TEXT    NOT NULL,               -- 所属项目
    name         TEXT    NOT NULL,               -- 显示名，如「生产-Tomcat01」
    os_type      TEXT    NOT NULL,               -- linux | windows
    host         TEXT    NOT NULL,               -- IP 或域名
    port         INTEGER NOT NULL DEFAULT 22,    -- Linux 默认 22；Windows 默认 445（SMB）
    username     TEXT    NOT NULL,
    password_enc TEXT,                           -- DPAPI 加密后的十六进制串；空=未设置
    remark       TEXT,
    created_at   TEXT    NOT NULL,
    updated_at   TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_deploy_servers_prj ON deploy_servers(prj_guid);
```

**密码安全**：`password_enc` 存 Windows DPAPI（当前用户作用域）加密结果，密文只能被同一台机器的同一 Windows 用户解开；前端永远拿不到明文，列表只返回 `has_password`。

### 3.2 亮灯（`module_state.deploy`）

`deploy_servers` 中该项目**至少有一条服务器** → `deploy = 1`（灯亮）。

## 4. 后端命令（`src-tauri/src/lib.rs`）

| 命令 | 入参 | 出参 | 说明 |
| --- | --- | --- | --- |
| `deploy_server_list` | `prj_guid` | `[{id,name,os_type,host,port,username,has_password,remark,created_at,updated_at}]` | 按创建时间倒序，不返回密码 |
| `deploy_server_save` | `id?`, `prj_guid`, `name`, `os_type`, `host`, `port`, `username`, `password?`, `remark` | `{id}` | 新增或更新；`password` 为空表示**保持原密码不变** |
| `deploy_server_delete` | `prj_guid`, `id` | `bool` | 只删本项目下的记录 |
| `deploy_server_test` | `prj_guid`, `id` | `{ok,latency_ms,msg}` | 对 host:port 做 TCP 连接（5 秒超时），验证网络可达 |

建表在 `db_init` 中幂等执行（`CREATE TABLE IF NOT EXISTS`），旧库升级无需手工迁移。

## 5. 界面

页面结构（单列，节约版面）：

1. **服务器** 面板
   - 右上「＋ 添加服务器」（唯一实心主按钮）
   - 服务器列表：名称 + 系统徽标 + `地址:端口` + 用户名 + 密码状态 + 操作（测试连接 / 编辑 / 删除）
   - 空状态：说明 + 引导添加
2. 添加/编辑用弹窗（宽 640）：名称、操作系统、地址、端口、用户名、密码、备注 —— 两列布局，短字段一行两个
3. 操作反馈走底部状态栏 + 行内结果（延迟/失败原因）

**未选择项目**时显示空状态提示，不报错、不写库。

## 6. 自验记录（2026-11-10）

- `cargo test --lib` 4 条全绿：hex 往返、DPAPI 加解密往返、CRUD + 跨项目隔离 + 改资料不传密码时原密文保留、TCP 探测开/闭端口。
- 真实库验证：`C:\Users\Administrator\AppData\Roaming\com.tdev.app\tdev.db` 已有 `deploy_servers` 表；界面添加的服务器密码列是 DPAPI 密文（`01000000d08c9ddf…`），用 `CryptUnprotectData` 可解回（长度 8），说明加密链路可用。
- 已知：`db_init` 幂等建表，旧库升级无需手工迁移。

## 7. 第二阶段：部署执行（2026-11-10 已实现）

### 7.1 流程

```
当前项目 → 选择服务器 → 保存部署配置 → 开始部署
  ①构建（本地，可留空跳过）
  ②找产物（支持 * 通配，取最新修改的文件）
  ③SSH 登录（Linux/Windows 共用，Windows 需开 OpenSSH Server）
  ④SFTP 上传到目标目录（目录不存在则创建）
  ⑤执行启动命令（远端 shell；留空跳过）
  ⑥启动验证（HTTP 访问地址优先，其次 TCP 端口，按次数重试）
  ⑦写发布记录（成功/失败 + 完整日志）
```

### 7.2 表

- `deploy_configs`（每项目一份）：`server_id / build_cmd / artifact_path / remote_dir / start_cmd / verify_url / verify_port / verify_retries`
- `deploy_releases`：`status(running|success|failed) / step / artifact / message / log / started_at / finished_at`

### 7.3 命令

| 命令 | 说明 |
| --- | --- |
| `deploy_config_get` | 读配置；没有则按项目类型返回默认值（不写库） |
| `deploy_config_save` | 保存配置（校验服务器/产物/目标目录） |
| `deploy_run` | 启动部署，立即返回 `runId`，后台跑管道 |
| `deploy_release_list` / `deploy_release_log` | 发布记录列表 / 单条完整日志 |
| `deploy_status` | 是否有部署在跑（切页回来恢复状态用） |

事件：`deploy_event` `{runId, step(build|upload|start|verify|done), status(start|log|ok|fail|success), line}`

### 7.4 默认配置（按项目类型）

| 类型 | 构建 | 产物 | 目标目录 | 启动 | 验证 |
| --- | --- | --- | --- | --- | --- |
| web | `mvn -q -DskipTests clean package` | `target/*.war` | `/opt/tomcat/webapps` | `sudo systemctl restart tomcat` | 端口 8080 |
| Tauri | `cargo build --release` | `src-tauri/target/release/*.exe` | `C:/Apps/<项目>` | PowerShell 停旧进程再启动 | 端口 0（需自填） |
| windows program | `cargo build --release` | `target/release/*.exe` | `C:/Apps/<项目>` | 同上 | 端口 0 |

### 7.5 构建环境与日志编码（2026-11-10 修）

- **构建环境自动加载**：Windows 下若项目同级存在 `<workspace>\Public\runtime\env.bat`（TestSpace 约定：JDK17 + Maven + Tomcat），构建前自动 `call` 它，再执行构建命令。mvn/java 不在系统 PATH 里也不会失败。
- **日志按 GBK 解码**：Windows 控制台默认 GBK，之前按 UTF-8 读会把中文报错当非法字符丢掉，用户只看到「退出码 1」。现在中文报错完整可见。
- **无输出兜底**：退出码非 0 且一行输出都没有时，直接提示「多半是命令不存在（PATH 里没有 mvn/java）」，并给出 `call Public\runtime\env.bat && mvn …` 的写法。
- 构建脚本用临时 `.bat` 承载（`%TEMP%\tdev_build_<runId>.bat`），避免 `cmd /C` 嵌套引号问题。

### 7.6 约束与已知限制

- 同一时刻只允许一个部署（进程内 `DEPLOY_RUNNING` 锁）。
- SSH 首次连接**信任主机密钥**（内网工具取舍）；如需严格校验，后续接 known_hosts。
- 启动验证的 HTTP 仅支持 `http://`（reqwest 未启用 TLS 后端）。
- Windows 目标需要安装 OpenSSH Server（当前通道统一走 SSH/SFTP）；如后续要 SMB 共享目录通道，另加。
- 密码只在部署进程内存里出现，不写日志、不回传前端。

### 7.6 自验

- `cargo test --lib` → 7 条全绿（配置校验/覆盖、默认值、产物 glob、DPAPI、服务器 CRUD、TCP 探测）。
- **真机端到端**：`cargo run --release --bin probe`（`src/bin/probe.rs` + `probe_pipeline_standalone`）→ 构建→上传→启动→验证全绿，远端 `/tmp` 用后清理。

## 8. Agent 部署（第三阶段，2026-11-10）

### 8.1 思路

不再由后端写死“构建→上传→重启→验证”的固定命令，而是**把部署能力和上下文交给 Agent**，由 Agent 自己判断项目怎么构建、传到哪、怎么启动、失败怎么查日志。后端只负责三件事：

1. **打通免密登录**（`deploy_agent_prepare`）
2. **给出部署上下文**（服务器、项目目录、产物、目标目录、验证地址、构建环境、铁律）
3. **记录结果**（`deploy_release_note`）

### 8.2 免密登录

- TDev 使用**专用密钥** `%USERPROFILE%\.ssh\tdev_deploy_ed25519`（不存在就 `ssh-keygen` 生成），把公钥追加进服务器 `~/.ssh/authorized_keys`（用已保存的 DPAPI 密码通过 russh 登录执行）。
- 随后用 `ssh -i <key> -o IdentitiesOnly=yes -o BatchMode=yes` 验证一次，通不过就直接报错，不把问题甩给 Agent。
- **为什么不用用户默认 `id_ed25519`**：本机那把私钥带口令（`incorrect passphrase supplied to decrypt private key`），无法非交互使用，硬用会一直 `Permission denied`。专用密钥无口令、只用于部署。

### 8.3 Agent 侧

- 走 TD Agent（127.0.0.1:8644，与 QQ 用的 default 进程隔离），场景 `module:deploy:<prjGUID>:run-…`，每次新会话。
- 工具：Agent 自带 `terminal` / `file` / `code_execution`，用上下文里给的 `ssh -i …` / `scp -i …` 命令直接操作服务器。
- 铁律写进 prompt：必须真实执行、成功标准三项（产物存在 + 已上传 + HTTP 2xx/3xx 或端口通）、失败要读日志定位、不许 sudo、不许碰 Docker/Dify/80/443、不许改源码掩盖环境问题、最后必须输出「部署结果：成功/失败 + 证据」。
- 前端在 Agent 结束后按回复里是否含「部署结果：成功」判定成败，写一条 `deploy_releases`（step=agent）。

### 8.4 界面（2026-11-10 改版）

- **部署配置表单已整体删除**（构建命令 / 产物 / 远程目录 / 启动命令 / 验证地址 / 重试次数 / 目标服务器下拉全部去掉），固定流程的「手动部署」与进度面板也一并移除。
- 页面只剩两块：**服务器**（增删改查 + 端口测试）和 **AI 部署对话**（与「业务流程 / AI开发」一致的对话式：消息列表 + 输入框 + 发送，Enter 发送 / Shift+Enter 换行）。
- 对话头部保留一个「部署到：」服务器选择器（只选目标，不是配置）；选好后直接说“把当前项目部署上去，验证能访问”。
- 每轮对话前自动调用 `deploy_agent_prepare` 打通免密并附上下文；Agent 若发现参考目录/端口与实际不符，会自行调整并说明理由。
- 部署记录仍在页面底部，点一条可看结果摘要。
- **部署记录**：默认只显示最近 1 行，右上「展开全部（N）」可展开、「收起」折叠。
- **执行过程独立面板**：Agent 的思考链 / 🔧 工具调用 走独立的「🛠 执行过程」面板（与 AI开发 同一套 `.ai-tool-panel`），不再混进聊天气泡；聊天气泡只保留用户消息和 Agent 最终答复。面板可点标题栏折叠/展开，含「强制解锁」逃生口。
