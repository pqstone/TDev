// ============================================================
// TDev 后端入口：插件注册 + 自定义 Tauri Command
// 规范：新增 IPC 命令时，在此使用 #[tauri::command] 定义，
//       并加入下方 generate_handler! 列表；相应权限在
//       capabilities/default.json 中最小化授予。
// ============================================================

use rusqlite::{params, Connection};
use tauri::Manager;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

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
            project_list,
            project_create,
            project_delete,
            cfg_save,
            cfg_load,
            agent_ask,
            project_save_theme,
            theme_preview_html,
            theme_read_project,
            constr_read_tpl,
            constr_save,
            process_save,
            process_list,
            process_load,
            process_auto_load,
            process_versions,
            process_restore,
            flow_backup,
            flow_backup_list,
            flow_backup_load
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
    // async 命令：直接 async reqwest（配合 Tauri async runtime，UI 不冻结）
    let url = "http://127.0.0.1:8642/v1/chat/completions";
    let body = serde_json::json!({
        "model": "hermes-agent",
        "messages": [{"role": "user", "content": question}]
    });
    let client = reqwest::Client::new();
    let resp = client.post(url)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer tdev-hulk-2026-api-server-key")
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
