// ============================================================
// TDev 后端入口：插件注册 + 自定义 Tauri Command
// 规范：新增 IPC 命令时，在此使用 #[tauri::command] 定义，
//       并加入下方 generate_handler! 列表；相应权限在
//       capabilities/default.json 中最小化授予。
// ============================================================

use rusqlite::{params, Connection};
use tauri::Manager;
use tauri::Emitter;

// ============================================================
// 全局 Agent 互斥锁（2026-11-06 卢Sir 定：只允许一个 Agent 运行）
// ------------------------------------------------------------
// 病根：TDev 每个场景都能自己发 run，前端防重入只管得住「计划按钮」，
// 管不住 ASK 回答 / 「看看」/ 聊天提问等旁路入口；多点几次就是 3 个 run 并发
// 写同一个项目目录 —— 代码互相覆盖、Agent 自己都能发现「文件被并发修改」，
// 而且每个 run 都带完整上下文（2 万 token 起），并发就是成倍烧钱。
// 这里做硬锁：进程内同一时刻只放行一个 agent_ask_stream。
// ============================================================
use std::sync::atomic::{AtomicI64, Ordering as AtomicOrdering};

/// 0 = 空闲；非 0 = 当前持有锁的 Unix 秒时间戳
static AGENT_BUSY_SINCE: AtomicI64 = AtomicI64::new(0);
/// 锁的兜底有效期：run 卡死（SSE 挂住）时不能让锁永久焊死，超过这个时长可被抢占
const AGENT_LOCK_TTL_SECS: i64 = 1800;   // 30 分钟

fn agent_now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 拿锁：已有活锁则拒绝（返回给前端的错误文案要能直接展示给用户）
fn agent_lock_try() -> Result<(), String> {
    let now = agent_now_secs();
    let prev = AGENT_BUSY_SINCE.load(AtomicOrdering::SeqCst);
    if prev != 0 {
        let held = (now - prev).max(0);
        if held < AGENT_LOCK_TTL_SECS {
            return Err(format!(
                "已有 Agent 正在运行（已 {} 分 {} 秒），同一时间只允许一个。等它结束再点，或到「工具设置」强制解锁。",
                held / 60, held % 60
            ));
        }
        // 超过 TTL 视为上一轮已经死了（进程崩溃 / SSE 断链没走到释放）
        AGENT_BUSY_SINCE.store(0, AtomicOrdering::SeqCst);
    }
    AGENT_BUSY_SINCE.store(now, AtomicOrdering::SeqCst);
    Ok(())
}

fn agent_lock_release() {
    AGENT_BUSY_SINCE.store(0, AtomicOrdering::SeqCst);
}

/// RAII 守卫：函数任意 return / 出错 / panic 展开都会自动释放，不会漏
struct AgentLockGuard;
impl Drop for AgentLockGuard {
    fn drop(&mut self) {
        agent_lock_release();
    }
}

/// 查当前锁状态，前端用来显示「谁在跑、跑了多久」
#[tauri::command]
fn agent_lock_state() -> serde_json::Value {
    let prev = AGENT_BUSY_SINCE.load(AtomicOrdering::SeqCst);
    if prev == 0 {
        serde_json::json!({ "busy": false, "held_secs": 0 })
    } else {
        serde_json::json!({ "busy": true, "held_secs": (agent_now_secs() - prev).max(0) })
    }
}

/// 强制解锁：前端「强制解锁」按钮用。真卡死时唯一的逃生口。
#[tauri::command]
fn agent_force_unlock() -> serde_json::Value {
    let prev = AGENT_BUSY_SINCE.swap(0, AtomicOrdering::SeqCst);
    serde_json::json!({ "ok": true, "was_busy": prev != 0 })
}
#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 系统构建前先把目录骨架建好，Agent 只负责往里填内容。
/// 原则（2026-11-05 卢Sir 定）：
///   ① 公共运行时（数据库 / JDK / Web 服务 / 依赖缓存）放 工作空间/Public，多项目共用，不重复安装；
///   ② 项目自己的一切都落在 工作空间/<项目名>/ 内，不外溢 —— 整个目录拷走就能迁移部署。
#[tauri::command]
fn project_ensure_dirs(prj_name: String, workspace: String) -> Result<serde_json::Value, String> {
    let name = prj_name.trim().to_string();
    let raw_ws = workspace.trim().to_string();
    let ws = raw_ws.trim_end_matches('/').trim_end_matches('\\').to_string();
    if name.is_empty() || ws.is_empty() {
        return Err("项目名和工作空间不能为空".to_string());
    }
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err("项目名不合法，不能包含路径分隔符".to_string());
    }
    // 工作空间是反斜杠风格就全程用反斜杠，别拼出混合分隔符（会原样出现在给 Agent 的指令里）。
    let sep = if ws.contains('\\') { '\\' } else { '/' };
    let proj_raw = format!("{}/{}", ws, name);
    let mut dirs: Vec<String> = vec![
        format!("{}/Public", ws),
        format!("{}/Public/runtime", ws),
        format!("{}/Public/cache", ws),
        format!("{}/Public/downloads", ws),
        format!("{}/app/src", proj_raw),
        format!("{}/app/web", proj_raw),
        format!("{}/app/sql", proj_raw),
        format!("{}/app/config", proj_raw),
        format!("{}/data", proj_raw),
        format!("{}/data/upload", proj_raw),
        format!("{}/logs", proj_raw),
        format!("{}/build", proj_raw),
    ];
    let proj = if sep == '\\' { proj_raw.replace('/', "\\") } else { proj_raw };
    if sep == '\\' {
        dirs = dirs.iter().map(|d| d.replace('/', "\\")).collect();
    }
    let mut created: Vec<String> = Vec::new();
    for d in dirs.iter() {
        if std::path::Path::new(d).exists() {
            continue;
        }
        std::fs::create_dir_all(d).map_err(|e| format!("建目录失败 {d}: {e}"))?;
        created.push(d.clone());
    }
    let public = if sep == '\\' {
        format!("{}\\Public", ws)
    } else {
        format!("{}/Public", ws)
    };
    Ok(serde_json::json!({
        "ok": true,
        "project": proj,
        "public": public,
        "created": created,
        "total": dirs.len()
    }))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 确保 AppConfig 目录存在：SQLite 库文件 tdev.db 存放于此
            let dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&dir)
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            create_folder,
            run_script,
            db_init,
            project_ensure_dirs,
            project_list,
            project_create,
            project_delete,
            cfg_save,
            cfg_load,
            agent_ask,
            agent_ask_stream,
            agent_lock_state,
            agent_force_unlock,
            project_save_theme,
            theme_preview_html,
            theme_read_project,
            stack_check,
            ironlaw_check,
            constr_read_tpl,
            constr_save,
            constr_context,
            process_save,
            process_list,
            process_load,
            process_auto_load,
            process_versions,
            process_restore,
            flow_backup,
            flow_backup_list,
            flow_backup_load,
            module_state_set,
            module_state_get
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// 获取本地 SQLite 库文件路径（AppConfig 目录下 tdev.db）
fn db_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("定位 AppConfig 目录失败: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    Ok(dir.join("tdev.db"))
}

/// 打开数据库连接（统一入口）
fn db_conn(app: &tauri::AppHandle) -> Result<Connection, String> {
    let path = db_path(app)?;
    Connection::open(&path).map_err(|e| format!("打开数据库失败: {e}"))
}

/// 当前时间字符串（YYYY-MM-DD HH:MM:SS，纯 std）
fn now_str() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs();
    let days = secs / 86400;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let mut y = 1970i64;
    let mut dleft = days as i64;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let yd = if leap { 366 } else { 365 };
        if dleft >= yd { dleft -= yd; y += 1; } else { break; }
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
    let mdays = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mon = 1usize;
    let mut dleft2 = dleft;
    for (i, md) in mdays.iter().enumerate() {
        if dleft2 >= *md { dleft2 -= *md; mon = i + 2; } else { mon = i + 1; break; }
    }
    format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", y, mon, dleft2 + 1, h, m, s)
}

/// 初始化本地 SQLite：建库 + 建表骨架 + 写入初始配置
/// 前端通过 invoke('db_init') 调用；返回库路径与数据表清单
#[tauri::command]
fn db_init(app: tauri::AppHandle) -> Result<String, String> {
    let path = db_path(&app)?;
    let conn = db_conn(&app)?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS app_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS demo_items (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            status TEXT DEFAULT 'active',
            created_at TEXT DEFAULT (datetime('now'))
         );
         CREATE TABLE IF NOT EXISTS projects (
            prjGUID TEXT PRIMARY KEY,
            prjName TEXT NOT NULL,
            prjType TEXT NOT NULL,
            prjDescription TEXT,
            created_at TEXT DEFAULT (datetime('now'))
         );
         INSERT OR IGNORE INTO app_meta(key, value) VALUES('theme', 'arctic-glacier-blue');",
    )
    .map_err(|e| format!("初始化表失败: {e}"))?;

    // 读取数据表清单
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .map_err(|e| format!("查询失败: {e}"))?;
    let tables: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .map_err(|e| format!("读取失败: {e}"))?
        .filter_map(|x| x.ok())
        .collect();

    let joined = if tables.is_empty() { "(无)".to_string() } else { tables.join(", ") };
    Ok(format!("SQLite 已初始化 @ {} | 数据表: {}", path.display(), joined))
}

/// 项目列表（按创建时间倒序），供下拉框使用
#[tauri::command]
fn project_list(app: tauri::AppHandle) -> Result<Vec<serde_json::Value>, String> {
    let conn = db_conn(&app)?;
    let mut stmt = conn
        .prepare(
            "SELECT prjGUID, prjName, prjType, prjDescription, created_at
             FROM projects ORDER BY created_at DESC",
        )
        .map_err(|e| format!("查询失败: {e}"))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(serde_json::json!({
                "prjGUID": r.get::<_, String>(0)?,
                "prjName": r.get::<_, String>(1)?,
                "prjType": r.get::<_, String>(2)?,
                "prjDescription": r.get::<_, Option<String>>(3)?,
                "created_at": r.get::<_, String>(4)?
            }))
        })
        .map_err(|e| format!("读取失败: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 新增项目：自动生成 prjGUID（UUID v4），参数化插入，并在工作空间创建项目目录、复制模板、生成项目说明
/// 返回 JSON: {"guid": "...", "steps": [{"step": "数据库", "ok": true, "msg": "..."}, ...]}
#[tauri::command]
fn project_create(
    app: tauri::AppHandle,
    prj_name: String,
    prj_type: String,
    prj_description: String,
) -> Result<serde_json::Value, String> {
    let guid = uuid::Uuid::new_v4().to_string();
    let mut steps: Vec<serde_json::Value> = Vec::new();

    // 1. 数据库插入
    let conn = db_conn(&app)?;
    conn.execute(
        "INSERT INTO projects (prjGUID, prjName, prjType, prjDescription) VALUES (?1, ?2, ?3, ?4)",
        params![guid, prj_name, prj_type, prj_description],
    )
    .map_err(|e| format!("保存失败: {e}"))?;
    steps.push(serde_json::json!({"step": "数据库", "ok": true, "msg": "已写入项目表"}));

    // 1.5 初始化模块状态表（统一亮灯数据源）
    // 基础信息=1（项目已建）、项目约束=1（模板已复制）、其余=0
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS module_state (
            prj_guid TEXT NOT NULL,
            module TEXT NOT NULL,
            filled INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (prj_guid, module)
        )", [],
    );
    let now0 = now_str();
    for (m, f) in [("basic", 1), ("constraints", 1), ("theme", 0), ("process", 0), ("ai_code", 0), ("deploy", 0)] {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO module_state (prj_guid, module, filled, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![guid, m, f, now0],
        );
    }
    steps.push(serde_json::json!({"step": "模块状态", "ok": true, "msg": "已初始化亮灯状态"}));

    // 2. 读取工作空间路径
    let workspace: String = {
        let mut stmt = conn
            .prepare("SELECT value FROM app_meta WHERE key = 'workspace'")
            .map_err(|e| format!("读取工作空间失败: {e}"))?;
        let mut rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| format!("读取工作空间失败: {e}"))?;
        rows.next().transpose().map_err(|e| format!("读取工作空间失败: {e}"))?
            .and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
            .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(String::from))
            .unwrap_or_default()
    };

    if workspace.is_empty() {
        return Ok(serde_json::json!({
            "guid": guid,
            "steps": steps,
            "error": "未设置工作空间路径，请先在项目空间页设定"
        }));
    }

    // 3. 创建项目目录（工作空间 + 项目名，清理非法字符）
    let safe_name: String = prj_name.trim().to_string();
    if safe_name.is_empty() {
        return Ok(serde_json::json!({"guid": guid, "steps": steps, "error": "项目名称不能作为目录名"}));
    }
    let proj_dir = std::path::Path::new(&workspace).join(&safe_name);
    std::fs::create_dir_all(&proj_dir).map_err(|e| format!("创建项目目录失败: {e}"))?;
    steps.push(serde_json::json!({"step": "目录", "ok": true, "msg": format!("已创建 {}", proj_dir.display())}));

    // 4. 复制模板目录（src/template）到项目目录
    let template_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/template");
    if template_dir.exists() {
        let copy_result = copy_dir_recursive(&template_dir, &proj_dir);
        match copy_result {
            Ok(n) => steps.push(serde_json::json!({"step": "模板", "ok": true, "msg": format!("已复制 {n} 个文件")})),
            Err(e) => steps.push(serde_json::json!({"step": "模板", "ok": false, "msg": format!("复制失败: {e}")})),
        }
        // 4.1 将 template.md 重命名为 {项目名}.md
        let tpl_md = proj_dir.join("template.md");
        if tpl_md.exists() {
            let new_md = proj_dir.join(format!("{}.md", safe_name));
            std::fs::rename(&tpl_md, &new_md).map_err(|e| format!("重命名模板文档失败: {e}"))?;
        }
        // 4.2 替换 devStandard 下所有 md 的项目占位信息
        let dev_std_dir = proj_dir.join("devStandard");
        if dev_std_dir.exists() {
            let replaced = replace_md_placeholders(&dev_std_dir, &prj_name, &prj_description);
            match replaced {
                Ok(n) => steps.push(serde_json::json!({"step": "文档", "ok": true, "msg": format!("已更新 {n} 个文档")})),
                Err(e) => steps.push(serde_json::json!({"step": "文档", "ok": false, "msg": format!("更新文档失败: {e}")})),
            }
        }
    } else {
        steps.push(serde_json::json!({"step": "模板", "ok": false, "msg": "模板目录不存在"}));
    }

    // 5. 重命名后的项目文档 {项目名}.md 写入真实项目信息
    let proj_md = proj_dir.join(format!("{}.md", safe_name));
    if proj_md.exists() {
        let md_content = format!(
            "{}

# {}

- 项目名称：{}
- 项目ID：{}
- 项目类型：{}
- 项目说明：{}
",
            guid, prj_name, prj_name, guid, prj_type, prj_description
        );
        std::fs::write(&proj_md, md_content).map_err(|e| format!("写项目文档失败: {e}"))?;
    }
    // 5.1 生成 README.md（项目说明副本）
    let desc_content = format!(
        "{}

# {}

- 项目名称：{}
- 项目ID：{}
- 项目类型：{}
- 项目说明：{}
",
        guid, prj_name, prj_name, guid, prj_type, prj_description
    );
    let desc_path = proj_dir.join("README.md");
    std::fs::write(&desc_path, desc_content).map_err(|e| format!("写项目说明失败: {e}"))?;
    steps.push(serde_json::json!({"step": "说明", "ok": true, "msg": "已生成 README.md"}));

    Ok(serde_json::json!({"guid": guid, "steps": steps}))
}

/// 递归替换 devStandard 下所有 .md 文件的占位内容
/// 规则：项目一句话定位占位符 → 项目说明；com.example → com.{项目名}
fn replace_md_placeholders(dir: &std::path::Path, prj_name: &str, prj_desc: &str) -> Result<usize, String> {
    let mut count = 0usize;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            count += replace_md_placeholders(&path, prj_name, prj_desc)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let orig_content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let mut new_content = orig_content.clone();
            // 项目一句话定位占位符
            new_content = new_content.replace(
                "[在此填写项目简介，例如：面向内部运营的订单查询与数据看板系统]",
                prj_desc,
            );
            // com.example → com.{项目名小写}
            let pkg = prj_name.to_lowercase().replace(' ', "_");
            new_content = new_content.replace("com.example", &format!("com.{}", pkg));
            if new_content != orig_content {
                std::fs::write(&path, new_content).map_err(|e| e.to_string())?;
                count += 1;
            }
        }
    }
    Ok(count)
}

/// 递归复制目录，返回复制的文件数
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<usize, String> {
    let mut count = 0usize;
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            count += copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    Ok(count)
}

/// 删除项目：按 prjGUID 删除，参数化绑定，返回是否删除成功
#[tauri::command]
fn project_delete(app: tauri::AppHandle, prj_guid: String) -> Result<bool, String> {
    let conn = db_conn(&app)?;
    let n = conn
        .execute(
            "DELETE FROM projects WHERE prjGUID = ?1",
            params![prj_guid],
        )
        .map_err(|e| format!("删除失败: {e}"))?;
    Ok(n > 0)
}

/// 通用配置保存：category 为配置分组（workspace/models/agents），payload 为 JSON 字符串，存 app_meta 表
#[tauri::command]
fn cfg_save(app: tauri::AppHandle, category: String, payload: String) -> Result<bool, String> {
    let conn = db_conn(&app)?;
    conn.execute(
        "INSERT INTO app_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        params![category, payload],
    )
    .map_err(|e| format!("保存配置失败: {e}"))?;
    Ok(true)
}

/// 通用配置读取：返回 category 对应的 JSON 字符串（不存在返回空字符串）
#[tauri::command]
fn cfg_load(app: tauri::AppHandle, category: String) -> Result<String, String> {
    let conn = db_conn(&app)?;
    let mut stmt = conn
        .prepare("SELECT value FROM app_meta WHERE key = ?1")
        .map_err(|e| format!("读取配置失败: {e}"))?;
    let mut rows = stmt
        .query_map(params![category], |r| r.get::<_, String>(0))
        .map_err(|e| format!("读取配置失败: {e}"))?;
    let val = rows.next().transpose().map_err(|e| format!("读取配置失败: {e}"))?;
    Ok(val.unwrap_or_default())
}

/// 调用 Hermes CLI 问答（-z 模式），支持指定会话 ID 保持上下文
/// 返回 Agent 的回复文本
#[tauri::command]
async fn agent_ask(question: String, session_id: String) -> Result<String, String> {
    // 和流式对话共用同一把锁：主题生成 / 流程梳理 / 计划执行，全程只允许一个 Agent。
    agent_lock_try()?;
    let _lock_guard = AgentLockGuard;

    // async 命令：直接 async reqwest（配合 Tauri async runtime，UI 不冻结）
    // TD：TDev 专用 Agent 实例（独立进程、独立 profile、独立端口 8644），
    // 与 QQ/Hulk 用的 8642 分开，避免长会话把开发请求拖住。
    let url = "http://127.0.0.1:8644/v1/chat/completions";
    let body = serde_json::json!({
        "model": "hermes-agent",
        "messages": [{"role": "user", "content": question}]
    });
    let client = reqwest::Client::new();
    let resp = client.post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer td-tdev-2026-key")
        .header("X-Hermes-Session-Id", session_id)
        .body(body.to_string())
        // Agent 执行时长不可预期：超时设为 1 小时（足够长）
        .timeout(std::time::Duration::from_secs(3600))
        .send()
        .await
        .map_err(|e| format!("请求 Hermes API 失败: {e}"))?;
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    // 解析 OpenAI 格式
    let parsed: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("解析响应失败: {e} (原始: {})", &text[..text.len().min(200)]))?;
    if let Some(content) = parsed["choices"][0]["message"]["content"].as_str() {
        Ok(content.trim().to_string())
    } else if let Some(err) = parsed["error"]["message"].as_str() {
        Err(format!("Hermes 错误: {err}"))
    } else {
        Ok("(无回复)".to_string())
    }
}

/// 流式 Agent 对话：POST /v1/runs 启动 → SSE 读取事件 → 通过 Tauri 事件推送增量
/// 事件推送：emit "agent_stream" {type: reasoning|delta|tool|done, text}
#[tauri::command]
async fn agent_ask_stream(
    app: tauri::AppHandle,
    window: tauri::Window,
    question: String,
    session_id: String,
    instructions: Option<String>,
    cwd: Option<String>,
) -> Result<String, String> {
    // 硬锁：同一时刻只允许一个 Agent run。守卫在任何 return / 出错路径都会自动释放。
    agent_lock_try()?;
    let _lock_guard = AgentLockGuard;

    // TD 实例（8644）：TDev 专用 Agent，与 QQ 用的 default(8642) 进程隔离
    let base = "http://127.0.0.1:8644";
    let key = "td-tdev-2026-key";
    let client = reqwest::Client::new();

    // 1. 启动 run
    // cwd：把 Agent 的工作目录钉在当前项目里，否则它会在 Hermes 自己的目录里生成代码。
    // 空值不下发，保持 Hermes 默认行为。
    let mut run_body = serde_json::json!({
        "input": question,
        "instructions": instructions.unwrap_or_default(),
        "conversation_history": []
    });
    if let Some(dir) = cwd {
        let dir = dir.trim().to_string();
        if !dir.is_empty() {
            run_body["cwd"] = serde_json::Value::String(dir);
        }
    }
    let resp = client.post(format!("{base}/v1/runs"))
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {key}"))
        .header("X-Hermes-Session-Id", session_id.clone())
        .body(run_body.to_string())
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("启动运行失败: {e}"))?;
    let run_text = resp.text().await.map_err(|e| format!("读取运行响应失败: {e}"))?;
    let run_parsed: serde_json::Value = serde_json::from_str(&run_text)
        .map_err(|e| format!("解析运行响应失败: {e}"))?;
    let run_id = run_parsed.get("run_id").and_then(|v| v.as_str())
        .ok_or_else(|| format!("未获取到 run_id: {run_text}"))?;

    // 2. SSE 流式读取
    let events_url = format!("{base}/v1/runs/{run_id}/events");
    let sse_resp = client.get(&events_url)
        .header("Authorization", format!("Bearer {key}"))
        .header("X-Hermes-Session-Id", session_id)
        .timeout(std::time::Duration::from_secs(3600))
        .send()
        .await
        .map_err(|e| format!("连接事件流失败: {e}"))?;

    let mut final_text = String::new();
    let mut reasoning_text = String::new();
    let mut stream = sse_resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    use futures_util::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取流失败: {e}"))?;
        buf.extend_from_slice(&chunk);
        // 按行解析 SSE 事件块（data 行，空行分隔）
        while let Some(pos) = buf.windows(2).position(|w| w == b"\n\n") {
            let line_block: Vec<u8> = buf.drain(..pos + 2).collect();
            let block_str = String::from_utf8_lossy(&line_block);
            for line in block_str.lines() {
                let line = line.trim();
                if !line.starts_with("data:") { continue; }
                let data = line[5..].trim();
                if data.is_empty() { continue; }
                let ev: serde_json::Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let ev_type = ev.get("event").and_then(|v| v.as_str()).unwrap_or("");
                if ev_type == "reasoning.available" {
                    if let Some(t) = ev.get("text").and_then(|v| v.as_str()) {
                        reasoning_text.push_str(t);
                        let _ = window.emit("agent_stream", serde_json::json!({
                            "type": "reasoning", "text": t
                        }));
                    }
                } else if ev_type == "message.delta" {
                    if let Some(d) = ev.get("delta").and_then(|v| v.as_str()) {
                        final_text.push_str(d);
                        let _ = window.emit("agent_stream", serde_json::json!({
                            "type": "delta", "text": d
                        }));
                    }
                } else if ev_type == "tool.started" {
                    // 字段名是 tool，不是 tool_name —— 读错名字会让界面只显示「🔧 调用工具: 」
                    // 而看不到到底在跑什么，用户就以为卡死了。
                    let name = ev.get("tool").and_then(|v| v.as_str())
                        .or_else(|| ev.get("tool_name").and_then(|v| v.as_str()))
                        .unwrap_or("");
                    let pv = ev.get("preview").and_then(|v| v.as_str()).unwrap_or("");
                    let mut desc = name.to_string();
                    if !pv.is_empty() {
                        let pv_short: String = pv.chars().take(60).collect();
                        desc.push_str("：");
                        desc.push_str(&pv_short);
                        if pv.chars().count() > 60 { desc.push_str("…"); }
                    }
                    let _ = window.emit("agent_stream", serde_json::json!({
                        "type": "tool", "text": desc, "tool": name
                    }));
                } else if ev_type == "tool.completed" {
                    // 没有这个事件，前端在工具跑完后就不知道该更新什么，
                    // 只能干等下一段文字 —— 装 Tomcat 那种几分钟的活看着就像死了。
                    let name = ev.get("tool").and_then(|v| v.as_str())
                        .or_else(|| ev.get("tool_name").and_then(|v| v.as_str()))
                        .unwrap_or("");
                    let dur = ev.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let is_err = ev.get("error").and_then(|v| v.as_bool()).unwrap_or(false);
                    let _ = window.emit("agent_stream", serde_json::json!({
                        "type": "tool_done", "text": name, "duration": dur, "error": is_err
                    }));
                } else if ev_type == "run.completed" {
                    let _ = window.emit("agent_stream", serde_json::json!({
                        "type": "done", "text": ""
                    }));
                    return Ok(final_text.trim().to_string());
                } else if ev_type == "run.failed" {
                    let err = ev.get("error").and_then(|v| v.as_str()).unwrap_or("运行失败");
                    let _ = window.emit("agent_stream", serde_json::json!({
                        "type": "error", "text": err
                    }));
                    return Err(err.to_string());
                }
            }
        }
    }
    let _ = window.emit("agent_stream", serde_json::json!({"type": "done", "text": ""}));
    Ok(final_text.trim().to_string())
}

/// 保存 AI 生成的主题三件套到项目目录（workspace/项目名/theme/：tokens.json + ui-preview.html + theme.md）
#[tauri::command]
fn project_save_theme(prj_guid: String, prj_name: String, workspace: String, content: String) -> Result<String, String> {
    let dir = format!("{}/{}/theme", workspace.trim_end_matches('/'), prj_name.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    // 先提取色系（避免 content 移动后再借用）
    let accent = extract_accent(&content).unwrap_or_else(|| "#0067C0".to_string());
    let palette = extract_palette(&content).unwrap_or_else(|| generate_palette(&accent));
    let color_map = extract_color_map(&content);
    // 1) tokens.json（Agent 生成内容）
    let tokens_path = format!("{}/tokens.json", dir);
    std::fs::write(&tokens_path, content).map_err(|e| format!("写入 tokens.json 失败: {e}"))?;
    let preview = build_preview_html(&accent, &palette, &color_map);
    let preview_path = format!("{}/ui-preview.html", dir);
    std::fs::write(&preview_path, preview).map_err(|e| format!("写入 ui-preview.html 失败: {e}"))?;
    // 3) theme.md（复制模板说明，标注本项目主题信息）
    let tpl_md = "E:/88.project/TDev/src/template/theme/theme.md";
    let md = std::fs::read_to_string(tpl_md).unwrap_or_default();
    let md = format!("> 本项目主题由 AI 生成。主色：{}。

{}", accent, md);
    let md_path = format!("{}/theme.md", dir);
    std::fs::write(&md_path, md).map_err(|e| format!("写入 theme.md 失败: {e}"))?;
    Ok(format!("tokens.json + ui-preview.html + theme.md"))
}

/// 用色系数据构建预览页（复用 theme_preview_html 的替换逻辑）
fn build_preview_html(accent: &str, palette: &std::collections::HashMap<String, String>, color_map: &std::collections::HashMap<String, String>) -> String {
    let tpl = "E:/88.project/TDev/src/template/theme/ui-preview.html";
    let mut out = std::fs::read_to_string(tpl).unwrap_or_default();
    for step in ["50", "100", "200", "300", "400", "500", "600", "700", "800", "900"] {
        if let Some(color) = palette.get(step) {
            let old_var = format!("--color-accent-{}: #{};", step, template_palette_value(step));
            let new_var = format!("--color-accent-{}: {};", step, color);
            out = out.replace(&old_var, &new_var);
            let em_old = format!("<em>#{}</em>", template_palette_value(step));
            let em_new = format!("<em>{}</em>", color);
            out = out.replace(&em_old, &em_new);
        }
    }
    for (var_name, token_key, light_tpl, dark_tpl) in [
        ("--color-background", "background", "F3F3F3", "202020"),
        ("--color-surface", "surface", "FFFFFF", "2B2B2B"),
        ("--color-surface-subtle", "surfaceSubtle", "F3F3F3", "3A3A3A"),
        ("--color-surface-hover", "surfaceHover", "F0F0F0", "363636"),
        ("--color-text-primary", "textPrimary", "1B1B1B", "FFFFFF"),
        ("--color-text-secondary", "textSecondary", "5F5F5F", "C0C0C0"),
        ("--color-text-tertiary", "textTertiary", "A0A0A0", "858585"),
        ("--color-text-on-accent", "textOnAccent", "FFFFFF", "003B70"),
        ("--color-accent", "accent", "0067C0", "60CDFF"),
        ("--color-accent-hover", "accentHover", "005FB8", "4CC2FF"),
        ("--color-accent-pressed", "accentPressed", "005299", "1E9CE6"),
        ("--color-border", "border", "D6D6D6", "454545"),
        ("--color-border-strong", "borderStrong", "C8C8C8", "5A5A5A"),
        ("--color-selection", "selection", "CCE4F7", "1A3E5C"),
        ("--color-success", "success", "0F7B0F", "6CCB5F"),
        ("--color-warning", "warning", "9D5D00", "FCE100"),
        ("--color-error", "error", "C42B1C", "FF6B6B"),
        ("--color-error-bg", "errorBg", "FDE7E9", "4C1D24"),
        ("--color-info", "info", "0067C0", "60CDFF"),
        ("--color-focus-ring", "focusRing", "0067C0", "60CDFF"),
    ] {
        if let Some(c) = color_map.get(&format!("light.{token_key}")) {
            let old_v = format!("{}: #{};", var_name, light_tpl);
            let new_v = format!("{}: {};", var_name, c);
            out = out.replace(&old_v, &new_v);
        }
        if let Some(c) = color_map.get(&format!("dark.{token_key}")) {
            let old_v = format!("{}: #{};", var_name, dark_tpl);
            let new_v = format!("{}: {};", var_name, c);
            out = out.replace(&old_v, &new_v);
        }
    }
    let inject = "<style>
html,body { height:100%; margin:0; }
.layout { height:100%; min-height:100%; }
.sidebar { overflow-y:auto; }
main { overflow-y:auto; }
</style>";
    if out.contains("</head>") {
        out = out.replace("</head>", &format!("{inject}</head>"));
    }
    out
}

/// 生成主题预览页：读取模板 ui-preview.html，用生成的 tokens.json 主色替换 accent 色值
#[tauri::command]
fn theme_preview_html(content: String) -> Result<String, String> {
    // 解析生成的 tokens.json，提取完整色系（优先 Agent 生成的 palette，缺则兜底）
    let accent = extract_accent(&content).unwrap_or_else(|| "#0067C0".to_string());
    let palette = extract_palette(&content).unwrap_or_else(|| generate_palette(&accent));
    let color_map = extract_color_map(&content);
    let out = build_preview_html(&accent, &palette, &color_map);
    Ok(out)
}

/// 从项目 theme 目录读取主题文件（tokens.json / ui-preview.html / theme.md），返回指定文件内容
#[tauri::command]
fn theme_read_project(prj_name: String, workspace: String, file: String) -> Result<String, String> {
    let safe_file = if file == "tokens.json" || file == "ui-preview.html" || file == "theme.md" { file } else { "tokens.json".to_string() };
    let path = format!("{}/{}/theme/{}", workspace.trim_end_matches('/'), prj_name.trim(), safe_file);
    std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

/// 技术栈硬校验：扫项目根基建产物，检查是否出现被禁止的框架/构建工具。
/// forbid 由前端从 AGENTS.md 第 2 章「禁止」列提取（英文关键词，逗号分隔）—— 规则不在代码里写第二份。
/// 铁律扫描：把 ARCHITECTURE.md 第10章里「有代码痕迹」的铁律变成可扫规则，命中即红牌。
/// 只做能拿到硬证据的：铁律1 硬编码身份 / 铁律2 绕过 api.js / 铁律3 DAO 无 DataScope /
/// 铁律4 建表缺四尾字段 / 铁律6 写死颜色 / 分层：web 层直接连库。
/// 靠自觉的条目（备份、YAGNI、模块边界、计划确认）这里不假装能校验。
#[tauri::command]
fn ironlaw_check(prj_name: String, workspace: String) -> Result<String, String> {
    let root = std::path::Path::new(&workspace.trim_end_matches('/')).join(prj_name.trim());
    let mut viols: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    if !root.exists() {
        return Ok(String::from("{\"ok\":true,\"violations\":[],\"scanned\":0}"));
    }
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    collect_code_files(&root, 0, &mut files);
    for f in files.iter() {
        if scanned >= 400 { break; }
        let ext = f.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !matches!(ext.as_str(), "java" | "sql" | "css" | "html" | "js") { continue; }
        let name = f.file_name().and_then(|e| e.to_str()).unwrap_or("").to_string();
        let content = match std::fs::read_to_string(f) { Ok(c) => c, Err(_) => continue };
        if content.len() > 2_000_000 { continue; }
        scanned += 1;
        let rel = f.strip_prefix(&root).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| name.clone());
        let low = content.to_lowercase();

        // 铁律1：业务代码硬编码用户身份
        if ext == "java" && name != "UserContext.java" {
            if content.contains("\"admin\"") {
                viols.push(format!("{}：硬编码用户身份 \"admin\"（铁律1，唯一例外 UserContext.defaultUser()）", rel));
            }
            if content.contains("\"当前用户\"") {
                viols.push(format!("{}：硬编码用户身份 \"当前用户\"（铁律1）", rel));
            }
        }

        // 铁律2：页面直接 new XMLHttpRequest()，绕过 api.js
        if ext == "js" && name != "api.js" && content.contains("new XMLHttpRequest") {
            viols.push(format!("{}：直接 new XMLHttpRequest()，必须走 api.js（铁律2）", rel));
        }

        // 铁律3：DAO 查询不带 DataScope
        if ext == "java" && (rel.contains("/dao/") || rel.contains("/mapper/")) && low.contains("select ") && !content.contains("DataScope") {
            viols.push(format!("{}：DAO 查询没有 DataScope.clause()（铁律3）", rel));
        }

        // 铁律4：建表缺四个尾字段
        if ext == "sql" {
            for v in sql_missing_tail_cols(&content, &rel) { viols.push(v); }
        }

        // 铁律6：CSS / 内联样式写死颜色值
        if ext == "css" || ext == "html" || ext == "js" {
            if let Some(line) = hardcoded_color_line(&content, &ext) {
                viols.push(format!("{}：第 {} 行写死颜色值（铁律6，必须用 CSS 变量）", rel, line));
            }
        }

        // 3.2 代码分层：Servlet/web 层直接拿连接，绕过 JdbcUtil
        if ext == "java" && (rel.contains("/web/") || rel.contains("/servlet/")) && content.contains("DriverManager.getConnection") {
            viols.push(format!("{}：web 层直接 DriverManager.getConnection，必须走 JdbcUtil（3.2 分层）", rel));
        }
    }

    let esc = |x: &str| x.replace('\\', "/").replace('"', "'");
    let mut out = String::from("{\"ok\":");
    out.push_str(if viols.is_empty() { "true" } else { "false" });
    out.push_str(",\"violations\":[");
    for (i, v) in viols.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('"'); out.push_str(&esc(v)); out.push('"');
    }
    out.push_str("],\"scanned\":");
    out.push_str(&scanned.to_string());
    out.push('}');
    Ok(out)
}

/// 递归收集代码文件（限深 8 层，跳过依赖/构建/日志目录）
fn collect_code_files(dir: &std::path::Path, depth: usize, out: &mut Vec<std::path::PathBuf>) {
    if depth > 8 || out.len() >= 400 { return; }
    let rd = match std::fs::read_dir(dir) { Ok(r) => r, Err(_) => return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let n = p.file_name().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            if matches!(n.as_str(), ".git" | "node_modules" | "target" | "build" | "dist" | ".idea" | "logs" | "data") { continue; }
            collect_code_files(&p, depth + 1, out);
        } else if p.is_file() {
            out.push(p);
        }
    }
}

/// 铁律4：逐条 CREATE TABLE 检查 id / created_at / created_by / org_id 四列
fn sql_missing_tail_cols(sql: &str, rel: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let up = sql.to_uppercase();
    let mut from = 0usize;
    while let Some(pos) = up[from..].find("CREATE TABLE") {
        let start = from + pos;
        let open = match sql[start..].find('(') { Some(v) => start + v, None => break };
        let mut depth = 0i32;
        let mut close = open;
        for (bi, ch) in sql[open..].char_indices() {
            if ch == '(' { depth += 1; }
            else if ch == ')' { depth -= 1; if depth == 0 { close = open + bi; break; } }
        }
        if close <= open { break; }
        let block = &sql[open + 1..close];
        let has = |col: &str| -> bool {
            for line in block.lines() {
                let t = line.trim_start_matches(|c: char| c == '(' || c == '`' || c.is_whitespace());
                let first = t.split(|c: char| c.is_whitespace() || c == ',' || c == '`')
                    .filter(|x| !x.is_empty()).next().unwrap_or("");
                if first.eq_ignore_ascii_case(col) { return true; }
            }
            false
        };
        let mut miss: Vec<&str> = Vec::new();
        for &col in ["id", "created_at", "created_by", "org_id"].iter() {
            if !has(col) { miss.push(col); }
        }
        if !miss.is_empty() {
            let tname = sql[start..open].trim().split_whitespace().last().unwrap_or("?").trim_matches('`').to_string();
            out.push(format!("{}：建表 {} 缺少必备字段 {}（铁律4）", rel, tname, miss.join(" / ")));
        }
        from = close;
    }
    out
}

/// 铁律6：找写死颜色值的行（6/8 位 hex；CSS 变量定义行放过）
fn hardcoded_color_line(content: &str, ext: &str) -> Option<usize> {
    for (idx, line) in content.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') { continue; }
        if t.starts_with("--") || t.contains("var(--") { continue; }
        if !has_hex6(line) { continue; }
        if ext == "css" { return Some(idx + 1); }
        if ext == "html" && (line.contains("style=") || line.contains("style:")) { return Some(idx + 1); }
        if ext == "js" && (line.contains(".style") || line.contains("cssText") || line.contains("setAttribute(\"style") || line.contains("setAttribute('style")) { return Some(idx + 1); }
    }
    None
}

fn has_hex6(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == '#' {
            let mut n = 0usize;
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_hexdigit() { n += 1; j += 1; }
            if n == 6 || n == 8 { return true; }
        }
        i += 1;
    }
    false
}

#[tauri::command]
fn stack_check(prj_name: String, workspace: String, forbid: String) -> Result<String, String> {
    let dir = format!("{}/{}", workspace.trim_end_matches('/'), prj_name.trim());
    let mut viols: Vec<String> = Vec::new();
    let mut checked: Vec<String> = Vec::new();

    // 1) npm / 非规定构建工具：只看文件是否存在（跨技术栈通用）
    let file_rules: [(&str, &str); 3] = [
        ("package.json", "出现 package.json —— 本项目禁止 npm 依赖与前端构建工具"),
        ("build.gradle", "出现 build.gradle —— 本项目构建工具规定为 Maven"),
        ("build.gradle.kts", "出现 build.gradle.kts —— 本项目构建工具规定为 Maven"),
    ];
    for (f, msg) in file_rules.iter() {
        let p = format!("{}/{}", dir, f);
        if std::path::Path::new(&p).exists() {
            checked.push(f.to_string());
            viols.push(msg.to_string());
        }
    }

    // 2) 关键词取自项目规范「禁止」列，只保留长度 >= 4 的纯英文词（避免 orm/jpa 这类短词误报 format/platform）
    let kw: Vec<String> = forbid
        .split(|c| c == ',' || c == '，' || c == '/' || c == '、')
        .map(|x| x.trim().to_lowercase())
        .filter(|x| x.len() >= 4 && x.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '-' || ch == '.'))
        .collect();

    // 3) 依赖清单文件：pom.xml / 根目录任意 .csproj
    let mut manifests: Vec<String> = Vec::new();
    for f in ["pom.xml", "app/pom.xml"].iter() {
        let p = format!("{}/{}", dir, f);
        if std::path::Path::new(&p).is_file() {
            manifests.push(p);
        }
    }
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let path = e.path();
            if path.is_file() && path.extension().map(|x| x == "csproj").unwrap_or(false) {
                manifests.push(path.to_string_lossy().to_string());
            }
        }
    }
    for p in manifests.iter() {
        let short = p.rsplit('/').next().unwrap_or(p.as_str()).to_string();
        checked.push(short.clone());
        if let Ok(c) = std::fs::read_to_string(p) {
            let lc = c.to_lowercase();
            for k in kw.iter() {
                if lc.contains(k.as_str()) {
                    let short_kw: String = k.chars().take(40).collect();
                    viols.push(format!("{} 里出现被禁止的依赖：{}", short, short_kw));
                }
            }
        }
    }

    let esc = |x: &str| x.replace('\\', "/").replace('"', "'");
    let mut out = String::from("{\"ok\":");
    out.push_str(if viols.is_empty() { "true" } else { "false" });
    out.push_str(",\"violations\":[");
    for (i, v) in viols.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('"');
        out.push_str(&esc(v));
        out.push('"');
    }
    out.push_str("],\"checked\":[");
    for (i, v) in checked.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push('"');
        out.push_str(&esc(v));
        out.push('"');
    }
    out.push_str("]}");
    Ok(out)
}


/// 读取 devStandard 规范文件（AGENTS.md / ARCHITECTURE.md / CODING-STANDARDS.md）
/// 优先读当前项目目录的 devStandard（新建项目时已复制+替换），无则回退模板
#[tauri::command]
fn constr_read_tpl(file: String, prj_name: String, workspace: String) -> Result<String, String> {
    let safe = match file.as_str() {
        "AGENTS.md" | "ARCHITECTURE.md" | "CODING-STANDARDS.md" => file,
        _ => return Err("非法文件名".to_string()),
    };
    // 1) 优先读项目目录
    if !prj_name.is_empty() && !workspace.is_empty() {
        let proj_path = format!("{}/{}/devStandard/{}", workspace.trim_end_matches('/'), prj_name.trim(), safe);
        if std::path::Path::new(&proj_path).exists() {
            if let Ok(c) = std::fs::read_to_string(&proj_path) {
                return Ok(c);
            }
        }
    }
    // 2) 回退模板
    let tpl_path = format!("E:/88.project/TDev/src/template/devStandard/{safe}");
    std::fs::read_to_string(&tpl_path).map_err(|e| format!("读取失败: {e}"))
}

/// 保存规范文件到当前项目的 devStandard 目录
#[tauri::command]
fn constr_save(file: String, prj_name: String, workspace: String, content: String) -> Result<String, String> {
    let safe = match file.as_str() {
        "AGENTS.md" | "ARCHITECTURE.md" | "CODING-STANDARDS.md" => file,
        _ => return Err("非法文件名".to_string()),
    };
    if prj_name.is_empty() || workspace.is_empty() {
        return Err("请先选择项目并设置工作空间".to_string());
    }
    let dir = format!("{}/{}/devStandard", workspace.trim_end_matches('/'), prj_name.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let path = format!("{}/{}", dir, safe);
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(format!("已保存到 {}", path))
}

/// A+ 方案：返回"项目约束上下文包"——铁律原文 + 三个规范文件的实际绝对路径。
/// 前端把它拼进 Agent 的 instructions（常驻 system prompt），细则由 Agent 按需自读文件。
/// 铁律唯一来源 = ARCHITECTURE.md 第 10 章：改文档自动生效，代码里不写第二份。
#[tauri::command]
fn constr_context(prj_name: String, workspace: String) -> Result<serde_json::Value, String> {
    let ws = workspace.trim_end_matches('/').to_string();
    let pn = prj_name.trim().to_string();
    let tpl_dir = "E:/88.project/TDev/src/template/devStandard";
    let has_proj = !pn.is_empty() && !ws.is_empty();

    // 三个文件的实际路径：项目目录优先，无则回退模板
    let mut paths = serde_json::Map::new();
    let mut from_project = false;
    for f in ["AGENTS.md", "ARCHITECTURE.md", "CODING-STANDARDS.md"] {
        let proj = format!("{}/{}/devStandard/{}", ws, pn, f);
        let p = if has_proj && std::path::Path::new(&proj).exists() {
            from_project = true;
            proj
        } else {
            format!("{}/{}", tpl_dir, f)
        };
        paths.insert(f.to_string(), serde_json::Value::String(p));
    }

    let arch_path = paths
        .get("ARCHITECTURE.md")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let arch = std::fs::read_to_string(&arch_path).unwrap_or_default();

    // 截取 "## 10." 到下一个 "## " 之间的内容（铁律原文）
    let ironlaws = if let Some(i) = arch.find("## 10.") {
        let rest = &arch[i..];
        let end = rest[1..].find("\n## ").map(|k| k + 1).unwrap_or(rest.len());
        rest[..end].trim().to_string()
    } else {
        String::new()
    };

    // 技术栈：解析 AGENTS.md 第 2 章「技术栈（硬约束）」的表格。
    // 开发计划的地基 6 步由这份表格驱动 —— 换 .NET + MS SQL 模板时只改 AGENTS.md，
    // 计划里就会写 SQL Server 而不是 MySQL。代码里绝不写死技术栈。
    let agents_path = paths
        .get("AGENTS.md")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let agents = std::fs::read_to_string(&agents_path).unwrap_or_default();
    let mut stack = serde_json::Map::new();
    let mut forbids = serde_json::Map::new();
    if let Some(i) = agents.find("## 2.") {
        let rest = &agents[i..];
        let end = rest[1..].find("
## ").map(|k| k + 1).unwrap_or(rest.len());
        for line in rest[..end].lines() {
            let l = line.trim();
            if !l.starts_with('|') {
                continue;
            }
            let cells: Vec<&str> = l.trim_matches('|').split('|').map(|c| c.trim()).collect();
            if cells.len() < 2 {
                continue;
            }
            let field = match cells[0] {
                "前端" => "frontend",
                "后端" => "backend",
                "服务器" => "server",
                "数据库" => "database",
                "缓存" => "cache",
                "构建" => "build",
                _ => continue,
            };
            if !cells[1].is_empty() {
                stack.insert(field.to_string(), serde_json::Value::String(cells[1].to_string()));
            }
            if cells.len() >= 3 && !cells[2].is_empty() {
                forbids.insert(field.to_string(), serde_json::Value::String(cells[2].to_string()));
            }
        }
    }

    Ok(serde_json::json!({
        "ok": !ironlaws.is_empty(),
        "ironlaws": ironlaws,
        "stack": serde_json::Value::Object(stack),
        "forbids": serde_json::Value::Object(forbids),
        "paths": paths,
        "fromProject": from_project
    }))
}

/// 保存业务流程文档到项目 doc 目录
#[tauri::command]
fn process_save(prj_name: String, workspace: String, file_name: String, content: String) -> Result<String, String> {
    if prj_name.is_empty() || workspace.is_empty() {
        return Err("请先选择项目并设置工作空间".to_string());
    }
    let dir = format!("{}/{}/doc", workspace.trim_end_matches('/'), prj_name.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let path = format!("{}/{}", dir, file_name);
    // 版本备份：文件已存在则先备份旧版到 doc/versions/
    if std::path::Path::new(&path).exists() {
        if let Ok(old_content) = std::fs::read_to_string(&path) {
            let ver_dir = format!("{}/versions", dir);
            let _ = std::fs::create_dir_all(&ver_dir);
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            let ver_path = format!("{}/{}", ver_dir, ts);
            let _ = std::fs::write(&ver_path, old_content);
            // 限制：versions 最多保留 100 个最新（按文件名时间戳排序，删最旧）
            let mut vers: Vec<String> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&ver_dir) {
                for e in entries.flatten() {
                    if let Some(n) = e.file_name().to_str() {
                        if n.parse::<u128>().is_ok() { vers.push(n.to_string()); }
                    }
                }
            }
            vers.sort();
            while vers.len() > 100 {
                let old_ver = vers.remove(0);
                let _ = std::fs::remove_file(format!("{}/{}", ver_dir, old_ver));
            }
        }
    }
    std::fs::write(&path, content).map_err(|e| format!("写入失败: {e}"))?;
    Ok(path)
}

/// 列出项目 doc 目录下的流程文档
#[tauri::command]
fn process_list(prj_name: String, workspace: String) -> Result<Vec<String>, String> {
    // 返回 "文件名|修改时间戳"（毫秒）——前端据此监测内容变化
    let dir = format!("{}/{}/doc", workspace.trim_end_matches('/'), prj_name.trim());
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                if name.ends_with(".json") {
                    let mtime = e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis())
                        .unwrap_or(0);
                    files.push(format!("{}|{}", name, mtime));
                }
            }
        }
    }
    Ok(files)
}

/// 读取项目 doc 目录下的流程文档
#[tauri::command]
fn process_load(prj_name: String, workspace: String, file_name: String) -> Result<String, String> {
    let path = format!("{}/{}/doc/{}", workspace.trim_end_matches('/'), prj_name.trim(), file_name);
    std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

/// 读取自动备份（doc/versions/<时间戳>）
#[tauri::command]
fn process_auto_load(prj_name: String, workspace: String, ts: String) -> Result<String, String> {
    let path = format!("{}/{}/doc/versions/{}", workspace.trim_end_matches('/'), prj_name.trim(), ts);
    std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))
}

/// 列出 doc/versions 下的版本备份（按时间倒序）
#[tauri::command]
fn process_versions(prj_name: String, workspace: String) -> Result<Vec<String>, String> {
    let dir = format!("{}/{}/doc/versions", workspace.trim_end_matches('/'), prj_name.trim());
    let mut vers = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if let Some(name) = e.file_name().to_str() {
                vers.push(name.to_string());
            }
        }
    }
    vers.sort();
    vers.reverse();
    Ok(vers)
}

/// 回退：把 doc/versions 里最新备份恢复到流程文件
#[tauri::command]
fn process_restore(prj_name: String, workspace: String, file_name: String) -> Result<String, String> {
    let vers = process_versions(prj_name.clone(), workspace.clone())?;
    if vers.is_empty() {
        return Err("没有可回退的版本".to_string());
    }
    let ver_dir = format!("{}/{}/doc/versions", workspace.trim_end_matches('/'), prj_name.trim());
    let latest = format!("{}/{}", ver_dir, vers[0]);
    let content = std::fs::read_to_string(&latest).map_err(|e| format!("读取版本失败: {e}"))?;
    let dir = format!("{}/{}/doc", workspace.trim_end_matches('/'), prj_name.trim());
    let path = format!("{}/{}", dir, file_name);
    std::fs::write(&path, &content).map_err(|e| format!("恢复失败: {e}"))?;
    Ok(content)
}

/// 专题备份：将当前流程内容 + 名称/说明 存入 SQLite（关联项目 prjGUID）
#[tauri::command]
fn flow_backup(app: tauri::AppHandle, prj_guid: String, name: String, desc: String, content: String) -> Result<String, String> {
    let conn = db_conn(&app)?;
    // 精确到秒的时间（纯 std 实现 YYYY-MM-DD HH:MM:SS，避免 chrono 依赖）
    let now = {
        let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        let secs = d.as_secs();
        let days = secs / 86400;
        let rem = secs % 86400;
        let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
        // 从 1970-01-01 起算年月日（儒略日转换的简化版，够用）
        let mut y = 1970i64;
        let mut dleft = days as i64;
        loop {
            let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
            let yd = if leap { 366 } else { 365 };
            if dleft >= yd { dleft -= yd; y += 1; } else { break; }
        }
        let leap = (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0);
        let mdays = [31, if leap {29} else {28}, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut mon = 1usize;
        let mut dleft2 = dleft;
        for (i, md) in mdays.iter().enumerate() {
            if dleft2 >= *md { dleft2 -= *md; mon = i + 2; } else { mon = i + 1; break; }
        }
        format!("{}-{:02}-{:02} {:02}:{:02}:{:02}", y, mon, dleft2 + 1, h, m, s)
    };
    conn.execute(
        "CREATE TABLE IF NOT EXISTS flow_backups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prj_guid TEXT NOT NULL,
            name TEXT NOT NULL,
            desc TEXT,
            time TEXT NOT NULL,
            content TEXT NOT NULL
        )",
        [],
    ).map_err(|e| format!("建表失败: {e}"))?;
    conn.execute(
        "INSERT INTO flow_backups (prj_guid, name, desc, time, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![prj_guid, name, desc, now, content],
    ).map_err(|e| format!("写入备份失败: {e}"))?;
    Ok(format!("已保存专题备份：{name} @ {now}"))
}

/// 列出专题备份（按时间倒序）
#[tauri::command]
fn flow_backup_list(app: tauri::AppHandle, prj_guid: String) -> Result<Vec<String>, String> {
    let conn = db_conn(&app)?;
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS flow_backups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            prj_guid TEXT NOT NULL,
            name TEXT NOT NULL,
            desc TEXT,
            time TEXT NOT NULL,
            content TEXT NOT NULL
        )", [],
    );
    let mut items = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, name, desc, time FROM flow_backups WHERE prj_guid = ?1 ORDER BY id DESC"
    ).map_err(|e| format!("查询失败: {e}"))?;
    let rows = stmt.query_map(rusqlite::params![prj_guid], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
    }).map_err(|e| format!("读取失败: {e}"))?;
    for row in rows.flatten() {
        items.push(format!("{}|{}|{}|{}", row.0, row.1, row.2, row.3));
    }
    Ok(items)
}

/// 读取专题备份内容（按 id）
#[tauri::command]
fn flow_backup_load(app: tauri::AppHandle, id: i64) -> Result<String, String> {
    let conn = db_conn(&app)?;
    let content: String = conn.query_row(
        "SELECT content FROM flow_backups WHERE id = ?1",
        rusqlite::params![id],
        |r| r.get(0),
    ).map_err(|e| format!("读取备份失败: {e}"))?;
    Ok(content)
}

/// 更新模块状态（统一亮灯数据源）
/// module: basic / constraints / theme / process / ai_code / deploy
#[tauri::command]
fn module_state_set(app: tauri::AppHandle, prj_guid: String, module: String, filled: bool) -> Result<String, String> {
    let conn = db_conn(&app)?;
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS module_state (
            prj_guid TEXT NOT NULL,
            module TEXT NOT NULL,
            filled INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (prj_guid, module)
        )", [],
    );
    conn.execute(
        "INSERT OR REPLACE INTO module_state (prj_guid, module, filled, updated_at) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![prj_guid, module, if filled { 1 } else { 0 }, now_str()],
    ).map_err(|e| format!("更新状态失败: {e}"))?;
    Ok(format!("{}={}", module, if filled { "亮" } else { "灭" }))
}

/// 读取项目全部模块状态（统一数据源，前端一次读取整体判断）
/// 返回 JSON: {"basic":true,"constraints":true,"theme":false,...}
/// 若该项目在表中无记录（旧项目/首次读取），实时探测真实数据（doc/theme 目录）并补写表
#[tauri::command]
fn module_state_get(app: tauri::AppHandle, prj_guid: String, prj_name: String, workspace: String) -> Result<String, String> {
    let conn = db_conn(&app)?;
    let _ = conn.execute(
        "CREATE TABLE IF NOT EXISTS module_state (
            prj_guid TEXT NOT NULL,
            module TEXT NOT NULL,
            filled INTEGER NOT NULL DEFAULT 0,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (prj_guid, module)
        )", [],
    );
    // 查表
    let mut stmt = conn.prepare(
        "SELECT module, filled FROM module_state WHERE prj_guid = ?1"
    ).map_err(|e| format!("查询失败: {e}"))?;
    let rows = stmt.query_map(rusqlite::params![prj_guid], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    }).map_err(|e| format!("读取失败: {e}"))?;
    let mut has_row = false;
    let mut map = std::collections::HashMap::new();
    for row in rows.flatten() {
        has_row = true;
        map.insert(row.0, row.1 > 0);
    }
    if !has_row {
        // 旧项目首次读取：实时探测真实数据（默认 basic/constraints=true）
        map.insert("basic".to_string(), true);
        map.insert("constraints".to_string(), true);
        // 探测主题：workspace/项目名/theme/tokens.json 存在？
        let theme_path = format!("{}/{}/theme/tokens.json", workspace.trim_end_matches('/'), prj_name.trim());
        let theme_ok = std::path::Path::new(&theme_path).exists();
        map.insert("theme".to_string(), theme_ok);
        // 探测流程：workspace/项目名/doc/ 下有 *_flow.json 或 flow.json？
        let doc_dir = format!("{}/{}/doc", workspace.trim_end_matches('/'), prj_name.trim());
        let mut proc_ok = false;
        if let Ok(entries) = std::fs::read_dir(&doc_dir) {
            for e in entries.flatten() {
                let n = e.file_name().to_string_lossy().to_string();
                if n.ends_with("_flow.json") || n.ends_with("flow.json") {
                    proc_ok = true;
                    break;
                }
            }
        }
        map.insert("process".to_string(), proc_ok);
        map.insert("ai_code".to_string(), false);
        map.insert("deploy".to_string(), false);
        // 补写表（让后续读取稳定）
        let now0 = now_str();
        for (m, f) in [("basic", true), ("constraints", true), ("theme", theme_ok), ("process", proc_ok), ("ai_code", false), ("deploy", false)] {
            let _ = conn.execute(
                "INSERT OR REPLACE INTO module_state (prj_guid, module, filled, updated_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![prj_guid, m, if f { 1 } else { 0 }, now0],
            );
        }
    }
    serde_json::to_string(&map).map_err(|e| format!("序列化失败: {e}"))
}


/// 以主色为基准生成 10 阶色卡（Material 风格：浅阶向白混合，深阶向黑混合）
fn generate_palette(accent: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let hex = accent.trim_start_matches('#');
    if hex.len() != 6 {
        map.insert("50".into(), "#E3F2FC".into());
        map.insert("100".into(), "#BBDEFB".into());
        map.insert("200".into(), "#90CAF9".into());
        map.insert("300".into(), "#64B5F6".into());
        map.insert("400".into(), "#42A5F5".into());
        map.insert("500".into(), "#2196F3".into());
        map.insert("600".into(), "#1E88E5".into());
        map.insert("700".into(), accent.to_string());
        map.insert("800".into(), "#00529B".into());
        map.insert("900".into(), "#003F77".into());
        return map;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let ratios: [(f32, f32); 10] = [
        (0.88, 0.0),
        (0.76, 0.0),
        (0.62, 0.0),
        (0.46, 0.0),
        (0.30, 0.0),
        (0.16, 0.0),
        (0.07, 0.0),
        (0.0, 0.0),
        (0.0, 0.12),
        (0.0, 0.25),
    ];
    let names = ["50", "100", "200", "300", "400", "500", "600", "700", "800", "900"];
    for (i, name) in names.iter().enumerate() {
        let (wr, br) = ratios[i];
        let nr = (r as f32 * (1.0 - wr) + 255.0 * wr) * (1.0 - br);
        let ng = (g as f32 * (1.0 - wr) + 255.0 * wr) * (1.0 - br);
        let nb = (b as f32 * (1.0 - wr) + 255.0 * wr) * (1.0 - br);
        map.insert(name.to_string(), format!("#{:02X}{:02X}{:02X}", nr as u8, ng as u8, nb as u8));
    }
    map
}

/// 模板 ui-preview.html 中对应阶的固定色值（用于替换 <em> 展示）
fn template_palette_value(step: &str) -> &'static str {
    match step {
        "50" => "E3F2FC",
        "100" => "BBDEFB",
        "200" => "90CAF9",
        "300" => "64B5F6",
        "400" => "42A5F5",
        "500" => "2196F3",
        "600" => "1E88E5",
        "700" => "0067C0",
        "800" => "00529B",
        "900" => "003F77",
        _ => "0067C0",
    }
}

/// 从生成的 tokens.json 提取完整 palette（优先 Agent 生成的，缺则 None）
fn extract_palette(json_text: &str) -> Option<std::collections::HashMap<String, String>> {
    // 剥离非 JSON 前缀
    let start = json_text.find('{')?;
    let v: serde_json::Value = serde_json::from_str(&json_text[start..]).ok()?;
    // palette 可能在顶层或 color.palette 嵌套
    let p = v.get("palette").or_else(|| v.get("color").and_then(|c| c.get("palette")))?;
    let mut map = std::collections::HashMap::new();
    for step in ["50", "100", "200", "300", "400", "500", "600", "700", "800", "900"] {
        if let Some(c) = p.get(format!("accent-{step}")).and_then(|x| x.as_str()) {
            map.insert(step.to_string(), c.to_string());
        }
    }
    if map.len() >= 10 { Some(map) } else { None }
}

/// 从生成的 tokens.json 提取完整 color 映射（"light.background" -> "#..."）
fn extract_color_map(json_text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_text) {
        if let Some(color) = v.get("color") {
            for theme in ["light", "dark"] {
                if let Some(t) = color.get(theme) {
                    if let Some(obj) = t.as_object() {
                        for (k, val) in obj {
                            if let Some(s) = val.as_str() {
                                map.insert(format!("{theme}.{k}"), s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

/// 从 tokens.json 文本中提取 accent 主色
fn extract_accent(json_text: &str) -> Option<String> {
    // 先剥离非 JSON 前缀（Agent 回复可能带说明文字），找第一个 {
    let start = json_text.find('{')?;
    let v: serde_json::Value = serde_json::from_str(&json_text[start..]).ok()?;
    // Win11 风格: color.light.accent
    if let Some(a) = v["color"]["light"]["accent"].as_str() { return Some(a.to_string()); }
    // 北极冰山风格: colors.primary.default
    if let Some(a) = v["colors"]["primary"]["default"].as_str() { return Some(a.to_string()); }
    // 通用: theme.colors.accent.default
    if let Some(a) = v["theme"]["colors"]["accent"]["default"].as_str() { return Some(a.to_string()); }
    None
}

/// 示例命令 ①：递归新建文件夹，返回结果文本
#[tauri::command]
fn create_folder(path: String) -> Result<String, String> {
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(format!("已创建文件夹: {path}"))
}

/// 示例命令 ②：通过 cmd /C 执行脚本，返回 stdout
/// 注意：任意命令执行属于高风险能力，生产环境必须经白名单化改造后再开放
#[tauri::command]
fn run_script(script: String) -> Result<String, String> {
    let output = std::process::Command::new("cmd")
        .arg("/C")
        .arg(&script)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
