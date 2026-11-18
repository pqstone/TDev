# TDev + Hermes 绿色包 — 打包清单

> 打包产物示例：`E:\99.Package\TDev_Hermes_A_20261117\`
> 打包机上没有自动化打包脚本（历史手工打包），本文件是可照着执行的权威清单。
> 新增「必须随包」的文件时，**同时**更新本文件和包内 `README_部署说明.md`。

## 一、目录骨架

```
TDev_Hermes_A_<日期>\
├─ TDev\
│  ├─ tdev.exe                  ← src-tauri\target\release\tdev.exe
│  ├─ template\                 ← E:\88.project\TDev\template\（theme/devStandard/template.md）
│  └─ 启动TDev.bat              ← explorer.exe 起 tdev.exe（cmd 直起会白窗）
├─ Hermes\
│  ├─ py314\                    ← Python 3.14 基础运行时
│  ├─ app\                      ← Hermes 源码 + app\.venv（含 site-packages）
│  ├─ data\                     ← 纯净配置库（首启自建 state.db）
│  ├─ launcher.py
│  └─ start.bat
└─ README_部署说明.md
```

## 二、必须随包的关键文件

| # | 路径（包内） | 源（打包机） | 用途 | 缺失后果 |
|---|---|---|---|---|
| 1 | `Hermes\py314\python.exe` | `E:\HermesMinimal_v23\py314\` | 唯一可靠的运行时（`app\.venv\pyvenv.cfg` 带绝对 home，换盘失效） | Hermes 起不来 |
| 2 | `Hermes\app\` | `E:\HermesMinimal_v23\app\` | Hermes 源码 + venv site-packages | 起不来 |
| 3 | `Hermes\data\config.yaml` | `E:\HermesMinimal_v23\data\config.yaml`（清空 key/QQ 后） | 模型 / 平台配置 | gateway 拒绝启动 |
| 4 | `Hermes\data\.env.example` | 同上 `.env` 脱敏版 | 首配模板 | 客户不知道填哪些 key |
| 5 | **`Hermes\data\scripts\model_config.py`** | **`E:\HermesMinimal_v23\data\scripts\model_config.py`** | **TDev「模型参数」弹窗的数据源：读 Hermes 自己的 `PROVIDER_REGISTRY`(78 项) + `read_raw_config()` + `.env`，产出「生效值 + 来源」** | 退化为 Rust 兜底：仍能弹窗/保存，但**没有 provider 下拉、没有默认端点提示** |
| 6 | `TDev\template\` | `E:\88.project\TDev\template\` | 主题 / devStandard / 项目模板 | 建项目拿不到模板 |
| 7 | `TDev\tdev.exe` | `src-tauri\target\release\tdev.exe` | 桌面端（前端已内嵌） | — |

### 待补项（尚未随包，勿遗忘）

| 路径 | 说明 |
|---|---|
| `Hermes\data\scripts\start_qq_channel.py` | TDev 的启动/停止/重启优雅路径（`--json` / `--stop` / `--restart`）。包内缺失时 TDev 会退回 `cmd /C` + 杀进程树。⚠️ 直接复制**不可用**：脚本里硬编码了开发机的 `8642` 与 `<本机 api key>`，包内 api key 是随机生成的，需先改成从 `config.yaml` 的 `gateway.platforms.api_server.extra.key` 读取 |

## 三、打包后校验（必须实跑）

```bat
:: 1) 运行时
Hermes\py314\python.exe --version

:: 2) 模型参数探针（TDev 弹窗的数据源）
cd Hermes
py314\python.exe data\scripts\model_config.py
:: 期望：一行 JSON，含 "providers":[...]（≥70 项）、"resolved":{...}
:: 若 .env 缺失：ok=false，missing 含 DEEPSEEK_API_KEY —— 属正常（首次配置前）

:: 3) 隔离起网关（别用客户默认端口/数据）
set HERMES_HOME=<临时目录>
py314\python.exe launcher.py gateway run
:: 另开窗口：curl -H "Authorization: Bearer <api key>" http://127.0.0.1:8642/v1/models → 200
```

## 四、其它注意事项

- `app\.venv\pyvenv.cfg` 的 `home` 是绝对路径，**换盘/换机后 venv 不可用**，一切调用都走 `py314\python.exe` + 脚本内补 `site-packages`。
- `tdev.db` 在 `%APPDATA%\com.tdev.app\`，**不随包**；客户机首启为空，TDev 的 Agent 列表需要重新配置（或后续做「自动发现同级 Hermes」）。
- TDev 模板路径已于 2026-11-18 改为相对路径：`<exe目录>\template` 优先、源码布局 `<exe目录>\..\..\..\src\template` 兜底；
  代码与界面文案里已无 `E:\88.project\...`（`.bak` 文件除外）。包内只要 `tdev.exe` 与 `template\` 同级即可。
