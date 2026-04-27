use md5::{Md5, Digest};
use sha1::Sha1;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::sync::OnceLock;
use tauri::command;
use tauri::Manager;
use tauri::Emitter;
use std::sync::Arc;
use tokio::sync::Mutex;

static HWID: OnceLock<String> = OnceLock::new();
static HIDDEN_HWID: OnceLock<String> = OnceLock::new();

#[command]
pub fn show_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn generate_hwid() -> String {
    use sysinfo::System;
    
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let mut components = Vec::new();
    
    if let Some(cpu) = sys.cpus().first() {
        components.push(cpu.brand().to_string());
        components.push(sys.cpus().len().to_string());
    }
    
    if let Some(hostname) = System::host_name() {
        components.push(hostname);
    }
    components.push(sys.total_memory().to_string());
    
    let disks = sysinfo::Disks::new_with_refreshed_list();
    if let Some(disk) = disks.list().first() {
        components.push(disk.name().to_string_lossy().to_string());
        components.push(disk.total_space().to_string());
    }
    
    if let Some(os_version) = System::os_version() {
        components.push(os_version);
    }
    
    let combined = components.join("|");
    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let result = format!("{:X}", hasher.finalize());
    result
}

fn generate_hidden_hwid() -> String {
    use sysinfo::System;
    
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let mut components = Vec::new();
    
    if let Some(cpu) = sys.cpus().first() {
        components.push(cpu.brand().to_string());
        components.push(sys.cpus().len().to_string());
    }
    
    if let Some(hostname) = System::host_name() {
        components.push(hostname);
    }
    components.push(sys.total_memory().to_string());
    
    if let Some(kernel) = System::kernel_version() {
        components.push(kernel);
    }
    
    if let Some(os_name) = System::name() {
        components.push(os_name);
    }
    if let Some(os_version) = System::os_version() {
        components.push(os_version);
    }
    
    let combined = components.join("|");
    let mut hasher = Sha1::new();
    hasher.update(combined.as_bytes());
    let result = format!("{:X}", hasher.finalize());
    result
}

pub fn get_hwid() -> &'static str {
    HWID.get_or_init(generate_hwid)
}

pub fn get_hidden_hwid() -> &'static str {
    HIDDEN_HWID.get_or_init(generate_hidden_hwid)
}

#[command]
pub fn get_hwid_info() -> Result<(String, String), String> {
    Ok((get_hwid().to_string(), get_hidden_hwid().to_string()))
}

#[derive(Serialize, Deserialize)]
pub struct FetchResponse {
    pub status: u16,
    pub body: String,
}

#[command]
pub async fn fetch_url(
    url: String,
    method: Option<String>,
    body: Option<String>,
    content_type: Option<String>,
) -> Result<FetchResponse, String> {
    let ua = "GameLauncherReborn 2.2.4 (+https://github.com/SPEED0U/RocketLauncher)";
    let x_ua = "GameLauncherReborn 2.2.4";
    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let method = method.unwrap_or_else(|| "GET".to_string());

    let mut request = match method.to_uppercase().as_str() {
        "POST" => client.post(&url),
        "PUT" => client.put(&url),
        "DELETE" => client.delete(&url),
        "HEAD" => client.head(&url),
        _ => client.get(&url),
    };

    request = request.header("X-UserAgent", x_ua);
    request = request.header("X-GameLauncherHash", "1DF8911B158CD1DF88BF95AE21ABA20AA84B4AE6");
    request = request.header("X-HWID", get_hwid());
    request = request.header("X-HiddenHWID", get_hidden_hwid());
    request = request.header("X-GameLauncherCertificate", "");
    request = request.header("X-DiscordID", "");

    if let Some(ct) = content_type {
        request = request.header("Content-Type", ct);
    }

    if let Some(b) = body {
        request = request.body(b);
    }

    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body = response.text().await.map_err(|e| e.to_string())?;

    Ok(FetchResponse { status, body })
}

#[command]
pub async fn fetch_server_list(api_url: String) -> Result<String, String> {
    let ua = "GameLauncherReborn 2.2.4 (+https://github.com/SPEED0U/RocketLauncher)";
    let client = reqwest::Client::builder()
        .user_agent(ua)
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&format!("{}/serverlist.json", api_url))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    response.text().await.map_err(|e| e.to_string())
}

#[command]
pub async fn ping_server(server_ip: String) -> Result<i64, String> {
    let base = server_ip.trim_end_matches('/');
    let url = format!("{}/GetServerInformation", base);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| e.to_string())?;

    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => Ok(1),
        _ => Ok(-1),
    }
}

static PROXY_HANDLE: std::sync::OnceLock<Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>> = std::sync::OnceLock::new();

static APP_HANDLE: std::sync::OnceLock<Arc<Mutex<Option<tauri::AppHandle>>>> = std::sync::OnceLock::new();

fn get_app_handle_store() -> &'static Arc<Mutex<Option<tauri::AppHandle>>> {
    APP_HANDLE.get_or_init(|| Arc::new(Mutex::new(None)))
}

fn get_proxy_handle() -> &'static Arc<Mutex<Option<tokio::task::JoinHandle<()>>>> {
    PROXY_HANDLE.get_or_init(|| Arc::new(Mutex::new(None)))
}

async fn start_proxy(target_base: String) -> Result<u16, String> {
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use http_body_util::{BodyExt, Full};
    use hyper::body::Bytes;

    {
        let mut handle = get_proxy_handle().lock().await;
        if let Some(h) = handle.take() {
            h.abort();
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind proxy: {}", e))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let base = target_base.trim_end_matches('/').to_string();
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(|e| e.to_string())?;

    let handle = tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => continue,
            };

            let base = base.clone();
            let client = client.clone();

            tokio::spawn(async move {
                let io = TokioIo::new(stream);
                let service = service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                    let base = base.clone();
                    let client = client.clone();
                    async move {
                        let method = req.method().clone();
                        let path = req.uri().path_and_query()
                            .map(|pq| pq.as_str().to_string())
                            .unwrap_or_else(|| "/".to_string());

                        let url = format!("{}{}", base, path);
                        let uri_path_for_minimize = path.split('?').next().unwrap_or(&path).to_string();

                        if uri_path_for_minimize.contains("/User/GetPermanentSession") {
                            let handle_store = get_app_handle_store();
                            let lock = handle_store.lock().await;
                            if let Some(ref app) = *lock {
                                let _ = app.emit("game-running", ());
                                if let Some(win) = app.get_webview_window("main") {
                                    let _ = tauri::WebviewWindow::minimize(&win);
                                }
                            }
                        }

                        let reqwest_method = match method.as_str() {
                            "GET" => reqwest::Method::GET,
                            "POST" => reqwest::Method::POST,
                            "PUT" => reqwest::Method::PUT,
                            "DELETE" => reqwest::Method::DELETE,
                            "HEAD" => reqwest::Method::HEAD,
                            "OPTIONS" => reqwest::Method::OPTIONS,
                            "PATCH" => reqwest::Method::PATCH,
                            _ => reqwest::Method::GET,
                        };

                        let mut headers = reqwest::header::HeaderMap::new();
                        for (name, value) in req.headers() {
                            if let Ok(n) = reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()) {
                                if let Ok(v) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                                    headers.insert(n, v);
                                }
                            }
                        }
                        headers.remove(reqwest::header::HOST);

                        let body_bytes = req.into_body().collect().await
                            .map(|c| c.to_bytes())
                            .unwrap_or_default();

                        let resp = client.request(reqwest_method, &url)
                            .headers(headers)
                            .body(body_bytes.to_vec())
                            .send()
                            .await;

                        match resp {
                            Ok(r) => {
                                let status = hyper::StatusCode::from_u16(r.status().as_u16())
                                    .unwrap_or(hyper::StatusCode::BAD_GATEWAY);

                                let mut builder = hyper::Response::builder().status(status);
                                for (name, value) in r.headers() {
                                    builder = builder.header(name.as_str(), value.as_bytes());
                                }

                                let resp_bytes = r.bytes().await.unwrap_or_default();

                                let query = path.split('?').nth(1).unwrap_or("");
                                let uri_path = path.split('?').next().unwrap_or(&path);
                                let is_gzip = builder.headers_ref()
                                    .and_then(|h| h.get("content-encoding"))
                                    .and_then(|v| v.to_str().ok())
                                    .map(|v| v.contains("gzip"))
                                    .unwrap_or(false);

                                if is_gzip {
                                    use flate2::read::GzDecoder;
                                    use std::io::Read;
                                    let mut decoder = GzDecoder::new(&resp_bytes[..]);
                                    let mut decoded = String::new();
                                    match decoder.read_to_string(&mut decoded) {
                                        Ok(_) => {
                                            crate::game_state::handle_game_response(uri_path, query, &decoded);
                                        }
                                        Err(_) => {
                                        }
                                    }
                                } else if let Ok(body_str) = std::str::from_utf8(&resp_bytes) {
                                    crate::game_state::handle_game_response(uri_path, query, body_str);
                                } else {
                                }

                                Ok::<_, hyper::Error>(
                                    builder
                                        .body(Full::new(Bytes::from(resp_bytes.to_vec())))
                                        .unwrap()
                                )
                            }
                            Err(_) => {
                                Ok(hyper::Response::builder()
                                    .status(502)
                                    .body(Full::new(Bytes::from("Bad Gateway")))
                                    .unwrap())
                            }
                        }
                    }
                });

                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, service)
                    .await;
            });
        }
    });

    {
        let mut proxy = get_proxy_handle().lock().await;
        *proxy = Some(handle);
    }

    Ok(port)
}

#[command]
pub async fn launch_game(
    app: tauri::AppHandle,
    game_path: String,
    server_id: String,
    server_name: String,
    server_ip: String,
    login_token: String,
    user_id: String,
    discord_app_id: Option<String>,
    close_on_exit: bool,
    disable_proxy: bool,
) -> Result<(), String> {
    let game_dir = Path::new(&game_path);
    let exe_path = game_dir.join("nfsw.exe");

    if !exe_path.exists() {
        return Err(format!("Game executable not found: {}", exe_path.display()));
    }

    if user_id == "undefined" || user_id.is_empty() {
        return Err(format!("Invalid user_id: '{}'. Please re-login.", user_id));
    }

    let effective_ip = if disable_proxy {
        server_ip.clone()
    } else {
        let scheme = if server_ip.starts_with("https://") || server_ip.starts_with("https:") {
            "https"
        } else {
            "http"
        };
        let without_scheme = server_ip
            .strip_prefix("https://")
            .or_else(|| server_ip.strip_prefix("http://"))
            .unwrap_or(&server_ip);
        let (authority, path) = match without_scheme.find('/') {
            Some(i) => (&without_scheme[..i], &without_scheme[i..]),
            None => (without_scheme, "/"),
        };
        let base = format!("{}://{}", scheme, authority);

        let rpc_server_ip = server_ip.trim_end_matches('/').to_string();
        tokio::spawn(async move {
            crate::rpc_data::init_remote(&rpc_server_ip).await;
        });

        let port = start_proxy(base).await?;
        format!("http://127.0.0.1:{}{}", port, path)
    };

    let display_name = if server_name.trim().is_empty() { server_id.clone() } else { server_name.clone() };
    crate::game_state::set_server_name(&display_name);

    let game_app_id = discord_app_id
        .as_ref()
        .filter(|s| !s.is_empty() && s.len() >= 15 && s.chars().all(|c| c.is_ascii_digit()))
        .cloned();
    let _ = crate::discord_rpc::discord_rpc_reconnect(game_app_id.clone());

    let _ = crate::discord_rpc::discord_rpc_update(
        Some(display_name.clone()),
        Some("Launching Game".to_string()),
        Some("nfsw".to_string()),
        Some("Need for Speed: World".to_string()),
        Some("ingame".to_string()),
        Some("In-Game".to_string()),
        Some("Project Site".to_string()),
        Some("https://soapboxrace.world".to_string()),
        None,
        None,
    );

    #[cfg(windows)]
    let guarded_child = {
        use std::path::Path;
        let server_id_upper = server_id.to_uppercase();
        let args = [
            server_id_upper.as_str(),
            effective_ip.as_str(),
            login_token.as_str(),
            user_id.as_str(),
        ];
        let result = crate::process_guard::launch_guarded(
            Path::new(&exe_path),
            Path::new(&game_dir.to_string_lossy().as_ref()),
            &args,
        );
        match result {
            Ok(child) => {
                use windows_sys::Win32::System::Threading::{
                    SetPriorityClass, SetProcessAffinityMask, ABOVE_NORMAL_PRIORITY_CLASS,
                };
                unsafe {
                    let handle = child.process_handle();
                    SetPriorityClass(handle, ABOVE_NORMAL_PRIORITY_CLASS);
                    let cpu_count = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(1);
                    let capped = cpu_count.min(8).max(1);
                    let mut affinity: usize = 0;
                    for i in 0..capped { affinity |= 1 << i; }
                    SetProcessAffinityMask(handle, affinity);
                }
                Some(child)
            }
            Err(_) => None
        }
    };

    #[cfg(not(windows))]
    let _guarded_child: Option<()> = None;

    #[cfg(windows)]
    let child_plain: Option<std::process::Child> = if guarded_child.is_none() {
        let c = std::process::Command::new(&exe_path)
            .current_dir(&game_dir)
            .arg(server_id.to_uppercase())
            .arg(&effective_ip)
            .arg(&login_token)
            .arg(&user_id)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{
            SetPriorityClass, SetProcessAffinityMask, ABOVE_NORMAL_PRIORITY_CLASS,
        };
        unsafe {
            let handle = c.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
            SetPriorityClass(handle, ABOVE_NORMAL_PRIORITY_CLASS);
            let cpu_count = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1);
            let capped = cpu_count.min(8).max(1);
            let mut affinity: usize = 0;
            for i in 0..capped { affinity |= 1 << i; }
            SetProcessAffinityMask(handle, affinity);
        }
        Some(c)
    } else {
        None
    };

    #[cfg(not(windows))]
    let child_plain = Some(
        std::process::Command::new("wine")
            .env("WINEDLLOVERRIDES", "dinput8=n,b")
            .arg(&exe_path)
            .current_dir(game_dir)
            .arg(server_id.to_uppercase())
            .arg(&effective_ip)
            .arg(&login_token)
            .arg(&user_id)
            .spawn()
            .map_err(|e| format!("Failed to launch game: {}", e))?
    );

    {
        let mut lock = get_app_handle_store().lock().await;
        *lock = Some(app.clone());
    }

    let app_for_exit = app.clone();
    let game_path_for_exit = game_path.clone();
    let close_on_exit_flag = close_on_exit;
    tokio::task::spawn_blocking(move || {
        #[cfg(windows)]
        let exit_code: u32 = {
            if let Some(ref gc) = guarded_child {
                gc.wait()
            } else if let Some(mut c) = child_plain {
                c.wait().ok()
                    .and_then(|s| s.code())
                    .map(|c| c as u32)
                    .unwrap_or(1)
            } else {
                0
            }
        };

        #[cfg(not(windows))]
        let exit_code: u32 = if let Some(mut c) = child_plain {
            c.wait().ok()
                .and_then(|s| s.code())
                .map(|c| c as u32)
                .unwrap_or(1)
        } else {
            0
        };

        #[cfg(windows)]
        drop(guarded_child);

        let _ = crate::downloader::clean_mods(game_path_for_exit);

        if exit_code != 0 {
            let _ = app_for_exit.emit("game-crashed", exit_code);
        }
        let _ = app_for_exit.emit("game-exited", exit_code);
        if close_on_exit_flag {
            if let Some(win) = app_for_exit.get_webview_window("main") {
                let _ = win.close();
            }
            std::process::exit(0);
        } else {
            if let Some(win) = app_for_exit.get_webview_window("main") {
                let _ = tauri::WebviewWindow::unminimize(&win);
                let _ = tauri::WebviewWindow::set_focus(&win);
            }
        }
    });

    Ok(())
}

#[command]
pub fn check_file_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[command]
pub fn check_process_running(process_name: String) -> bool {
    use sysinfo::System;
    let sys = System::new_all();
    
    let target_name = process_name.to_lowercase();
    for (_, process) in sys.processes() {
        let proc_name = process.name().to_str().unwrap_or("").to_lowercase();
        if proc_name == target_name {
            return true;
        }
    }
    false
}

#[command]
pub fn remove_server_mods(game_path: String) -> Result<(), String> {
    use std::path::PathBuf;
    
    let game_dir = PathBuf::from(&game_path);
    let mods_dir = game_dir.join("MODS");
    let data_dir = game_dir.join(".data");
    
    let mut errors = Vec::new();
    
    if mods_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&mods_dir) {
            errors.push(format!("Failed to remove MODS: {}", e));
        }
    }
    
    if data_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&data_dir) {
            errors.push(format!("Failed to remove .data: {}", e));
        }
    }
    
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[command]
pub fn read_settings_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read {}: {}", path, e))
}

#[command]
pub fn write_settings_file(path: String, content: String) -> Result<(), String> {
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }
    std::fs::write(&path, &content).map_err(|e| format!("Failed to write {}: {}", path, e))
}

#[command]
pub fn hash_file_md5(path: String) -> Result<String, String> {
    let mut file =
        std::fs::File::open(&path).map_err(|e| format!("Failed to open {}: {}", path, e))?;

    let mut hasher = Md5::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = file
            .read(&mut buffer)
            .map_err(|e| format!("Read error: {}", e))?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

#[command]
pub fn hash_string_md5(input: String) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[command]
pub fn list_game_files(directory: String) -> Result<Vec<String>, String> {
    let dir = Path::new(&directory);
    if !dir.exists() {
        return Err(format!("Directory not found: {}", directory));
    }

    let mut files = Vec::new();
    collect_files(dir, dir, &mut files).map_err(|e| e.to_string())?;
    Ok(files)
}

fn collect_files(
    base: &Path,
    dir: &Path,
    files: &mut Vec<String>,
) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(base, &path, files)?;
        } else {
            if let Ok(relative) = path.strip_prefix(base) {
                files.push(relative.to_string_lossy().to_string());
            }
        }
    }
    Ok(())
}

#[command]
pub async fn pick_directory() -> Result<Option<String>, String> {
    Ok(None)
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub os_name: String,
    pub os_version: String,
    pub kernel_version: String,
    pub hostname: String,
    pub cpu_brand: String,
    pub cpu_cores: usize,
    pub total_memory: u64,
    pub used_memory: u64,
    pub total_swap: u64,
    pub used_swap: u64,
    pub gpu_name: String,
    pub gpu_driver: String,
    pub disk_free: u64,
    pub disk_total: u64,
    pub disk_kind: String,
}

#[cfg(target_os = "windows")]
fn get_gpu_info() -> (String, String) {
    use std::process::Command;
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x08000000;

    let output = Command::new("powershell")
        .args([
            "-NoProfile", "-NonInteractive", "-Command",
            "Get-WmiObject Win32_VideoController | Select-Object -First 1 Name,DriverVersion | ConvertTo-Json"
        ])
        .creation_flags(NO_WINDOW)
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let name = stdout
                .lines()
                .find(|l| l.contains("\"Name\""))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().trim_matches(['"', ',', ' ']).to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            let driver = stdout
                .lines()
                .find(|l| l.contains("\"DriverVersion\""))
                .and_then(|l| l.split(':').nth(1))
                .map(|v| v.trim().trim_matches(['"', ',', ' ']).to_string())
                .unwrap_or_else(|| "Unknown".to_string());
            (name, driver)
        }
        _ => ("Unknown".to_string(), "Unknown".to_string()),
    }
}

#[cfg(not(target_os = "windows"))]
fn get_gpu_info() -> (String, String) {
    ("N/A".to_string(), "N/A".to_string())
}

#[command]
pub fn get_system_info() -> Result<SystemInfo, String> {
    use sysinfo::System;

    let mut sys = System::new_all();
    sys.refresh_all();

    let cpu_brand = sys.cpus().first()
        .map(|cpu| cpu.brand().to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let (gpu_name, gpu_driver) = get_gpu_info();

    let disks = sysinfo::Disks::new_with_refreshed_list();
    let disk = disks.iter().find(|d| {
        let mount = d.mount_point().to_string_lossy();
        mount == "C:\\" || mount == "C:/" || mount == "/"
    }).or_else(|| disks.iter().next());
    let (disk_free, disk_total, disk_kind) = if let Some(d) = disk {
        let kind = match d.kind() {
            sysinfo::DiskKind::SSD => "SSD".to_string(),
            sysinfo::DiskKind::HDD => "HDD".to_string(),
            _ => "Unknown".to_string(),
        };
        (d.available_space(), d.total_space(), kind)
    } else {
        (0u64, 0u64, "Unknown".to_string())
    };

    let info = SystemInfo {
        os_name: System::name().unwrap_or_else(|| "Unknown".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "Unknown".to_string()),
        kernel_version: System::kernel_version().unwrap_or_else(|| "Unknown".to_string()),
        hostname: System::host_name().unwrap_or_else(|| "Unknown".to_string()),
        cpu_brand,
        cpu_cores: sys.cpus().len(),
        total_memory: sys.total_memory(),
        used_memory: sys.used_memory(),
        total_swap: sys.total_swap(),
        used_swap: sys.used_swap(),
        gpu_name,
        gpu_driver,
        disk_free,
        disk_total,
        disk_kind,
    };

    Ok(info)
}

#[command]
pub fn grant_folder_permissions(path: String) -> Result<(), String> {
    let target = Path::new(&path);
    if !target.exists() {
        return Err(format!("Directory does not exist: {}", path));
    }

    let username = std::env::var("USERNAME").map_err(|_| "Cannot determine current user")?;

    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;

    #[cfg(target_os = "windows")]
    let output = std::process::Command::new("icacls")
        .arg(&path)
        .arg("/grant")
        .arg(format!("{}:(OI)(CI)F", username))
        .arg("/T")
        .arg("/Q")
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("Failed to run icacls: {}", e))?;

    #[cfg(target_os = "windows")]
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("icacls failed: {}", stderr));
    }

    Ok(())
}

fn netsh_run(args: &[&str]) -> Result<std::process::Output, String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("netsh");
    for a in args { cmd.arg(a); }
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.output().map_err(|e| format!("netsh: {}", e))
}

fn run_powershell(script: &str) -> Result<String, String> {
    #[cfg(target_os = "windows")]
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    let out = cmd.output().map_err(|e| format!("PowerShell: {}", e))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let stdout_s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Err(if !err.is_empty() { err } else { stdout_s })
    }
}

fn get_launcher_dir() -> Result<String, String> {
    std::env::current_exe()
        .map_err(|e| format!("Cannot determine launcher exe: {}", e))?
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "Cannot determine launcher directory".to_string())
}

fn get_launcher_exe_path() -> Result<String, String> {
    std::env::current_exe()
        .map_err(|e| format!("Cannot determine launcher exe: {}", e))
        .map(|p| p.to_string_lossy().into_owned())
}

fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[derive(Serialize, Deserialize)]
pub struct FirewallRulesStatus {
    pub has_launcher: bool,
    pub has_game: bool,
}

const FW_RULE_LAUNCHER: &str = "SBRW Launcher";
const FW_RULE_GAME: &str = "SBRW Game";

fn fw_rule_exists(rule_name: &str) -> bool {
    let name_arg = format!("name={}", rule_name);
    netsh_run(&["advfirewall", "firewall", "show", "rule", &name_arg])
        .map(|o| o.status.success() && !String::from_utf8_lossy(&o.stdout).contains("No rules match"))
        .unwrap_or(false)
}

fn fw_add_rule(rule_name: &str, program: &str) -> Result<(), String> {
    let name_arg = format!("name={}", rule_name);
    let prog_arg = format!("program={}", program);
    for dir in &["in", "out"] {
        let dir_arg = format!("dir={}", dir);
        let out = netsh_run(&[
            "advfirewall", "firewall", "add", "rule",
            &name_arg, &dir_arg, "action=allow", &prog_arg,
            "protocol=any", "enable=yes", "profile=any",
        ])?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() && !stdout.to_lowercase().contains("already") {
            return Err(format!("netsh add rule failed: {}", stdout.trim()));
        }
    }
    Ok(())
}

fn fw_delete_rule(rule_name: &str) -> Result<(), String> {
    let name_arg = format!("name={}", rule_name);
    netsh_run(&["advfirewall", "firewall", "delete", "rule", &name_arg])?;
    Ok(())
}

#[command]
pub fn check_firewall_api() -> Result<(), String> {
    let out = netsh_run(&["advfirewall", "show", "allprofiles", "state"])?;
    if out.status.success() {
        Ok(())
    } else {
        Err("Windows Firewall API unavailable".to_string())
    }
}

#[command]
pub fn check_firewall_rules() -> Result<FirewallRulesStatus, String> {
    Ok(FirewallRulesStatus {
        has_launcher: fw_rule_exists(FW_RULE_LAUNCHER),
        has_game: fw_rule_exists(FW_RULE_GAME),
    })
}

#[command]
pub fn add_firewall_rules(game_path: String, which: String) -> Result<(), String> {
    let launcher_exe = get_launcher_exe_path()?;
    let game_exe = Path::new(&game_path).join("nfsw.exe").to_string_lossy().into_owned();
    match which.as_str() {
        "launcher" => fw_add_rule(FW_RULE_LAUNCHER, &launcher_exe),
        "game"     => fw_add_rule(FW_RULE_GAME, &game_exe),
        _          => { fw_add_rule(FW_RULE_LAUNCHER, &launcher_exe)?; fw_add_rule(FW_RULE_GAME, &game_exe) }
    }
}

#[command]
pub fn remove_firewall_rules(which: String) -> Result<(), String> {
    match which.as_str() {
        "launcher" => fw_delete_rule(FW_RULE_LAUNCHER),
        "game"     => fw_delete_rule(FW_RULE_GAME),
        _          => { fw_delete_rule(FW_RULE_LAUNCHER)?; fw_delete_rule(FW_RULE_GAME) }
    }
}

#[derive(Serialize, Deserialize)]
pub struct DefenderExclusionsStatus {
    pub has_launcher: bool,
    pub has_game: bool,
}

#[command]
pub fn check_defender_api() -> Result<(), String> {
    run_powershell("Get-MpPreference -ErrorAction Stop | Out-Null")?;
    Ok(())
}

#[command]
pub fn check_defender_exclusions(game_path: String) -> Result<DefenderExclusionsStatus, String> {
    let launcher_dir = get_launcher_dir()?.to_lowercase();
    let game_lower = game_path.to_lowercase();
    let output = run_powershell(
        "(Get-MpPreference -ErrorAction Stop).ExclusionPath -join \"`n\""
    )?.to_lowercase();
    Ok(DefenderExclusionsStatus {
        has_launcher: !launcher_dir.is_empty() && output.contains(&launcher_dir),
        has_game: !game_lower.is_empty() && output.contains(&game_lower),
    })
}

#[command]
pub fn add_defender_exclusions(game_path: String, which: String) -> Result<(), String> {
    let launcher_dir = get_launcher_dir()?;
    let paths: Vec<String> = match which.as_str() {
        "launcher" => vec![launcher_dir],
        "game"     => vec![game_path],
        _          => vec![launcher_dir, game_path],
    };
    for p in &paths {
        run_powershell(&format!("Add-MpPreference -ExclusionPath '{}' -Force", ps_escape(p)))?;
    }
    Ok(())
}

#[command]
pub fn remove_defender_exclusions(game_path: String, which: String) -> Result<(), String> {
    let launcher_dir = get_launcher_dir()?;
    let paths: Vec<String> = match which.as_str() {
        "launcher" => vec![launcher_dir],
        "game"     => vec![game_path],
        _          => vec![launcher_dir, game_path],
    };
    for p in &paths {
        run_powershell(&format!("Remove-MpPreference -ExclusionPath '{}' -Force", ps_escape(p)))?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
pub struct FolderPermissionsStatus {
    pub launcher_ok: bool,
    pub game_ok: bool,
}

fn has_write_access(dir: &str) -> bool {
    let test = Path::new(dir).join(".sbrw_perm_test");
    let ok = std::fs::write(&test, b"test").is_ok();
    if ok { let _ = std::fs::remove_file(&test); }
    ok
}

#[command]
pub fn check_folder_permissions(game_path: String) -> Result<FolderPermissionsStatus, String> {
    let launcher_dir = get_launcher_dir()?;
    Ok(FolderPermissionsStatus {
        launcher_ok: has_write_access(&launcher_dir),
        game_ok: game_path.is_empty() || has_write_access(&game_path),
    })
}

#[command]
pub fn fix_folder_permissions(game_path: String) -> Result<(), String> {
    let launcher_dir = get_launcher_dir()?;
    grant_folder_permissions(launcher_dir)?;
    if !game_path.is_empty() {
        grant_folder_permissions(game_path)?;
    }
    Ok(())
}

#[command]
pub fn get_game_language() -> Result<String, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "Cannot determine APPDATA")?;
    let settings_path = Path::new(&appdata)
        .join("Need for Speed World")
        .join("Settings")
        .join("UserSettings.xml");

    if !settings_path.exists() {
        return Ok("EN".to_string());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read UserSettings.xml: {}", e))?;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(&content);
    let mut path_stack: Vec<String> = Vec::new();
    let mut capture_next = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name);
                capture_next = path_stack.join("/") == "Settings/UI/Language";
            }
            Ok(Event::End(_)) => {
                path_stack.pop();
                capture_next = false;
            }
            Ok(Event::Text(ref e)) => {
                if capture_next {
                    let val = e.unescape().unwrap_or_default().to_string();
                    if !val.trim().is_empty() {
                        return Ok(val.trim().to_string());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok("EN".to_string())
}

#[command]
pub fn set_game_language(language: String) -> Result<(), String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "Cannot determine APPDATA")?;
    let settings_path = Path::new(&appdata)
        .join("Need for Speed World")
        .join("Settings")
        .join("UserSettings.xml");

    if !settings_path.exists() {
        return Err("UserSettings.xml not found. Launch the game at least once first.".to_string());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read UserSettings.xml: {}", e))?;

    use quick_xml::events::{Event, BytesText};
    use quick_xml::{Reader, Writer};
    use std::io::Cursor;

    let mut reader = Reader::from_str(&content);
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    let mut path_stack: Vec<String> = Vec::new();
    let mut replace_next_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name.clone());

                let current_path = path_stack.join("/");
                replace_next_text = current_path == "Settings/PersistentValue/Chat/DefaultChatGroup"
                    || current_path == "Settings/UI/Language";

                writer.write_event(Event::Start(e.clone())).map_err(|e| e.to_string())?;
            }
            Ok(Event::End(ref e)) => {
                path_stack.pop();
                replace_next_text = false;
                writer.write_event(Event::End(e.clone())).map_err(|e| e.to_string())?;
            }
            Ok(Event::Text(ref _e)) => {
                if replace_next_text {
                    let new_text = BytesText::new(&language);
                    writer.write_event(Event::Text(new_text)).map_err(|e| e.to_string())?;
                    replace_next_text = false;
                } else {
                    writer.write_event(Event::Text(_e.clone())).map_err(|e| e.to_string())?;
                }
            }
            Ok(Event::Eof) => break,
            Ok(e) => writer.write_event(e).map_err(|e| e.to_string())?,
            Err(e) => return Err(format!("XML parse error: {}", e)),
        }
    }

    let result = writer.into_inner().into_inner();
    let output = String::from_utf8(result).map_err(|e| e.to_string())?;

    std::fs::write(&settings_path, output)
        .map_err(|e| format!("Failed to write UserSettings.xml: {}", e))?;

    Ok(())
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GameSettings {
    pub screen_width: i32,
    pub screen_height: i32,
    pub screen_windowed: bool,
    pub brightness: i32,
    pub vsync: bool,
    pub performance_level: i32,
    
    pub base_texture_filter: i32,
    pub base_texture_max_anisotropy: i32,
    pub road_texture_filter: i32,
    pub road_texture_max_anisotropy: i32,
    pub car_environment_map: i32,
    pub global_detail_level: i32,
    pub road_reflection: i32,
    pub shader_detail: i32,
    pub shadow_detail: i32,
    
    pub fsaa_level: i32,
    pub motion_blur: bool,
    pub particle_system: bool,
    pub post_processing: bool,
    pub rain: bool,
    pub water_sim: bool,
    pub visual_treatment: bool,
    pub max_skid_marks: i32,
    
    pub audio_mode: i32,
    pub audio_quality: i32,
    pub master_volume: f32,
    pub sfx_volume: f32,
    pub car_volume: f32,
    pub speech_volume: f32,
    pub music_volume: f32,
    pub frontend_music_volume: f32,
    
    pub camera: i32,
    pub transmission: i32,
    pub damage: bool,
    pub speed_units: i32,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            screen_width: 1024,
            screen_height: 768,
            screen_windowed: false,
            brightness: 52,
            vsync: true,
            performance_level: 2,
            base_texture_filter: 0,
            base_texture_max_anisotropy: 0,
            road_texture_filter: 0,
            road_texture_max_anisotropy: 0,
            car_environment_map: 0,
            global_detail_level: 0,
            road_reflection: 0,
            shader_detail: 0,
            shadow_detail: 0,
            fsaa_level: 0,
            motion_blur: false,
            particle_system: false,
            post_processing: false,
            rain: false,
            water_sim: false,
            visual_treatment: false,
            max_skid_marks: 0,
            audio_mode: 1,
            audio_quality: 0,
            master_volume: 1.0,
            sfx_volume: 0.52,
            car_volume: 0.52,
            speech_volume: 0.52,
            music_volume: 0.52,
            frontend_music_volume: 0.52,
            camera: 2,
            transmission: 1,
            damage: true,
            speed_units: 1,
        }
    }
}

#[command]
pub fn get_game_settings() -> Result<GameSettings, String> {
    
    let appdata = std::env::var("APPDATA").map_err(|_| "Cannot determine APPDATA")?;
    
    let settings_path = Path::new(&appdata)
        .join("Need for Speed World")
        .join("Settings")
        .join("UserSettings.xml");

    
    if !settings_path.exists() {
        return Ok(GameSettings::default());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read UserSettings.xml: {}", e))?;

    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(&content);
    let mut settings = GameSettings::default();
    let mut path_stack: Vec<String> = Vec::new();
    let mut current_element_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name.clone());
                current_element_text.clear();

                let current_path = path_stack.join("/");

                if current_path == "Settings/UI/Audio/AudioOptions" {
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            
                            match key.as_str() {
                                "AudioMode" => settings.audio_mode = value.parse().unwrap_or(1),
                                "MasterVol" => settings.master_volume = value.parse().unwrap_or(1.0),
                                "SFXVol" => settings.sfx_volume = value.parse().unwrap_or(0.52),
                                "CarVol" => settings.car_volume = value.parse().unwrap_or(0.52),
                                "SpeechVol" => settings.speech_volume = value.parse().unwrap_or(0.52),
                                "GameMusicVol" => settings.music_volume = value.parse().unwrap_or(0.52),
                                "FEMusicVol" => settings.frontend_music_volume = value.parse().unwrap_or(0.52),
                                _ => {}
                            }
                        }
                    }
                }

                if current_path == "Settings/UI/Gameplay/GamePlayOptions" {
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            
                            match key.as_str() {
                                "camera" => settings.camera = value.parse().unwrap_or(2),
                                "transmission" => settings.transmission = value.parse().unwrap_or(1),
                                "damage" => settings.damage = value.parse::<i32>().unwrap_or(1) != 0,
                                "speedUnits" => settings.speed_units = value.parse().unwrap_or(1),
                                _ => {}
                            }
                        }
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name.clone());
                let current_path = path_stack.join("/");

                if current_path == "Settings/UI/Audio/AudioOptions" {
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            
                            match key.as_str() {
                                "AudioMode" => settings.audio_mode = value.parse().unwrap_or(1),
                                "MasterVol" => settings.master_volume = value.parse().unwrap_or(1.0),
                                "SFXVol" => settings.sfx_volume = value.parse().unwrap_or(0.52),
                                "CarVol" => settings.car_volume = value.parse().unwrap_or(0.52),
                                "SpeechVol" => settings.speech_volume = value.parse().unwrap_or(0.52),
                                "GameMusicVol" => settings.music_volume = value.parse().unwrap_or(0.52),
                                "FEMusicVol" => settings.frontend_music_volume = value.parse().unwrap_or(0.52),
                                _ => {}
                            }
                        }
                    }
                }

                if current_path == "Settings/UI/Gameplay/GamePlayOptions" {
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            
                            match key.as_str() {
                                "camera" => settings.camera = value.parse().unwrap_or(2),
                                "transmission" => settings.transmission = value.parse().unwrap_or(1),
                                "damage" => settings.damage = value.parse::<i32>().unwrap_or(1) != 0,
                                "speedUnits" => settings.speed_units = value.parse().unwrap_or(1),
                                _ => {}
                            }
                        }
                    }
                }
                
                path_stack.pop();
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string().trim().to_string();
                if !text.is_empty() {
                    current_element_text = text;
                }
            }
            Ok(Event::End(_)) => {
                let current_path = path_stack.join("/");
                
                if !current_element_text.is_empty() {
                    match current_path.as_str() {
                        "Settings/VideoConfig/screenwidth" => {
                            settings.screen_width = current_element_text.parse().unwrap_or(1024);
                        }
                        "Settings/VideoConfig/screenheight" => {
                            settings.screen_height = current_element_text.parse().unwrap_or(768);
                        }
                        "Settings/VideoConfig/screenwindowed" => {
                            settings.screen_windowed = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/brightness" => {
                            settings.brightness = current_element_text.parse().unwrap_or(52);
                        }
                        "Settings/VideoConfig/vsyncon" => {
                            settings.vsync = current_element_text.parse::<i32>().unwrap_or(1) != 0;
                        }
                        "Settings/VideoConfig/performancelevel" => {
                            settings.performance_level = current_element_text.parse().unwrap_or(2);
                        }
                        "Settings/VideoConfig/basetexturefilter" => {
                            settings.base_texture_filter = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/basetexturemaxani" => {
                            settings.base_texture_max_anisotropy = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/roadtexturefilter" => {
                            settings.road_texture_filter = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/roadtexturemaxani" => {
                            settings.road_texture_max_anisotropy = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/carenvironmentmapenable" => {
                            settings.car_environment_map = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/globaldetaillevel" => {
                            settings.global_detail_level = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/roadreflectionenable" => {
                            settings.road_reflection = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/shaderdetail" => {
                            settings.shader_detail = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/shadowdetail" => {
                            settings.shadow_detail = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/fsaalevel" => {
                            settings.fsaa_level = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/motionblurenable" => {
                            settings.motion_blur = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/particlesystemenable" => {
                            settings.particle_system = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/postprocessingenable" => {
                            settings.post_processing = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/rainenable" => {
                            settings.rain = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/watersimenable" => {
                            settings.water_sim = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/visualtreatment" => {
                            settings.visual_treatment = current_element_text.parse::<i32>().unwrap_or(0) != 0;
                        }
                        "Settings/VideoConfig/maxskidmarks" => {
                            settings.max_skid_marks = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/VideoConfig/audiomode" => {
                            settings.audio_mode = current_element_text.parse().unwrap_or(1);
                        }
                        "Settings/VideoConfig/audioquality" => {
                            settings.audio_quality = current_element_text.parse().unwrap_or(0);
                        }
                        "Settings/Physics/CameraPOV" => {
                            settings.camera = current_element_text.parse().unwrap_or(2);
                        }
                        "Settings/Physics/TransmissionType" => {
                            settings.transmission = current_element_text.parse().unwrap_or(1);
                        }
                        _ => {}
                    }
                    current_element_text.clear();
                }
                
                path_stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }

    Ok(settings)
}

#[command]
pub fn test_xml_access() -> Result<String, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "Cannot determine APPDATA")?;
    let settings_path = Path::new(&appdata)
        .join("Need for Speed World")
        .join("Settings")
        .join("UserSettings.xml");
    
    if !settings_path.exists() {
        return Err(format!("File not found at: {}", settings_path.display()));
    }
    
    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read: {}", e))?;
    
    if let Some(start) = content.find("<VideoConfig>") {
        let end = content[start..].find("</VideoConfig>").unwrap_or(500) + start;
        Ok(content[start..end.min(start + 1000)].to_string())
    } else {
        Err("VideoConfig section not found in XML".to_string())
    }
}

#[command]
pub fn set_game_settings(settings: GameSettings) -> Result<(), String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "Cannot determine APPDATA")?;
    let settings_path = Path::new(&appdata)
        .join("Need for Speed World")
        .join("Settings")
        .join("UserSettings.xml");

    if !settings_path.exists() {
        return Err("UserSettings.xml not found. Launch the game at least once first.".to_string());
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read UserSettings.xml: {}", e))?;

    use quick_xml::events::{Event, BytesStart, BytesText};
    use quick_xml::{Reader, Writer};
    use std::io::Cursor;

    let mut reader = Reader::from_str(&content);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut path_stack: Vec<String> = Vec::new();
    let mut skip_text = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name.clone());
                let current_path = path_stack.join("/");

                let elem_name_str = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut new_elem = BytesStart::from_content(elem_name_str, 0);

                for attr in e.attributes() {
                    if let Ok(attr) = attr {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let value = String::from_utf8_lossy(&attr.value);
                        
                        if current_path == "Settings/UI/Audio/AudioOptions" {
                            match key.as_str() {
                                "AudioMode" => {
                                    new_elem.push_attribute(("AudioMode", settings.audio_mode.to_string().as_str()));
                                    continue;
                                }
                                "MasterVol" => {
                                    new_elem.push_attribute(("MasterVol", settings.master_volume.to_string().as_str()));
                                    continue;
                                }
                                "SFXVol" => {
                                    new_elem.push_attribute(("SFXVol", settings.sfx_volume.to_string().as_str()));
                                    continue;
                                }
                                "CarVol" => {
                                    new_elem.push_attribute(("CarVol", settings.car_volume.to_string().as_str()));
                                    continue;
                                }
                                "SpeechVol" => {
                                    new_elem.push_attribute(("SpeechVol", settings.speech_volume.to_string().as_str()));
                                    continue;
                                }
                                "GameMusicVol" => {
                                    new_elem.push_attribute(("GameMusicVol", settings.music_volume.to_string().as_str()));
                                    continue;
                                }
                                "FEMusicVol" => {
                                    new_elem.push_attribute(("FEMusicVol", settings.frontend_music_volume.to_string().as_str()));
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        else if current_path == "Settings/UI/Gameplay/GamePlayOptions" {
                            match key.as_str() {
                                "camera" => {
                                    new_elem.push_attribute(("camera", settings.camera.to_string().as_str()));
                                    continue;
                                }
                                "transmission" => {
                                    new_elem.push_attribute(("transmission", settings.transmission.to_string().as_str()));
                                    continue;
                                }
                                "damage" => {
                                    new_elem.push_attribute(("damage", if settings.damage { "1" } else { "0" }));
                                    continue;
                                }
                                "speedUnits" => {
                                    new_elem.push_attribute(("speedUnits", settings.speed_units.to_string().as_str()));
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        
                        new_elem.push_attribute((key.as_str(), value.as_ref()));
                    }
                }

                writer.write_event(Event::Start(new_elem)).map_err(|e| e.to_string())?;
                
                skip_text = false;
                match current_path.as_str() {
                    "Settings/VideoConfig/screenwidth" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.screen_width.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/screenheight" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.screen_height.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/screenwindowed" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.screen_windowed { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/brightness" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.brightness.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/vsyncon" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.vsync { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/performancelevel" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.performance_level.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/basetexturefilter" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.base_texture_filter.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/basetexturemaxani" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.base_texture_max_anisotropy.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/roadtexturefilter" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.road_texture_filter.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/roadtexturemaxani" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.road_texture_max_anisotropy.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/carenvironmentmapenable" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.car_environment_map.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/globaldetaillevel" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.global_detail_level.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/roadreflectionenable" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.road_reflection.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/shaderdetail" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.shader_detail.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/shadowdetail" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.shadow_detail.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/fsaalevel" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.fsaa_level.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/motionblurenable" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.motion_blur { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/particlesystemenable" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.particle_system { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/postprocessingenable" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.post_processing { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/rainenable" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.rain { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/watersimenable" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.water_sim { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/visualtreatment" => {
                        writer.write_event(Event::Text(BytesText::new(if settings.visual_treatment { "1" } else { "0" }))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/maxskidmarks" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.max_skid_marks.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/audiomode" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.audio_mode.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/VideoConfig/audioquality" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.audio_quality.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/Physics/CameraPOV" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.camera.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    "Settings/Physics/TransmissionType" => {
                        writer.write_event(Event::Text(BytesText::new(&settings.transmission.to_string()))).map_err(|e| e.to_string())?;
                        skip_text = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path_stack.push(name.clone());
                let current_path = path_stack.join("/");

                let elem_name_str = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut new_elem = BytesStart::from_content(elem_name_str, 0);

                for attr in e.attributes() {
                    if let Ok(attr) = attr {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                        let value = String::from_utf8_lossy(&attr.value);
                        
                        if current_path == "Settings/UI/Audio/AudioOptions" {
                            match key.as_str() {
                                "AudioMode" => {
                                    new_elem.push_attribute(("AudioMode", settings.audio_mode.to_string().as_str()));
                                    continue;
                                }
                                "MasterVol" => {
                                    new_elem.push_attribute(("MasterVol", settings.master_volume.to_string().as_str()));
                                    continue;
                                }
                                "SFXVol" => {
                                    new_elem.push_attribute(("SFXVol", settings.sfx_volume.to_string().as_str()));
                                    continue;
                                }
                                "CarVol" => {
                                    new_elem.push_attribute(("CarVol", settings.car_volume.to_string().as_str()));
                                    continue;
                                }
                                "SpeechVol" => {
                                    new_elem.push_attribute(("SpeechVol", settings.speech_volume.to_string().as_str()));
                                    continue;
                                }
                                "GameMusicVol" => {
                                    new_elem.push_attribute(("GameMusicVol", settings.music_volume.to_string().as_str()));
                                    continue;
                                }
                                "FEMusicVol" => {
                                    new_elem.push_attribute(("FEMusicVol", settings.frontend_music_volume.to_string().as_str()));
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        else if current_path == "Settings/UI/Gameplay/GamePlayOptions" {
                            match key.as_str() {
                                "camera" => {
                                    new_elem.push_attribute(("camera", settings.camera.to_string().as_str()));
                                    continue;
                                }
                                "transmission" => {
                                    new_elem.push_attribute(("transmission", settings.transmission.to_string().as_str()));
                                    continue;
                                }
                                "damage" => {
                                    new_elem.push_attribute(("damage", if settings.damage { "1" } else { "0" }));
                                    continue;
                                }
                                "speedUnits" => {
                                    new_elem.push_attribute(("speedUnits", settings.speed_units.to_string().as_str()));
                                    continue;
                                }
                                _ => {}
                            }
                        }
                        
                        new_elem.push_attribute((key.as_str(), value.as_ref()));
                    }
                }

                writer.write_event(Event::Empty(new_elem)).map_err(|e| e.to_string())?;
                path_stack.pop();
            }
            Ok(Event::Text(ref e)) => {
                if !skip_text {
                    writer.write_event(Event::Text(e.clone())).map_err(|e| e.to_string())?;
                }
                skip_text = false;
            }
            Ok(Event::End(ref e)) => {
                path_stack.pop();
                writer.write_event(Event::End(e.clone())).map_err(|e| e.to_string())?;
            }
            Ok(Event::Eof) => break,
            Ok(e) => writer.write_event(e).map_err(|e| e.to_string())?,
            Err(e) => return Err(format!("XML parse error: {}", e)),
        }
    }

    let result = writer.into_inner().into_inner();
    let output = String::from_utf8(result).map_err(|e| e.to_string())?;

    std::fs::write(&settings_path, output)
        .map_err(|e| format!("Failed to write UserSettings.xml: {}", e))?;

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub version: String,
    pub exe: String,
    #[serde(rename = "publishDate")]
    pub publish_date: String,
    #[serde(rename = "productName")]
    pub product_name: String,
}

#[command]
pub async fn check_for_updates() -> Result<Option<UpdateInfo>, String> {
    const UPDATE_URL: &str = "https://rocket.nightriderz.world/latest.json";
    const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
    
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    
    let response = client
        .get(UPDATE_URL)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch update info: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Server returned error: {}", response.status()));
    }
    
    let update_info: UpdateInfo = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse update info: {}", e))?;
    
    
    fn parse_semver(v: &str) -> (u32, u32, u32) {
        let parts: Vec<u32> = v.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        (parts.get(0).copied().unwrap_or(0), parts.get(1).copied().unwrap_or(0), parts.get(2).copied().unwrap_or(0))
    }

    let remote = parse_semver(&update_info.version);
    let current = parse_semver(CURRENT_VERSION);

    if remote > current {
        Ok(Some(update_info))
    } else {
        Ok(None)
    }
}

#[command]
pub async fn download_update(exe_name: String) -> Result<String, String> {
    const BASE_URL: &str = "https://rocket.nightriderz.world";
    
    let download_url = format!("{}/{}", BASE_URL, exe_name);
    
    let temp_dir = std::env::temp_dir();
    let dest_path = temp_dir.join(&exe_name);
    
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    
    let response = client
        .get(&download_url)
        .send()
        .await
        .map_err(|e| format!("Failed to download update: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("Server returned error: {}", response.status()));
    }
    
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read download: {}", e))?;
    
    
    std::fs::write(&dest_path, bytes)
        .map_err(|e| format!("Failed to save installer: {}", e))?;
    
    
    Ok(dest_path.to_string_lossy().to_string())
}

#[command]
pub async fn install_update(installer_path: String) -> Result<(), String> {
    
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        
        Command::new(&installer_path)
            .spawn()
            .map_err(|e| format!("Failed to launch installer: {}", e))?;
        
        
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::process::exit(0);
        });
    }
    
    Ok(())
}

const DXVK_MARKER: &str = ".dxvk_installed";

#[derive(Serialize)]
pub struct DxvkStatus {
    pub installed: bool,
    pub version: Option<String>,
}

#[command]
pub fn check_dxvk(game_path: String) -> Result<DxvkStatus, String> {
    let marker = Path::new(&game_path).join(DXVK_MARKER);
    if marker.exists() {
        let version = std::fs::read_to_string(&marker).ok().map(|s| s.trim().to_string());
        Ok(DxvkStatus { installed: true, version })
    } else {
        Ok(DxvkStatus { installed: false, version: None })
    }
}

#[command]
pub async fn install_dxvk(game_path: String) -> Result<(), String> {
    let game = game_path.clone();
    tokio::task::spawn_blocking(move || install_dxvk_sync(&game))
        .await
        .map_err(|e| format!("Task panic: {}", e))?
}

fn install_dxvk_sync(game_path: &str) -> Result<(), String> {
    let escaped = ps_escape(game_path);
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
$version = '2.4'
$url = 'https://github.com/doitsujin/dxvk/releases/download/v2.4/dxvk-2.4.tar.gz'
$tmp  = $env:TEMP
$tar  = Join-Path $tmp 'dxvk_install.tar.gz'
$dir  = Join-Path $tmp 'dxvk_extract'
$game = '{escaped}'

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
Write-Host "[DXVK] Downloading..."
Invoke-WebRequest -Uri $url -OutFile $tar -UseBasicParsing

if (Test-Path $dir) {{ Remove-Item $dir -Recurse -Force }}
New-Item -ItemType Directory -Path $dir | Out-Null

Write-Host "[DXVK] Extracting..."
$tarExe = "$env:SystemRoot\System32\tar.exe"
if (-not (Test-Path $tarExe)) {{ throw "tar.exe not found (requires Windows 10 1803+)" }}
& $tarExe -xzf $tar -C $dir
if ($LASTEXITCODE -ne 0) {{ throw "tar extraction failed (code $LASTEXITCODE)" }}

$dllSrc = Join-Path $dir "dxvk-$version\x32\d3d9.dll"
if (-not (Test-Path $dllSrc)) {{ throw "x32/d3d9.dll not found in archive at $dllSrc" }}

$dllDst = Join-Path $game 'd3d9.dll'
$dllBak = Join-Path $game 'd3d9.dll.bak'

if ((Test-Path $dllDst) -and -not (Test-Path $dllBak)) {{
    try {{
        # Force OneDrive Files-on-Demand hydration by reading the file first
        $null = [System.IO.File]::ReadAllBytes($dllDst)
        Copy-Item $dllDst $dllBak -Force
        Write-Host "[DXVK] Backed up original d3d9.dll"
    }} catch {{
        Write-Host "[DXVK] Warning: could not back up original d3d9.dll (skipping): $_"
    }}
}}

Copy-Item $dllSrc $dllDst -Force
Write-Host "[DXVK] Installed d3d9.dll"

"$version" | Out-File -FilePath (Join-Path $game '.dxvk_installed') -Encoding utf8 -NoNewline

Remove-Item $tar  -Force -ErrorAction SilentlyContinue
Remove-Item $dir  -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "[DXVK] Done"
"#,
        escaped = escaped
    );

    run_powershell(&script)?;
    Ok(())
}

#[command]
pub fn remove_dxvk(game_path: String) -> Result<(), String> {
    let game = Path::new(&game_path);
    let dll = game.join("d3d9.dll");
    let backup = game.join("d3d9.dll.bak");
    let marker = game.join(DXVK_MARKER);

    if backup.exists() {
        std::fs::copy(&backup, &dll)
            .map_err(|e| format!("Restore backup failed: {}", e))?;
        std::fs::remove_file(&backup)
            .map_err(|e| format!("Remove backup failed: {}", e))?;
    } else if dll.exists() {
        std::fs::remove_file(&dll)
            .map_err(|e| format!("Remove d3d9.dll failed: {}", e))?;
    }

    if marker.exists() {
        let _ = std::fs::remove_file(&marker);
    }

    Ok(())
}
