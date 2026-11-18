# 模型参数自检与配置（config.yaml / .env / provider 三处一致）

> 版本：v1 · 2026-11-18 · 相关：`Agent配置` 页的「模型参数…」按钮、TDev 启动自检、`data/scripts/model_config.py`

## 1. 背景

Hermes 的模型参数散落在三个地方，任意一处不完整都会导致 Agent 起得来但**不能真正回话**：

| 来源 | 字段 | 说明 |
|---|---|---|
| `<Hermes>/data/config.yaml` | `model.default` / `model.provider` / `model.base_url` | 显式配置 |
| `<Hermes>/data/.env` | `<PROVIDER>_API_KEY` / `<PROVIDER>_BASE_URL` | 环境变量覆盖 |
| Hermes 内置 provider 注册表 | 默认端点、该 provider 认的环境变量名 | 78 项，见 `hermes_cli.auth.PROVIDER_REGISTRY` |

Hermes 运行期解析顺序（`hermes_cli/auth.py:2082`）：

```
config.model.base_url  →  环境变量 <PROVIDER>_BASE_URL  →  provider 内置默认（如 https://api.deepseek.com/v1）
```

**只看 config.yaml 会误判**：`base_url: ''` 但 `.env` 里有 `DEEPSEEK_BASE_URL` 时，Agent 照样能正常回话。

## 2. 目标

1. TDev 启动时自检模型参数，缺失则弹「Hermes 模型参数配置」窗口；
2. 弹窗展示**生效值 + 来源**（config.yaml / .env / provider 默认），而不是只显示 config 里的字面值；
3. 保存时**同时写** `config.yaml` 与 `.env`，让三处指向同一套参数；
4. 与 Hermes 版本保持一致 —— 不手写 provider 映射。

## 3. 实现

### 3.1 数据源：调 Hermes 自己的解析器

`<Hermes>/data/scripts/model_config.py`：

- `sys.path` 补 `<Hermes>/app` 与 `<Hermes>/app/.venv/Lib/site-packages`（绿色包必须，venv 的 `pyvenv.cfg` 带绝对 home，换盘失效）
- 用 `hermes_cli.auth.PROVIDER_REGISTRY` + `hermes_cli.config.read_raw_config()` 计算
- 输出一行 JSON：

```json
{
  "ok": false,
  "missing": ["DEEPSEEK_API_KEY"],
  "root": "E:\\HermesMinimal_v23",
  "configPath": "...\\data\\config.yaml",
  "envPath": "...\\data\\.env",
  "resolved": {
    "model":    { "value": "deepseek-v4.1-flash", "source": "config" },
    "provider": { "value": "deepseek",            "source": "config" },
    "baseUrl":  { "value": "https://opencode...", "source": "env:DEEPSEEK_BASE_URL",
                  "configValue": "", "envValue": "https://opencode...",
                  "envKey": "DEEPSEEK_BASE_URL", "providerDefault": "https://api.deepseek.com/v1" },
    "apiKey":   { "set": false, "source": "none", "envKeys": ["DEEPSEEK_API_KEY"], "fromConfig": false }
  },
  "providers": [ { "id": "deepseek", "name": "DeepSeek", "defaultBaseUrl": "...",
                   "apiKeyEnvs": ["DEEPSEEK_API_KEY"], "baseUrlEnv": "DEEPSEEK_BASE_URL" }, ... ],
  "providerDef": { ... }
}
```

`source` 取值：`config` / `env:<VAR>` / `provider-default` / `none`。

### 3.2 Rust 侧（`src-tauri/src/lib.rs`）

| 函数/命令 | 作用 |
|---|---|
| `run_model_probe(root)` | 用 `py314\python.exe`（优先）或 `app\.venv\Scripts\python.exe` 跑上述脚本，`CREATE_NO_WINDOW`，解析 stdout |
| `hermes_model_check` | 首选调用脚本；失败时用 Rust 直解析兜底（**同结构**，`engine: "fallback"`，`providers` 为空） |
| `hermes_model_save` | 备份 `config.yaml` / `.env` → 只改 `model:` 段的三个键 → 写 `.env` 的 `<PROVIDER>_API_KEY` 与 `<PROVIDER>_BASE_URL` |

> ⚠️ 改 `model:` 段时 `splice` 从 `start + 1` 开始 —— `start` 指向 `model:` 行本身，写错会把顶层键删掉。

### 3.3 前端（`dist/index.html`）

- 启动 2.5s 后调 `hermes_model_check`，`ok=false` 才弹窗（齐全时不打扰）
- 弹窗顶部是「模型 / Provider / Base URL / API Key」四行 = 生效值 + 来源标签
- Provider 输入框带 `<datalist>`（78 项），切换时显示该 provider 的默认端点 / 环境变量名
- 保存后提示写了哪些 env 键，并提供「立即重启 Agent」
- `Agent配置` 页另有「模型参数…」按钮（`hermesModelEnsure(true)`，即使齐全也显示当前值）

## 4. 验收清单

| # | 项 | 判定 |
|---|---|---|
| 1 | config 完整、.env 有 key | 启动不弹窗 |
| 2 | 删掉 `model.default` | 启动弹窗，`missing` 含 `model.default` |
| 3 | 清空 `model.base_url` 但 `.env` 有 `<P>_BASE_URL` | **不弹窗**（来源显示 `.env · XXX_BASE_URL`） |
| 4 | 再把 `.env` 的 `<P>_BASE_URL` 也去掉 | 弹窗，`missing` 含 `model.base_url` |
| 5 | 保存 | `config.yaml` 三个键 + `.env` 两个键同时更新，有 `.bak-tdev-<时间戳>` 备份 |
| 6 | 保存后重启 Agent | Agent 正常回话 |

## 5. 已知取舍

- 弹窗只覆盖「主模型」；`auxiliary.*`（视觉/评审等子模型）不在本窗口范围内；
- provider 列表随 Hermes 版本变化，TDev 不缓存 —— 每次自检都现读；
- 脚本缺失（如精简包没带）时退化为 Rust 兜底：仍能检查/保存，但没有 provider 下拉与默认端点提示。
