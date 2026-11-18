# 本地 Agent 管理（Agent配置 扩展）设计文档

> 版本：v1 · 2026-11-17 · 范围：TDev `Agent配置`（菜单 `data-m=7`）

## 1. 背景与目标

现状：Agent配置 只能填「接入地址 + Key」，Agent 本身要靠人去命令行启动、也没有任何运行状态反馈。
TDev 的默认 Agent（Hermes 8642/8644）是本机进程，启动/停止/看状态是开发者的高频动作。

目标（本次范围）：

1. Agent 增加 **是否本地 Agent** 开关；
2. 勾选本地后，增加 **启动脚本 / 可执行文件路径** 与 **工作目录**（可选）配置；
3. 列表里对本地 Agent 提供 **状态灯 + 启动 / 重启 / 停止 + 命令行对话框**；
4. 命令行对话框：显示并可编辑实际启动命令行，支持「执行启动」与「试运行（捕获输出，最多 8 秒）」。

不在本次范围：远程服务器上的 Agent 进程管理（走部署发布模块）、多实例并发启动（沿用「同一时刻只允许一个 Agent run」的既有约束）。

## 2. 数据模型

`app_meta.key = 'agents'`（现有结构扩展，向后兼容）：

```json
{
  "list": [
    {
      "name": "Hermes-Hulk",
      "type": "hermes",
      "url": "http://127.0.0.1:8642",
      "key": "***",
      "extra": "",
      "local": true,                                  // 新增：是否本地 Agent
      "script": "E:\\HermesMinimal_v23\\start_all_v23.bat",  // 新增：启动脚本/可执行文件
      "cwd": "E:\\HermesMinimal_v23",                  // 新增：工作目录（空=脚本所在目录）
      "cmdline": "cmd /C \"E:\\...\\start_all_v23.bat\""     // 新增：对话框里确认过的命令行（空=默认生成）
    }
  ],
  "defaultName": "Hermes-Hulk"
}
```

兼容：老配置没有 `local/script/cwd/cmdline` 字段 → 一律按 `local=false` 处理，界面与行为不变。

## 3. 状态判定（后端 `local_agent_status`）

按优先级两条通道，任一命中即「运行中」：

| 通道 | 规则 | 说明 |
|---|---|---|
| 端口（主） | 从 `url` 解析端口 → `TcpStream::connect_timeout(127.0.0.1:port, 350ms)` | 最可靠：不管谁启动的、是不是脚本拉起的子进程 |
| 进程（备） | 启动时记录的 PID（`%APPDATA%\com.tdev.app\agent_pids.json`，按脚本路径归一化做 key）→ `tasklist /FI "PID eq N"` 探活 | 覆盖无 HTTP 端口的自定义 Agent；TDev 重启后仍有效 |

返回：

```json
{ "running": true, "pid": 12345, "port": 8642, "listening": true,
  "trackedPid": 12345, "scriptOk": true }
```

`scriptOk=false` → 前端把状态文案写成「已停止 · 启动脚本不存在」，避免用户点启动才发现路径错。

## 4. 后端命令契约（`src-tauri/src/lib.rs`）

| 命令 | 参数 | 行为 |
|---|---|---|
| `local_agent_status` | `url, script` | 只读探测，不启动任何东西；前端 5s 轮询 |
| `local_agent_start` | `script, cwd?, cmdline?` | `cmd /C <cmdline>`，`CREATE_NO_WINDOW`（**绝不弹黑窗**）+ 独立进程组；记录 PID |
| `local_agent_stop` | `url, script` | 先按端口找 PID，再按记录 PID；`taskkill /PID x /T /F` 杀整棵进程树；等端口释放（≤5s）；清记录 |
| `local_agent_restart` | `url, script, cwd?, cmdline?` | stop → 等端口空闲 → 600ms → start |
| `local_agent_cmd_run` | `script, cwd?, cmdline?, wait_ms?` | 对话框「试运行」：捕获 stdout/stderr（GBK 解码），最多等 8s（可调，上限 60s），超时杀进程并标 `timedOut` |

安全边界：

- 只执行**用户在配置里指定的**脚本/命令行，不接受任意远端输入；
- 拒绝杀自己：`pid == std::process::id()` 时不执行；
- 启动一律无窗口 + stdout/stderr 丢弃（试运行除外），不污染桌面；
- 所有路径先做存在性校验，错误信息原样回前端（不静默失败）。

## 5. 前端 UI

### 5.1 新增/修改表单

在现有 5 个字段后追加（整行宽度）：

```
[✓] 本地 Agent（在本机启动，可用状态灯管理）
启动脚本 / 可执行文件  [__________________________] [浏览…]     ← 勾选后才显示
工作目录（可选）        [__________________________]              ← 勾选后才显示
```

校验：勾选本地但脚本为空 → `alert("勾选了本地 Agent，请填写启动脚本路径")`，不落库。

### 5.2 列表项（本地 Agent 多一行）

```
Hermes-Hulk  [默认]                          [修改] [设为默认] [删除]
hermes · http://127.0.0.1:8642 · Token: ***
────────────────────────────────────────────────────────────
● 运行中 · PID 12345 · 端口 8642   [启动] [重启] [停止] [命令行…]
```

状态灯三态：绿 `on`（运行中）、红 `off`（已停止/脚本不存在）、黄 `busy`（操作中，脉冲动画）。

### 5.3 轮询

- 进入 Agent配置页 → `agentLedStart(list)`，立即刷新一次，之后每 5s 一次；
- 切到别的菜单（`render(idx)` 里 `idx !== 7`）→ `agentLedStop()`，不留后台定时器；
- 操作（启动/停止/重启）后 0.8s、3s 各补一次刷新，不等下一轮。

### 5.4 命令行对话框

自定义模态（复用 `.modal/.modal-box`，不用 `window.confirm`）：

- 命令行（textarea，可编辑，默认 `cmd /C "<脚本>"`）
- 工作目录（input，默认脚本所在目录）
- `[关闭] [试运行（最多 8 秒看输出）] [执行启动]`
- 输出区 `<pre>` 显示试运行结果（stdout+stderr，最多 12k 字符）
- 「执行启动」会把命令行与工作目录**存回该 Agent 配置**，下次打开直接带出

## 6. 验收清单

| # | 项 | 判定 |
|---|---|---|
| 1 | 不勾本地 | 列表与现状一致，无状态灯、无多余按钮 |
| 2 | 勾本地但脚本空 | 保存被拦下并提示 |
| 3 | 勾本地 + 有效脚本 | 保存后列表出现状态灯与四个按钮 |
| 4 | 状态灯 | Agent 在跑 → 绿 + PID/端口；停掉 → 5s 内变红 |
| 5 | 启动 | 点击后 Agent 起来，无黑色控制台窗口 |
| 6 | 停止 | 进程树被杀干净（端口不再监听） |
| 7 | 重启 | 停止 → 启动，状态灯保持绿 |
| 8 | 命令行对话框 | 可改命令行 → 执行启动生效；试运行能看到输出 |
| 9 | 切菜单 | 轮询停止（不再 invoke `local_agent_status`） |
| 10 | 老配置 | 未含新字段的 Agent 打开/编辑/保存均正常 |

## 7. 已知取舍

- 脚本用 `start /min` 之类**自行脱离**时，cmd 立刻退出 → PID 跟踪失效，但**端口通道仍能正确判定运行中**，停止也按端口 PID 杀，行为不受影响；
- 试运行会**占用该 Agent 的启动过程**（8 秒内），只用于排错，正式启动请用「执行启动」；
- 端口解析支持 `http(s)://host:port/...` 与 IPv6 `[::1]:port`，无端口时按 80/443。

## 8. v2 变更（2026-11-18）：启停语义与优雅停止

### 8.1 启动对象改为启动器脚本

`script` 必须指向**自带动作的启动器脚本**（如 Hermes 的 `start_qq_channel.py`），不要指 `.bat`：

- `.bat` 末尾的 `pause` 会让 `cmd /C` 永久挂住（`agent_pids.json` 里留下永不退出的死 pid）；
- 启动器提供 `--json`（机器可读结果）与 `--stop` / `--restart`，TDev 才能拿退出码与结果。

`cmdline` 写绝对路径且**不加引号**（`cmd /C` 会把首尾引号当剥离标记）：

```
E:\HermesMinimal_v23\app\.venv\Scripts\python.exe E:\HermesMinimal_v23\start_qq_channel.py --json
```

### 8.2 停止/重启走优雅路径

`local_agent_stop` / `local_agent_restart` 新增 `la_graceful_launcher()`：

- 识别到 `start_qq_channel.py` → 先跑 `--stop --json` / `--restart --json --timeout 90`
- 失败才回落到「端口 PID → 杀进程树」

**为什么不能直接 taskkill**：强杀进行中的 turn 会写成 `unknown_execution`，整个会话冻结，必须人工 `sessions discard` 才能恢复。

### 8.3 期望状态记忆（desired state）

`app_meta.local_agent_desired`（`running` / `stopped`）：

| 用户操作 | 写入 |
|---|---|
| 启动 / 重启 | `running` |
| 停止 | `stopped` |

TDev 启动时（`setup()` 后台线程，延迟 1.2s）：

- `stopped` → **不拉起**，日志记 `skip: user stopped the agent last time`
- 其它 → 调一次幂等启动（在线时 0.2s 返回 `already running`，不打断会话）

日志：`%APPDATA%\com.tdev.app\hermes-autostart.log`。

### 8.4 按钮状态联动

`agentLedButtons(i, running)`：

- 运行中：启动禁用，重启/停止可用
- 已停止：启动可用，重启/停止禁用
- 操作进行中：三个全禁用（防连点）；状态取不到时回到「可启动」保守态

### 8.5 v2 验收补充

| # | 项 | 判定 |
|---|---|---|
| 11 | 点停止 → 关 TDev → 再开 | Agent 保持停止（灯红），不被自动拉起 |
| 12 | 点启动 → 关 TDev → 再开 | Agent 自动恢复在线 |
| 13 | 运行中点停止 | 走优雅停止，日志无 `exited UNCLEANLY` |
| 14 | 运行中按钮 | 启动灰、重启/停止可点 |
