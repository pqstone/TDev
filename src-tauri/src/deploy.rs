// ============================================================
// 部署发布 · 引擎（第二阶段）
// 设计：docs/deploy-module.md
// ------------------------------------------------------------
// 通用能力：当前项目 → 选择服务器 → 构建 → 上传 → 启动 → 启动验证
// 通道：SSH/SFTP（Linux 与 Windows 都用同一条通道；Windows 需在服务器上启用 OpenSSH Server）
// 密码：从 deploy_servers.password_enc 用 DPAPI 解出，只在内存里用，不落日志
// ============================================================

use crate::{db_conn, deploy_ensure_table, deploy_tcp_probe, dpapi_unprotect, now_str};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// 全局部署锁：同一时刻只允许一个部署任务（和 Agent 锁同样的思路）
static DEPLOY_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn ensure_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS deploy_configs (
            prj_guid       TEXT PRIMARY KEY,
            server_id      INTEGER NOT NULL,
            build_cmd      TEXT,
            artifact_path  TEXT,
            remote_dir     TEXT,
            start_cmd      TEXT,
            verify_url     TEXT,
            verify_port    INTEGER DEFAULT 0,
            verify_retries INTEGER DEFAULT 10,
            updated_at     TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS deploy_releases (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            prj_guid    TEXT NOT NULL,
            server_id   INTEGER NOT NULL,
            server_name TEXT,
            status      TEXT NOT NULL,
            step        TEXT,
            artifact    TEXT,
            message     TEXT,
            log         TEXT,
            started_at  TEXT NOT NULL,
            finished_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_deploy_releases_prj ON deploy_releases(prj_guid);",
    )
    .map_err(|e| format!("创建部署表失败: {e}"))
}

/// 按项目类型给一份默认部署配置（用户可改，保存后以库里为准）
fn default_config(prj_type: &str, prj_name: &str) -> Value {
    let t = prj_type.to_lowercase();
    let name = if prj_name.trim().is_empty() { "app" } else { prj_name.trim() };
    if t == "web" {
        json!({
            "build_cmd": "mvn -q -DskipTests clean package",
            "artifact_path": "target/*.war",
            "remote_dir": "/opt/tomcat/webapps",
            "start_cmd": "sudo systemctl restart tomcat",
            "verify_url": "",
            "verify_port": 8080,
            "verify_retries": 15
        })
    } else if t == "tauri" {
        json!({
            "build_cmd": "cargo build --release",
            "artifact_path": "src-tauri/target/release/*.exe",
            "remote_dir": format!("C:/Apps/{}", name),
            "start_cmd": format!("powershell -NoProfile -Command \"Get-Process -Name '{}' -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 1; Start-Process 'C:/Apps/{}/{}.exe'\"", name, name, name),
            "verify_url": "",
            "verify_port": 0,
            "verify_retries": 10
        })
    } else {
        json!({
            "build_cmd": "cargo build --release",
            "artifact_path": "target/release/*.exe",
            "remote_dir": format!("C:/Apps/{}", name),
            "start_cmd": format!("powershell -NoProfile -Command \"Get-Process -Name '{}' -ErrorAction SilentlyContinue | Stop-Process -Force; Start-Sleep -Seconds 1; Start-Process 'C:/Apps/{}/{}.exe'\"", name, name, name),
            "verify_url": "",
            "verify_port": 0,
            "verify_retries": 10
        })
    }
}

fn config_row(conn: &Connection, prj_guid: &str) -> Result<Option<Value>, String> {
    ensure_tables(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT server_id, build_cmd, artifact_path, remote_dir, start_cmd,
                    verify_url, verify_port, verify_retries, updated_at
             FROM deploy_configs WHERE prj_guid = ?1",
        )
        .map_err(|e| format!("查询部署配置失败: {e}"))?;
    let mut rows = stmt
        .query_map(params![prj_guid], |r| {
            Ok(json!({
                "server_id": r.get::<_, i64>(0)?,
                "build_cmd": r.get::<_, Option<String>>(1)?,
                "artifact_path": r.get::<_, Option<String>>(2)?,
                "remote_dir": r.get::<_, Option<String>>(3)?,
                "start_cmd": r.get::<_, Option<String>>(4)?,
                "verify_url": r.get::<_, Option<String>>(5)?,
                "verify_port": r.get::<_, Option<i64>>(6)?,
                "verify_retries": r.get::<_, Option<i64>>(7)?,
                "updated_at": r.get::<_, String>(8)?
            }))
        })
        .map_err(|e| format!("读取部署配置失败: {e}"))?;
    Ok(rows.next().transpose().map_err(|e| e.to_string())?)
}

/// 读配置：库里没有就返回按项目类型的默认值（不写库）
#[tauri::command]
pub fn deploy_config_get(
    app: AppHandle,
    prj_guid: String,
    prj_type: String,
    prj_name: String,
) -> Result<Value, String> {
    let conn = db_conn(&app)?;
    if let Some(v) = config_row(&conn, &prj_guid)? {
        return Ok(v);
    }
    let mut v = default_config(&prj_type, &prj_name);
    v["server_id"] = json!(0);
    Ok(v)
}

/// 保存部署配置（选择服务器 + 构建/上传/启动/验证参数）
#[tauri::command]
pub fn deploy_config_save(
    app: AppHandle,
    prj_guid: String,
    server_id: i64,
    build_cmd: String,
    artifact_path: String,
    remote_dir: String,
    start_cmd: String,
    verify_url: String,
    verify_port: i64,
    verify_retries: i64,
) -> Result<bool, String> {
    let conn = db_conn(&app)?;
    config_save_db(&conn, &prj_guid, server_id, &build_cmd, &artifact_path, &remote_dir,
                   &start_cmd, &verify_url, verify_port, verify_retries)?;
    Ok(true)
}

/// 保存配置（数据层，便于单测）
fn config_save_db(
    conn: &Connection,
    prj_guid: &str,
    server_id: i64,
    build_cmd: &str,
    artifact_path: &str,
    remote_dir: &str,
    start_cmd: &str,
    verify_url: &str,
    verify_port: i64,
    verify_retries: i64,
) -> Result<(), String> {
    if prj_guid.trim().is_empty() {
        return Err("未选择项目".to_string());
    }
    if server_id <= 0 {
        return Err("请先选择一台服务器".to_string());
    }
    if remote_dir.trim().is_empty() {
        return Err("请填写服务器上的目标目录".to_string());
    }
    if artifact_path.trim().is_empty() {
        return Err("请填写要上传的产物路径".to_string());
    }
    ensure_tables(conn)?;
    conn.execute(
        "INSERT INTO deploy_configs
            (prj_guid, server_id, build_cmd, artifact_path, remote_dir, start_cmd,
             verify_url, verify_port, verify_retries, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(prj_guid) DO UPDATE SET
            server_id=excluded.server_id, build_cmd=excluded.build_cmd,
            artifact_path=excluded.artifact_path, remote_dir=excluded.remote_dir,
            start_cmd=excluded.start_cmd, verify_url=excluded.verify_url,
            verify_port=excluded.verify_port, verify_retries=excluded.verify_retries,
            updated_at=excluded.updated_at",
        params![
            prj_guid,
            server_id,
            build_cmd.trim(),
            artifact_path.trim(),
            remote_dir.trim(),
            start_cmd.trim(),
            verify_url.trim(),
            verify_port,
            verify_retries.clamp(1, 120),
            now_str()
        ],
    )
    .map_err(|e| format!("保存部署配置失败: {e}"))?;
    Ok(())
}

/// 发布记录（不含日志正文，列表用）
#[tauri::command]
pub fn deploy_release_list(app: AppHandle, prj_guid: String) -> Result<Vec<Value>, String> {
    let conn = db_conn(&app)?;
    ensure_tables(&conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, server_name, status, step, artifact, message, started_at, finished_at
             FROM deploy_releases WHERE prj_guid = ?1 ORDER BY id DESC LIMIT 30",
        )
        .map_err(|e| format!("查询发布记录失败: {e}"))?;
    let rows = stmt
        .query_map(params![prj_guid], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "server_name": r.get::<_, Option<String>>(1)?,
                "status": r.get::<_, String>(2)?,
                "step": r.get::<_, Option<String>>(3)?,
                "artifact": r.get::<_, Option<String>>(4)?,
                "message": r.get::<_, Option<String>>(5)?,
                "started_at": r.get::<_, String>(6)?,
                "finished_at": r.get::<_, Option<String>>(7)?
            }))
        })
        .map_err(|e| format!("读取发布记录失败: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// 某次发布的完整日志
#[tauri::command]
pub fn deploy_release_log(app: AppHandle, id: i64) -> Result<String, String> {
    let conn = db_conn(&app)?;
    ensure_tables(&conn)?;
    let mut stmt = conn
        .prepare("SELECT log FROM deploy_releases WHERE id = ?1")
        .map_err(|e| format!("查询失败: {e}"))?;
    let mut rows = stmt.query_map(params![id], |r| r.get::<_, Option<String>>(0))
        .map_err(|e| format!("读取失败: {e}"))?;
    Ok(rows.next().transpose().map_err(|e| e.to_string())?.flatten().unwrap_or_default())
}

fn emit(app: Option<&AppHandle>, run_id: i64, step: &str, status: &str, line: &str) {
    deploy_log_push(format!("[{}] {}", step, line));
    if app.is_none() {
        println!("[{:6}] {} {}", status, step, line);
    }
    if let Some(a) = app {
        let _ = a.emit(
            "deploy_event",
            json!({ "runId": run_id, "step": step, "status": status, "line": line }),
        );
    }
}

fn workspace_path(conn: &Connection) -> String {
    conn.query_row("SELECT value FROM app_meta WHERE key='workspace'", [], |r| r.get::<_, String>(0))
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(|s| s.to_string()))
        .unwrap_or_default()
}

/// 解析产物：支持 `*` 通配，命中多个时取最新修改的那个
fn resolve_artifact(project_dir: &std::path::Path, pattern: &str) -> Result<std::path::PathBuf, String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Err("未配置产物路径".to_string());
    }
    let pat_path = std::path::Path::new(pattern);
    let full = if pat_path.is_absolute() { pat_path.to_path_buf() } else { project_dir.join(pattern) };
    if !pattern.contains('*') {
        if full.is_file() {
            return Ok(full);
        }
        return Err(format!("找不到产物：{}", full.display()));
    }
    let parent = full.parent().ok_or_else(|| format!("产物路径不合法：{}", full.display()))?;
    let file_pat = full.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let (prefix, suffix) = match file_pat.split_once('*') {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (file_pat.to_string(), String::new()),
    };
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    let entries = std::fs::read_dir(parent)
        .map_err(|e| format!("读取产物目录失败 {}：{e}", parent.display()))?;
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with(&prefix) && name.ends_with(&suffix) {
            let mt = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if best.as_ref().map(|(t, _)| mt > *t).unwrap_or(true) {
                best = Some((mt, p));
            }
        }
    }
    best.map(|(_, p)| p).ok_or_else(|| format!("没有匹配到产物：{}", full.display()))
}

/// 本地构建：流式回传日志；超时/失败返回 Err
/// Windows 下若项目同级存在 `Public\runtime\env.bat`（TestSpace 约定：JDK17+Maven+Tomcat），
/// 自动先加载它再执行构建命令 —— 否则 mvn/java 不在 PATH 里，用户只会看到「退出码 1」。
/// 输出按 GBK 解码（Windows 控制台默认编码），否则中文报错会被当成非法 UTF-8 丢掉。
fn run_build(
    app: Option<&AppHandle>,
    run_id: i64,
    project_dir: &std::path::Path,
    cmd: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    use std::io::BufRead;
    use std::process::{Command, Stdio};

    #[cfg(windows)]
    let (mut child, env_note) = {
        let ws = project_dir.parent().unwrap_or(project_dir);
        let envbat = ws.join("Public").join("runtime").join("env.bat");
        // 用临时 .bat 承载「加载环境 + 构建命令」，彻底绕开 cmd /C 的嵌套引号问题
        let bat = std::env::temp_dir().join(format!("tdev_build_{}.bat", run_id));
        let mut body = String::from("@echo off\r\n");
        let note = if envbat.is_file() {
            body.push_str(&format!("call \"{}\" >nul 2>&1\r\n", envbat.display()));
            format!("已加载构建环境：{}", envbat.display())
        } else {
            "未找到 Public\\runtime\\env.bat，使用系统 PATH".to_string()
        };
        body.push_str(&format!("cd /d \"{}\"\r\n", project_dir.display()));
        body.push_str(cmd);
        body.push_str("\r\nexit /b %ERRORLEVEL%\r\n");
        std::fs::write(&bat, body).map_err(|e| format!("写构建脚本失败：{e}"))?;

        let mut c = Command::new("cmd");
        c.arg("/C").arg(&bat);
        c.current_dir(project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW：不留黑窗
        (c.spawn().map_err(|e| format!("启动构建命令失败：{e}"))?, note)
    };

    #[cfg(not(windows))]
    let (mut child, env_note) = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(cmd);
        c.current_dir(project_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        (c.spawn().map_err(|e| format!("启动构建命令失败：{e}"))?, "使用系统 PATH".to_string())
    };

    emit(app, run_id, "build", "log", &env_note);

    fn pump<R: std::io::Read + Send + 'static>(
        reader: R,
        app: Option<AppHandle>,
        run_id: i64,
        counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let mut r = std::io::BufReader::new(reader);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let (s, _, _) = encoding_rs::GBK.decode(&buf);
                        let line = s.trim_end_matches(['\r', '\n']).to_string();
                        if !line.trim().is_empty() {
                            counter.fetch_add(1, Ordering::Relaxed);
                        }
                        emit(app.as_ref(), run_id, "build", "log", &line);
                    }
                }
            }
        })
    }

    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let t1 = match child.stdout.take() {
        Some(o) => pump(o, app.map(|a| a.clone()), run_id, counter.clone()),
        None => std::thread::spawn(|| {}),
    };
    let t2 = match child.stderr.take() {
        Some(e) => pump(e, app.map(|a| a.clone()), run_id, counter.clone()),
        None => std::thread::spawn(|| {}),
    };

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if start.elapsed().as_secs() > timeout_secs {
                    let _ = child.kill();
                    let _ = t1.join();
                    let _ = t2.join();
                    return Err(format!("构建超时（{} 秒）", timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            Err(e) => {
                let _ = t1.join();
                let _ = t2.join();
                return Err(format!("构建进程异常：{e}"));
            }
        }
    };
    let _ = t1.join();
    let _ = t2.join();

    if status.success() {
        return Ok(());
    }
    let code = status.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
    if counter.load(Ordering::Relaxed) == 0 {
        Err(format!(
            "构建失败（退出码 {}），且命令没有任何输出：多半是命令不存在（PATH 里没有 mvn/java）。\
             可在构建命令里先加载环境，例如：call Public\\runtime\\env.bat && mvn -q -DskipTests clean package",
            code
        ))
    } else {
        Err(format!("构建失败（退出码 {}），详见上方日志", code))
    }
}

// ---------- SSH / SFTP ----------

struct SshHandler;

impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // 首次连接即信任（内网部署工具；如需严格校验可在此比对 known_hosts）
        Ok(true)
    }
}

async fn ssh_connect(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
) -> Result<russh::client::Handle<SshHandler>, String> {
    let cfg = std::sync::Arc::new(russh::client::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(120)),
        ..Default::default()
    });
    let mut session = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        russh::client::connect(cfg, (host, port), SshHandler),
    )
    .await
    .map_err(|_| format!("连接 {}:{} 超时（15 秒）", host, port))?
    .map_err(|e| format!("SSH 连接失败：{e}"))?;

    let auth = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        session.authenticate_password(user.to_string(), password.to_string()),
    )
    .await
    .map_err(|_| "SSH 登录超时（20 秒）".to_string())?
    .map_err(|e| format!("SSH 登录失败：{e}"))?;

    match auth {
        russh::client::AuthResult::Success => Ok(session),
        _ => Err("SSH 登录失败：用户名或密码不正确".to_string()),
    }
}

async fn ssh_exec(
    app: Option<&AppHandle>,
    run_id: i64,
    step: &str,
    session: &russh::client::Handle<SshHandler>,
    cmd: &str,
    timeout_secs: u64,
) -> Result<(i32, String), String> {
    let mut ch = session
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SSH 通道失败：{e}"))?;
    ch.exec(true, cmd)
        .await
        .map_err(|e| format!("执行远端命令失败：{e}"))?;
    let mut all = String::new();
    let mut code = -1i32;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(format!("远端命令超时（{} 秒）：{}", timeout_secs, cmd));
        }
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), ch.wait()).await;
        let msg = match msg {
            Err(_) => continue, // 等待中，检查总超时
            Ok(None) => break,
            Ok(Some(m)) => m,
        };
        match msg {
            russh::ChannelMsg::Data { data } => {
                let s = String::from_utf8_lossy(&data).to_string();
                for l in s.lines() {
                    emit(app, run_id, step, "log", l);
                }
                all.push_str(&s);
            }
            russh::ChannelMsg::ExtendedData { data, .. } => {
                let s = String::from_utf8_lossy(&data).to_string();
                for l in s.lines() {
                    emit(app, run_id, step, "log", l);
                }
                all.push_str(&s);
            }
            russh::ChannelMsg::ExitStatus { exit_status } => code = exit_status as i32,
            // ⚠️ Eof 之后还会来 ExitStatus，不能在 Eof 就 break，否则退出码永远是 -1
            russh::ChannelMsg::Close => break,
            _ => {}
        }
    }
    if code < 0 {
        // 少数服务器不发 ExitStatus：没有报错就当成功
        emit(app, run_id, step, "log", "服务器未返回退出码，按成功处理");
        code = 0;
    }
    Ok((code, all))
}

/// 逐级创建远端目录：SFTP 的 create_dir 不递归，父目录不存在会直接报 NoSuchFile
async fn sftp_ensure_dir(
    app: Option<&AppHandle>,
    run_id: i64,
    sftp: &russh_sftp::client::SftpSession,
    dir: &str,
) -> Result<(), String> {
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() {
        return Ok(());
    }
    let absolute = dir.starts_with('/');
    let mut cur = if absolute { "/".to_string() } else { String::new() };
    for part in dir.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." {
            continue;
        }
        if cur.is_empty() {
            cur.push_str(part);
        } else if cur.ends_with('/') {
            cur.push_str(part);
        } else {
            cur.push('/');
            cur.push_str(part);
        }
        match sftp.try_exists(cur.clone()).await {
            Ok(true) => continue,
            Ok(false) => {
                emit(app, run_id, "upload", "log", &format!("创建远端目录 {}", cur));
                sftp.create_dir(cur.clone())
                    .await
                    .map_err(|e| {
                        let raw = format!("{:?}", e);
                        let hint = if raw.contains("PermissionDenied") {
                            "没有写权限：换一个该账号可写的目录（如 /home/<用户>/apps/...），或用有权限的账号"
                        } else if raw.contains("NoSuchFile") {
                            "父目录不存在且无法创建"
                        } else {
                            "创建失败"
                        };
                        format!("创建目录 {} 失败：{}（{}）", cur, hint, raw)
                    })?;
            }
            Err(e) => return Err(format!("检查目录 {} 失败：{:?}", cur, e)),
        }
    }
    Ok(())
}

async fn sftp_upload(
    app: Option<&AppHandle>,
    run_id: i64,
    session: &russh::client::Handle<SshHandler>,
    local: &std::path::Path,
    remote_dir: &str,
    remote_name: &str,
) -> Result<String, String> {
    let channel = session
        .channel_open_session()
        .await
        .map_err(|e| format!("打开 SFTP 通道失败：{e}"))?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|e| format!("请求 SFTP 子系统失败：{e}"))?;
    let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
        .await
        .map_err(|e| format!("SFTP 初始化失败：{e}"))?;

    let remote_dir = remote_dir.replace('\\', "/");
    let dir = remote_dir.trim_end_matches('/').to_string();
    sftp_ensure_dir(app, run_id, &sftp, &dir).await?;
    let data = std::fs::read(local).map_err(|e| format!("读取本地产物失败：{e}"))?;
    let size = data.len();
    let remote_path = format!("{}/{}", dir, remote_name);
    emit(
        app,
        run_id,
        "upload",
        "log",
        &format!("上传 {}（{} KB）→ {}", remote_name, (size + 1023) / 1024, remote_path),
    );
    use tokio::io::AsyncWriteExt;
    let mut f = sftp
        .create(remote_path.clone())
        .await
        .map_err(|e| format!("创建远端文件失败：{e}"))?;
    f.write_all(&data)
        .await
        .map_err(|e| format!("写入远端文件失败：{e}"))?;
    f.close().await.map_err(|e| format!("关闭远端文件失败：{e}"))?;
    Ok(remote_path)
}

/// 启动验证：优先 HTTP，其次 TCP 端口
async fn verify_started(
    app: Option<&AppHandle>,
    run_id: i64,
    verify_url: &str,
    host: &str,
    port: i64,
    retries: i64,
) -> Result<String, String> {
    let retries = retries.clamp(1, 120);
    if !verify_url.trim().is_empty() {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(6))
            .build()
            .map_err(|e| format!("HTTP 客户端创建失败：{e}"))?;
        for i in 1..=retries {
            emit(app, run_id, "verify", "log", &format!("第 {}/{} 次访问 {}", i, retries, verify_url));
            match client.get(verify_url).send().await {
                Ok(resp) => {
                    let code = resp.status().as_u16();
                    if (200..400).contains(&code) {
                        return Ok(format!("HTTP {} 验证通过：{}", code, verify_url));
                    }
                    emit(app, run_id, "verify", "log", &format!("HTTP {}，程序可能还没起来", code));
                }
                Err(e) => emit(app, run_id, "verify", "log", &format!("访问失败：{e}")),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        return Err(format!("启动验证失败：{} 始终不可用", verify_url));
    }
    if port > 0 {
        for i in 1..=retries {
            let (ok, ms, _msg) = deploy_tcp_probe(host, port);
            emit(
                app,
                run_id,
                "verify",
                "log",
                &format!("第 {}/{} 次探测 {}:{} → {}", i, retries, host, port, if ok { format!("通（{} ms）", ms) } else { "不通".to_string() }),
            );
            if ok {
                return Ok(format!("端口 {} 验证通过", port));
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        return Err(format!("启动验证失败：{}:{} 始终不可达", host, port));
    }
    emit(app, run_id, "verify", "log", "未配置验证地址/端口，跳过启动验证");
    Ok("未配置启动验证（建议填写访问地址或端口）".to_string())
}

/// 执行一次部署：当前项目 → 选定服务器
#[tauri::command]
pub async fn deploy_run(app: AppHandle, prj_guid: String, server_id: i64) -> Result<Value, String> {
    if DEPLOY_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("已有部署正在进行，等它结束再点".to_string());
    }
    let prepared = (|| -> Result<(Value, Value, std::path::PathBuf, String, i64), String> {
        let conn = db_conn(&app)?;
        ensure_tables(&conn)?;
        deploy_ensure_table(&conn)?;
        let cfg = config_row(&conn, &prj_guid)?
            .ok_or_else(|| "还没有保存部署配置，请先选择服务器并填写构建/上传参数".to_string())?;
        let prj_name: String = conn
            .query_row("SELECT prjName FROM projects WHERE prjGUID=?1", params![prj_guid], |r| r.get(0))
            .map_err(|_| "项目不存在".to_string())?;
        let server = conn
            .query_row(
                "SELECT id, name, os_type, host, port, username, COALESCE(password_enc,'')
                 FROM deploy_servers WHERE id=?1 AND prj_guid=?2",
                params![server_id, prj_guid],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?,
                        "os_type": r.get::<_, String>(2)?, "host": r.get::<_, String>(3)?,
                        "port": r.get::<_, i64>(4)?, "username": r.get::<_, String>(5)?,
                        "password_enc": r.get::<_, String>(6)?
                    }))
                },
            )
            .map_err(|_| "找不到该服务器（或不属于当前项目）".to_string())?;
        let ws = workspace_path(&conn);
        if ws.trim().is_empty() {
            return Err("还没设置工作路径，请先到「项目空间」设置".to_string());
        }
        let project_dir = std::path::Path::new(&ws).join(&prj_name);
        if !project_dir.is_dir() {
            return Err(format!("项目目录不存在：{}", project_dir.display()));
        }
        let now = now_str();
        conn.execute(
            "INSERT INTO deploy_releases (prj_guid, server_id, server_name, status, step, started_at)
             VALUES (?1, ?2, ?3, 'running', 'build', ?4)",
            params![prj_guid, server_id, server["name"].as_str().unwrap_or(""), now],
        )
        .map_err(|e| format!("创建发布记录失败：{e}"))?;
        let run_id = conn.last_insert_rowid();
        Ok((cfg, server, project_dir, prj_guid.clone(), run_id))
    })();

    let (cfg, server, project_dir, prj_guid, run_id) = match prepared {
        Ok(v) => v,
        Err(e) => {
            DEPLOY_RUNNING.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = run_pipeline(Some(&app2), run_id, &prj_guid, &project_dir, &server, &cfg).await;

        // 收尾：写发布记录
        let conn = match db_conn(&app2) {
            Ok(c) => c,
            Err(_) => return,
        };
        match result {
            Ok(msg) => {
                let _ = conn.execute(
                    "UPDATE deploy_releases SET status='success', step='done', message=?1, finished_at=?2, log=?3 WHERE id=?4",
                    params![msg, now_str(), deploy_log_take(), run_id],
                );
                emit(Some(&app2), run_id, "done", "success", &msg);
            }
            Err(e) => {
                let _ = conn.execute(
                    "UPDATE deploy_releases SET status='failed', message=?1, finished_at=?2, log=?3 WHERE id=?4",
                    params![e, now_str(), deploy_log_take(), run_id],
                );
                emit(Some(&app2), run_id, "done", "fail", &e);
            }
        }
        DEPLOY_RUNNING.store(false, Ordering::SeqCst);
    });

    Ok(json!({ "runId": run_id }))
}

// 当前这次部署的日志累积（单任务，所以用一个静态缓冲区即可）
use std::sync::Mutex;
static DEPLOY_LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn deploy_log_push(line: String) {
    if let Ok(mut v) = DEPLOY_LOG.lock() {
        v.push(line);
        if v.len() > 4000 {
            let drop_n = v.len() - 4000;
            v.drain(0..drop_n);
        }
    }
}

fn deploy_log_take() -> String {
    if let Ok(mut v) = DEPLOY_LOG.lock() {
        let s = v.join("\n");
        v.clear();
        s
    } else {
        String::new()
    }
}

async fn run_pipeline(
    app: Option<&AppHandle>,
    run_id: i64,
    _prj_guid: &str,
    project_dir: &std::path::Path,
    server: &Value,
    cfg: &Value,
) -> Result<String, String> {
    let host = server["host"].as_str().unwrap_or("").to_string();
    let port = server["port"].as_i64().unwrap_or(22) as u16;
    let user = server["username"].as_str().unwrap_or("").to_string();
    let enc = server["password_enc"].as_str().unwrap_or("");
    let password = if enc.is_empty() { String::new() } else { dpapi_unprotect(enc)? };

    // 1) 构建
    let build_cmd = cfg["build_cmd"].as_str().unwrap_or("").trim().to_string();
    if build_cmd.is_empty() {
        emit(app, run_id, "build", "log", "未配置构建命令，跳过构建");
    } else {
        emit(app, run_id, "build", "start", &format!("开始构建：{}", build_cmd));
        run_build(app, run_id, project_dir, &build_cmd, 1800)?;
    }
    deploy_log_push(format!("[build] {}", if build_cmd.is_empty() { "跳过".to_string() } else { build_cmd.clone() }));
    emit(app, run_id, "build", "ok", "构建完成");

    // 2) 找产物
    let artifact_pattern = cfg["artifact_path"].as_str().unwrap_or("");
    let artifact = resolve_artifact(project_dir, artifact_pattern)?;
    let file_name = artifact.file_name().and_then(|s| s.to_str()).unwrap_or("app").to_string();
    emit(app, run_id, "upload", "start", &format!("产物：{}", artifact.display()));

    // 3) SSH 连接
    emit(app, run_id, "upload", "log", &format!("连接 {}@{}:{} …", user, host, port));
    let session = ssh_connect(&host, port, &user, &password).await?;
    emit(app, run_id, "upload", "log", "SSH 登录成功");

    // 4) 上传
    let remote_dir = cfg["remote_dir"].as_str().unwrap_or("").to_string();
    let remote_path = sftp_upload(app, run_id, &session, &artifact, &remote_dir, &file_name).await?;
    emit(app, run_id, "upload", "ok", &format!("已上传到 {}", remote_path));

    // 5) 启动
    let start_cmd = cfg["start_cmd"].as_str().unwrap_or("").trim().to_string();
    if start_cmd.is_empty() {
        emit(app, run_id, "start", "log", "未配置启动命令，跳过启动");
    } else {
        emit(app, run_id, "start", "start", &format!("执行启动命令：{}", start_cmd));
        let (code, out) = ssh_exec(app, run_id, "start", &session, &start_cmd, 300).await?;
        deploy_log_push(format!("[start] exit={} {}", code, out));
        if code != 0 {
            return Err(format!("启动命令返回非 0（{}）：{}", code, out.trim()));
        }
    }
    emit(app, run_id, "start", "ok", "启动命令执行完成");

    // 6) 启动验证
    let verify_url = cfg["verify_url"].as_str().unwrap_or("");
    let verify_port = cfg["verify_port"].as_i64().unwrap_or(0);
    let retries = cfg["verify_retries"].as_i64().unwrap_or(10);
    emit(app, run_id, "verify", "start", "开始启动验证");
    let vmsg = verify_started(app, run_id, verify_url, &host, verify_port, retries).await?;
    emit(app, run_id, "verify", "ok", &vmsg);

    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "done").await;
    Ok(format!("部署成功：{} → {}（{}）", file_name, server["name"].as_str().unwrap_or(&host), vmsg))
}

/// 部署状态：前端刷新页面时用来恢复「正在部署」
#[tauri::command]
pub fn deploy_status() -> Value {
    json!({ "running": DEPLOY_RUNNING.load(Ordering::SeqCst) })
}

// ============================================================
// 部署配置 / 产物解析 单测
// ============================================================
#[cfg(test)]
mod config_tests {
    use super::*;

    fn mem() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        ensure_tables(&c).unwrap();
        c
    }

    #[test]
    fn config_roundtrip_and_validation() {
        let c = mem();
        assert!(config_save_db(&c, "", 0, "", "", "", "", "", 0, 10).is_err(), "空项目应拒绝");
        assert!(config_save_db(&c, "P1", 0, "", "a", "/tmp", "", "", 0, 10).is_err(), "未选服务器应拒绝");
        assert!(config_save_db(&c, "P1", 1, "", "a", "", "", "", 0, 10).is_err(), "空目标目录应拒绝");
        assert!(config_save_db(&c, "P1", 1, "", "", "/tmp", "", "", 0, 10).is_err(), "空产物路径应拒绝");

        config_save_db(&c, "P1", 7, "mvn package", "target/*.war", "/opt/tomcat/webapps",
                       "systemctl restart tomcat", "http://h:8080/", 8080, 5).unwrap();
        let v = config_row(&c, "P1").unwrap().unwrap();
        assert_eq!(v["server_id"], 7);
        assert_eq!(v["build_cmd"], "mvn package");
        assert_eq!(v["verify_port"], 8080);

        config_save_db(&c, "P1", 8, "", "a.jar", "/srv", "", "", 9000, 999).unwrap();
        let v2 = config_row(&c, "P1").unwrap().unwrap();
        assert_eq!(v2["server_id"], 8);
        assert_eq!(v2["artifact_path"], "a.jar");
        assert_eq!(v2["verify_retries"], 120, "重试次数应被 clamp 到 120");
        assert!(config_row(&c, "P2").unwrap().is_none(), "别的项目读不到这份配置");
    }

    #[test]
    fn defaults_by_project_type() {
        let web = default_config("web", "P");
        assert_eq!(web["artifact_path"], "target/*.war");
        assert_eq!(web["verify_port"], 8080);
        let tauri = default_config("Tauri", "P");
        assert!(tauri["artifact_path"].as_str().unwrap().contains("src-tauri"));
        let win = default_config("windows program", "P");
        assert!(win["start_cmd"].as_str().unwrap().contains("Start-Process"));
    }

    #[test]
    fn artifact_glob_picks_newest() {
        let dir = std::env::temp_dir().join(format!("tdev_art_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a-1.0.war"), b"x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("a-2.0.war"), b"x").unwrap();
        std::fs::write(dir.join("other.jar"), b"x").unwrap();
        let p = resolve_artifact(&dir, "*.war").unwrap();
        assert_eq!(p.file_name().unwrap(), "a-2.0.war", "应取最新修改的产物");
        assert!(resolve_artifact(&dir, "*.zip").is_err());
        assert!(resolve_artifact(&dir, "a-1.0.war").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}


/// 独立探针入口（供 src/bin/probe.rs 调用）：真机跑完整管道，不依赖 Tauri AppHandle
/// 用法：cargo run --release --bin probe
pub async fn probe_pipeline_standalone() -> Result<String, String> {
    let db = std::env::var("TDEV_DB").unwrap_or_else(|_| {
        r"C:\Users\Administrator\AppData\Roaming\com.tdev.app\tdev.db".to_string()
    });
    let conn = rusqlite::Connection::open(&db).map_err(|e| format!("打开 tdev.db 失败：{e}"))?;
    let (host, port, user, enc): (String, i64, String, String) = conn
        .query_row(
            "SELECT host, port, username, password_enc FROM deploy_servers
             WHERE password_enc IS NOT NULL AND password_enc <> '' ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "库里没有可用服务器".to_string())?;
    let _pw = dpapi_unprotect(&enc)?;
    let server = json!({
        "id": 1, "name": "probe", "os_type": "linux",
        "host": host, "port": port, "username": user, "password_enc": enc
    });

    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let real_project = env("TDEV_PROBE_DIR");
    let (dir, temp_created) = match &real_project {
        Some(p) => (std::path::PathBuf::from(p), false),
        None => {
            let d = std::env::temp_dir().join(format!("tdev_pipeline_{}", std::process::id()));
            std::fs::create_dir_all(d.join("build")).map_err(|e| e.to_string())?;
            std::fs::write(d.join("build").join("out.txt"), b"pipeline-ok").map_err(|e| e.to_string())?;
            (d, true)
        }
    };

    let cfg = json!({
        "build_cmd": env("TDEV_PROBE_BUILD").unwrap_or_else(|| "echo build-ok".to_string()),
        "artifact_path": env("TDEV_PROBE_ARTIFACT").unwrap_or_else(|| "build/*.txt".to_string()),
        "remote_dir": env("TDEV_PROBE_REMOTE").unwrap_or_else(|| "/tmp/tdev_pipeline_test".to_string()),
        "start_cmd": env("TDEV_PROBE_START").unwrap_or_else(|| "cat /tmp/tdev_pipeline_test/out.txt".to_string()),
        "verify_url": env("TDEV_PROBE_URL").unwrap_or_default(),
        "verify_port": env("TDEV_PROBE_PORT").map(|v| v.parse::<i64>().unwrap_or(0)).unwrap_or(22),
        "verify_retries": 5
    });

    let result = run_pipeline(None, 0, "probe", &dir, &server, &cfg).await;

    // 只清理“临时造出来”的东西，真项目/真目录一律不动
    if temp_created {
        if let Ok(pw) = dpapi_unprotect(&enc) {
            if let Ok(session) = ssh_connect(&host, port as u16, &user, &pw).await {
                if let Ok(mut ch) = session.channel_open_session().await {
                    let remote = cfg["remote_dir"].as_str().unwrap_or("/tmp/tdev_pipeline_test");
                    let _ = ch.exec(true, format!("rm -rf {}", remote)).await;
                    while ch.wait().await.is_some() {}
                }
            }
        }
        std::fs::remove_dir_all(&dir).ok();
    }
    result
}

/// 构建步骤单独探针：验证 env.bat 自动加载 + GBK 日志
pub fn probe_build_once(project_dir: &str, cmd: &str) -> Result<String, String> {
    let dir = std::path::Path::new(project_dir);
    if !dir.is_dir() {
        return Err(format!("目录不存在：{}", project_dir));
    }
    run_build(None, 0, dir, cmd, 1800)?;
    Ok("构建成功".to_string())
}

/// SSH 探针：在服务器上执行一条命令并打印输出（排查用，超时可用 TDEV_SSH_TIMEOUT 调整）
pub async fn probe_ssh_exec(cmd: &str) -> Result<String, String> {
    let db = std::env::var("TDEV_DB").unwrap_or_else(|_| {
        r"C:\Users\Administrator\AppData\Roaming\com.tdev.app\tdev.db".to_string()
    });
    let conn = rusqlite::Connection::open(&db).map_err(|e| e.to_string())?;
    let (host, port, user, enc): (String, i64, String, String) = conn
        .query_row(
            "SELECT host, port, username, password_enc FROM deploy_servers
             WHERE password_enc IS NOT NULL AND password_enc <> '' ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "库里没有可用服务器".to_string())?;
    let pw = dpapi_unprotect(&enc)?;
    let session = ssh_connect(&host, port as u16, &user, &pw).await?;
    let timeout: u64 = std::env::var("TDEV_SSH_TIMEOUT").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
    let (_code, out) = ssh_exec(None, 0, "probe", &session, cmd, timeout).await?;
    Ok(out.lines().filter(|l| !l.contains("服务器未返回退出码")).collect::<Vec<_>>().join("\n"))
}

// ============================================================
// Agent 部署（第三阶段）：TDev 打通 SSH 免密 + 给上下文，Agent 自主执行
// ============================================================

/// 本机 SSH 密钥路径（没有就生成），返回私钥路径
fn ensure_local_key() -> Result<(String, String), String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "取不到用户目录".to_string())?;
    let dir = std::path::Path::new(&home).join(".ssh");
    let _ = std::fs::create_dir_all(&dir);
    // 专用部署密钥：不依赖用户默认 id_ed25519（它可能带口令，无法非交互使用）
    let key = dir.join("tdev_deploy_ed25519");
    let pubkey = dir.join("tdev_deploy_ed25519.pub");
    if !key.is_file() || !pubkey.is_file() {
        let out = std::process::Command::new("ssh-keygen")
            .args(["-t", "ed25519", "-N", "", "-f"])
            .arg(&key)
            .output()
            .map_err(|e| format!("生成 SSH 密钥失败（找不到 ssh-keygen）：{e}"))?;
        if !out.status.success() {
            return Err(format!(
                "生成 SSH 密钥失败：{}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }
    let pub_text = std::fs::read_to_string(&pubkey)
        .map_err(|e| format!("读取公钥失败：{e}"))?
        .trim()
        .to_string();
    Ok((key.to_string_lossy().to_string(), pub_text))
}

/// 准备 Agent 部署：打通免密登录、确保远端目录、返回上下文与铁律
#[tauri::command]
pub async fn deploy_agent_prepare(
    app: AppHandle,
    prj_guid: String,
    server_id: i64,
) -> Result<Value, String> {
    let (host, port, user, enc, os_type, prj_name, ws, cfg) = {
        let conn = db_conn(&app)?;
        deploy_ensure_table(&conn)?;
        ensure_tables(&conn)?;
        let (host, port, user, enc, os_type): (String, i64, String, String, String) = conn
            .query_row(
                "SELECT host, port, username, COALESCE(password_enc,''), os_type
                 FROM deploy_servers WHERE id=?1 AND prj_guid=?2",
                params![server_id, prj_guid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .map_err(|_| "找不到该服务器（或不属于当前项目）".to_string())?;
        let (prj_name, prj_type): (String, String) = conn
            .query_row(
                "SELECT prjName, prjType FROM projects WHERE prjGUID=?1",
                params![prj_guid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|_| "项目不存在".to_string())?;
        // 界面上已取消“部署配置”表单：这里只用按项目类型算出的参考值，Agent 按服务器实际情况调整
        let cfg = default_config(&prj_type, &prj_name);
        (host, port, user, enc, os_type, prj_name, workspace_path(&conn), cfg)
    };

    if enc.is_empty() {
        return Err("这台服务器没有保存密码，无法打通免密登录，请先编辑服务器补填密码".to_string());
    }
    let password = dpapi_unprotect(&enc)?;
    let (key_path, pubkey) = ensure_local_key()?;

    let session = ssh_connect(&host, port as u16, &user, &password).await?;
    let cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && (grep -qF '{0}' ~/.ssh/authorized_keys || echo '{0}' >> ~/.ssh/authorized_keys) && echo KEY_OK",
        pubkey
    );
    let (code, out) = ssh_exec(None, 0, "prepare", &session, &cmd, 30).await?;
    if code != 0 || !out.contains("KEY_OK") {
        return Err(format!("写入 authorized_keys 失败（退出码 {}）：{}", code, out.trim()));
    }
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "prepared").await;

    // 用本机 ssh 验证免密是否真的通
    let ssh_check = tokio::process::Command::new("ssh")
        .args([
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "IdentitiesOnly=yes",
            "-o", "ConnectTimeout=10",
            "-i", &key_path,
            "-p", &port.to_string(),
            &format!("{}@{}", user, host),
            "echo TDEV_KEY_OK",
        ])
        .output()
        .await
        .map_err(|e| format!("调用 ssh 失败：{e}"))?;
    let ssh_out = String::from_utf8_lossy(&ssh_check.stdout).to_string();
    if !ssh_check.status.success() || !ssh_out.contains("TDEV_KEY_OK") {
        return Err(format!(
            "免密登录验证失败：{} {}",
            String::from_utf8_lossy(&ssh_check.stderr).trim(),
            ssh_out.trim()
        ));
    }

    let project_dir = std::path::Path::new(&ws).join(&prj_name);
    let remote_dir = cfg["remote_dir"].as_str().unwrap_or("").to_string();
    let artifact = cfg["artifact_path"].as_str().unwrap_or("").to_string();
    let verify_url = cfg["verify_url"].as_str().unwrap_or("").to_string();
    let verify_port = cfg["verify_port"].as_i64().unwrap_or(0);

    let context = format!(
r#"【部署任务上下文（由 TDev 提供，直接使用，不要重新询问）】
服务器：{host}:{port}（{os_type}，登录用户 {user}，已配置 SSH 免密，无需密码）
免密登录命令（必须带 -i 和 IdentitiesOnly）：ssh -i "{key_win}" -o IdentitiesOnly=yes -p {port} {user}@{host} '命令'
上传命令：scp -i "{key_win}" -o IdentitiesOnly=yes -P {port} <本地文件> {user}@{host}:<远端目录>/
本地项目目录：{project_dir}
服务器上的目标目录（参考）：{remote_dir}
构建产物（相对项目目录，支持通配，参考）：{artifact}
启动验证地址（参考）：{verify_url}
启动验证端口（参考）：{verify_port}
以上“参考”值只是按项目类型给的默认猜测，必须先看服务器实际情况（Tomcat/服务装在哪、端口占没占、目录能不能写），不一致就按实际调整，并在回复里说明为什么改。
本机构建环境：项目同级有 Public\runtime\env.bat（JDK17+Maven+Tomcat），构建前必须先加载它，例如：
  cmd: call "E:\88.project\TestSpace\Public\runtime\env.bat" && mvn -q -DskipTests clean package

【部署铁律】
1. 必须真的执行命令并把真实输出作为依据，禁止编造“部署成功”。
2. 成功标准：构建产物存在 + 文件确实上传到服务器 + 服务已重启 + 验证地址返回 HTTP 2xx/3xx（或验证端口可连通）。三项缺一不算成功。
3. 失败必须定位：读 Tomcat/应用日志（如 {remote_dir} 同级的 ../logs/catalina.out），说明根因后再决定是否重试；同一问题最多重试 2 次。
4. 禁止 sudo（该账号无免密 sudo）；只用 {remote_dir} 及其父目录，不要动系统目录。
5. 禁止操作与本项目无关的服务：不要碰 docker 容器 / Dify / 80、443 端口。只使用 8080 附近端口。
6. 不要修改项目源码来解决环境问题；环境问题（缺库、缺表、连不上数据库）要如实报告给用户，并给出可执行的下一步建议。
7. 每执行一条关键命令，用一行 `$ 命令` 和 `→ 结果摘要` 记录，最后输出「部署结果：成功/失败」「证据：HTTP 码或端口」「未完成项」。
"#,
        key_win = key_path.replace('\\', "/"),
        host = host, port = port, os_type = os_type, user = user,
        project_dir = project_dir.display(), remote_dir = remote_dir,
        artifact = artifact, verify_url = verify_url, verify_port = verify_port
    );

    Ok(json!({
        "host": host, "port": port, "user": user, "os_type": os_type,
        "key_path": key_path, "project_dir": project_dir.display().to_string(),
        "remote_dir": remote_dir, "artifact_path": artifact,
        "verify_url": verify_url, "verify_port": verify_port,
        "context": context
    }))
}

/// 记录一次 Agent 部署结果（前端在 Agent 结束后调用）
#[tauri::command]
pub fn deploy_release_note(
    app: AppHandle,
    prj_guid: String,
    server_id: i64,
    server_name: String,
    success: bool,
    message: String,
    log: String,
) -> Result<i64, String> {
    let conn = db_conn(&app)?;
    ensure_tables(&conn)?;
    let now = now_str();
    conn.execute(
        "INSERT INTO deploy_releases (prj_guid, server_id, server_name, status, step, message, log, started_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, 'agent', ?5, ?6, ?7, ?7)",
        params![
            prj_guid, server_id, server_name,
            if success { "success" } else { "failed" },
            message, log, now
        ],
    )
    .map_err(|e| format!("记录部署结果失败：{e}"))?;
    Ok(conn.last_insert_rowid())
}

/// Agent 部署准备探针：验证 TDev 专用密钥生成、写入 authorized_keys、免密登录
pub async fn probe_agent_prepare() -> Result<String, String> {
    let db = std::env::var("TDEV_DB").unwrap_or_else(|_| {
        r"C:\Users\Administrator\AppData\Roaming\com.tdev.app\tdev.db".to_string()
    });
    let conn = rusqlite::Connection::open(&db).map_err(|e| e.to_string())?;
    let (host, port, user, enc): (String, i64, String, String) = conn
        .query_row(
            "SELECT host, port, username, password_enc FROM deploy_servers
             WHERE password_enc IS NOT NULL AND password_enc <> '' ORDER BY id LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(|_| "库里没有可用服务器".to_string())?;
    let pw = dpapi_unprotect(&enc)?;
    let (key_path, pubkey) = ensure_local_key()?;
    println!("专用密钥：{}", key_path);
    println!("公钥：{}", pubkey);

    let session = ssh_connect(&host, port as u16, &user, &pw).await?;
    let cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && (grep -qF '{0}' ~/.ssh/authorized_keys || echo '{0}' >> ~/.ssh/authorized_keys) && echo KEY_OK",
        pubkey
    );
    let (code, out) = ssh_exec(None, 0, "prepare", &session, &cmd, 30).await?;
    println!("写入 authorized_keys：exit={} {}", code, out.trim());
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "probe").await;

    let key_win = key_path.replace('\\', "/");
    let check = tokio::process::Command::new("ssh")
        .args([
            "-o", "BatchMode=yes",
            "-o", "StrictHostKeyChecking=accept-new",
            "-o", "IdentitiesOnly=yes",
            "-o", "ConnectTimeout=10",
            "-i", &key_win,
            "-p", &port.to_string(),
            &format!("{}@{}", user, host),
            "echo TDEV_KEY_OK; whoami",
        ])
        .output()
        .await
        .map_err(|e| format!("调用 ssh 失败：{e}"))?;
    if !check.status.success() {
        return Err(format!(
            "免密验证失败：{}",
            String::from_utf8_lossy(&check.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&check.stdout).trim().to_string())
}
