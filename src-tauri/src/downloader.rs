use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lzma_rs::lzma_decompress;
use md5::{Digest, Md5};
use sha1::Sha1;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, command};
use tokio::sync::Semaphore;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFileEntry {
    pub path: String,
    pub file: String,
    pub length: u64,
    pub compressed: Option<u64>,
    pub hash: Option<String>,
    pub section: u32,
    pub offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexHeader {
    pub total_length: u64,
    pub total_compressed: u64,
    pub firstcab: u64,
    pub lastcab: u64,
}

#[derive(Clone, Serialize)]
pub struct DownloadEvent {
    pub status: String,
    pub file_name: String,
    pub current_file: u32,
    pub total_files: u32,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed: u64,
    pub eta: u64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct VerifyEvent {
    pub status: String,
    pub current_file: String,
    pub current_index: u32,
    pub total_files: u32,
    pub corrupted_count: u32,
}


fn parse_index_xml(xml: &str) -> Result<(IndexHeader, Vec<IndexFileEntry>), String> {
    let mut reader = Reader::from_str(xml);
    let mut entries = Vec::new();
    let mut header = IndexHeader {
        total_length: 0,
        total_compressed: 0,
        firstcab: 1048576,
        lastcab: 0,
    };

    let mut current_entry: Option<IndexFileEntry> = None;
    let mut current_tag = String::new();
    let mut in_header = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                current_tag = name.clone();
                match name.as_str() {
                    "header" => in_header = true,
                    "fileinfo" => {
                        current_entry = Some(IndexFileEntry {
                            path: String::new(),
                            file: String::new(),
                            length: 0,
                            compressed: None,
                            hash: None,
                            section: 0,
                            offset: 0,
                        });
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                match name.as_str() {
                    "header" => in_header = false,
                    "fileinfo" => {
                        if let Some(entry) = current_entry.take() {
                            entries.push(entry);
                        }
                    }
                    _ => {}
                }
                current_tag.clear();
            }
            Ok(Event::Text(e)) => {
                let text = e.unescape().map_err(|e| e.to_string())?.to_string();
                if in_header {
                    match current_tag.as_str() {
                        "length" => header.total_length = text.parse().unwrap_or(0),
                        "compressed" => header.total_compressed = text.parse().unwrap_or(0),
                        "firstcab" => header.firstcab = text.parse().unwrap_or(1048576),
                        "lastcab" => header.lastcab = text.parse().unwrap_or(0),
                        _ => {}
                    }
                } else if let Some(ref mut entry) = current_entry {
                    match current_tag.as_str() {
                        "path" => entry.path = text,
                        "file" => entry.file = text,
                        "length" => entry.length = text.parse().unwrap_or(0),
                        "compressed" => entry.compressed = Some(text.parse().unwrap_or(0)),
                        "hash" => entry.hash = Some(text),
                        "section" => entry.section = text.parse().unwrap_or(0),
                        "offset" => entry.offset = text.parse().unwrap_or(0),
                        _ => {}
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {}", e)),
            _ => {}
        }
    }

    Ok((header, entries))
}


fn is_lzma(data: &[u8]) -> bool {
    data.len() >= 2 && data[0] == 0x5D && data[1] == 0x00
}

fn decompress_lzma(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let mut reader = Cursor::new(data);
    lzma_decompress(&mut reader, &mut output).map_err(|e| format!("LZMA decompress error: {}", e))?;
    Ok(output)
}


fn md5_base64_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Md5::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(BASE64.encode(hasher.finalize()))
}


fn file_sections(entry: &IndexFileEntry, cab_size: u64) -> Vec<u32> {
    let size = entry.compressed.unwrap_or(entry.length);
    let space_in_first = cab_size.saturating_sub(entry.offset);
    if size <= space_in_first {
        return vec![entry.section];
    }
    let extra = size - space_in_first;
    let additional = ((extra + cab_size - 1) / cab_size) as u32;
    (entry.section..=entry.section + additional).collect()
}

fn extract_raw(entry: &IndexFileEntry, sections: &HashMap<u32, Vec<u8>>) -> Result<Vec<u8>, String> {
    let total = entry.compressed.unwrap_or(entry.length) as usize;
    let mut buf = Vec::with_capacity(total);
    let mut remaining = total;
    let mut sec = entry.section;
    let mut off = entry.offset as usize;

    while remaining > 0 {
        let data = sections
            .get(&sec)
            .ok_or_else(|| format!("Missing section{}.dat", sec))?;
        let avail = data.len().saturating_sub(off);
        if avail == 0 {
            return Err(format!(
                "No data in section{}.dat at offset {} (len={})",
                sec,
                off,
                data.len()
            ));
        }
        let take = remaining.min(avail);
        buf.extend_from_slice(&data[off..off + take]);
        remaining -= take;
        sec += 1;
        off = 0;
    }
    Ok(buf)
}

fn strip_first_component(path: &str) -> &str {
    match path.find('/') {
        Some(i) => &path[i + 1..],
        None => "",
    }
}


fn build_cdn_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Rocket Launcher 1.0.0 (+https://github.com/SoapBoxRaceWorld/GameLauncher_NFSW)")
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())
}


struct PackageWork {
    base_url: String,
    files_to_download: Vec<IndexFileEntry>,
    needed_sections: std::collections::BTreeSet<u32>,
}

/// Builds a rayon thread pool limited to half the available cores (min 1, max 4).
/// This prevents heavy CPU work from starving the WebView UI on low-end machines.
fn limited_pool() -> rayon::ThreadPool {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let threads = (cpus / 2).max(1).min(4);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap())
}

/// Single-threaded pool for disk-IO intensive verification.
/// Avoids saturating the disk and starving the WebView on low-end machines.
fn io_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(1).build().unwrap())
}

#[command]
pub async fn download_game(
    app: AppHandle,
    cdn_url: String,
    game_path: String,
) -> Result<(), String> {
    let client = build_cdn_client()?;
    let game_dir = PathBuf::from(&game_path);
    let base = cdn_url.trim_end_matches('/').to_string();
    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    let packages_config = [
        ("", "Core"),
        ("/Tracks", "Tracks"),
        ("/en", "Speech"),
    ];

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: "Analyse des packages...".into(),
        current_file: 0, total_files: 0,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    let mut packages: Vec<PackageWork> = Vec::new();

    for (pkg_path, label) in &packages_config {
        let pkg_url = if pkg_path.is_empty() {
            base.clone()
        } else {
            format!("{}{}", base, pkg_path)
        };

        let index_url = format!("{}/index.xml", pkg_url);
        let resp = client
            .get(&index_url)
            .send()
            .await
            .map_err(|e| format!("[{}] index.xml: {}", label, e))?;

        if !resp.status().is_success() {
            continue;
        }

        let index_xml = resp.text().await.map_err(|e| e.to_string())?;
        let (header, entries) = parse_index_xml(&index_xml)?;
        let cab_size = header.firstcab;

        let _ = app.emit("download-progress", DownloadEvent {
            status: "verifying".into(),
            file_name: format!("[{}] Verifying...", label),
            current_file: 0, total_files: entries.len() as u32,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });

        // Async sequential verification: one spawn_blocking per file + yield_now()
        // between each so the tokio runtime (and WebView) stays fully responsive.
        let total_entries = entries.len() as u32;
        let entries_for_verify: Vec<(PathBuf, Option<String>)> = entries.iter()
            .map(|e| (
                game_dir.join(strip_first_component(&e.path)).join(&e.file),
                e.hash.clone()
            ))
            .collect();
        let label_str = label.to_string();
        let mut verified_needs: Vec<bool> = Vec::with_capacity(total_entries as usize);
        let mut done: u32 = 0;
        for (file_path, expected_hash) in entries_for_verify {
            let needs = tokio::task::spawn_blocking(move || {
                if !file_path.exists() {
                    true
                } else {
                    match expected_hash {
                        Some(expected) => md5_base64_file(&file_path)
                            .map(|actual| actual != expected)
                            .unwrap_or(true),
                        None => false,
                    }
                }
            }).await.unwrap_or(true);
            verified_needs.push(needs);
            done += 1;
            if done % 50 == 0 || done == total_entries {
                let _ = app.emit("download-progress", DownloadEvent {
                    status: "verifying".into(),
                    file_name: format!("[{}] Verifying... {}/{}", label_str, done, total_entries),
                    current_file: done, total_files: total_entries,
                    downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
                });
            }
            tokio::task::yield_now().await;
        };

        let mut files_to_download: Vec<IndexFileEntry> = Vec::new();
        for (i, needs) in verified_needs.into_iter().enumerate() {
            if needs {
                files_to_download.push(entries[i].clone());
            }
        }

        if files_to_download.is_empty() {
            continue;
        }

        let mut needed_sections = std::collections::BTreeSet::new();
        for entry in &files_to_download {
            for sec_id in file_sections(entry, cab_size) {
                needed_sections.insert(sec_id);
            }
        }

        packages.push(PackageWork {
            base_url: pkg_url,
            files_to_download,
            needed_sections,
        });
    }

    if packages.is_empty() {
        let _ = app.emit("download-progress", DownloadEvent {
            status: "completed".into(),
            file_name: "Nothing to download".into(),
            current_file: 0, total_files: 0,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });
        return Ok(());
    }

    let total_sections: u32 = packages.iter().map(|p| p.needed_sections.len() as u32).sum();
    let total_compressed_bytes: u64 = packages.iter()
        .flat_map(|p| &p.files_to_download)
        .map(|e| e.compressed.unwrap_or(e.length))
        .sum();
    let total_uncompressed_bytes: u64 = packages.iter()
        .flat_map(|p| &p.files_to_download)
        .map(|e| e.length)
        .sum();
    let grand_total_bytes: u64 = total_uncompressed_bytes;
    let dl_sem = Arc::new(Semaphore::new(num_cpus.max(4).min(12)));
    let sections_done = Arc::new(AtomicU32::new(0));
    let dl_bytes = Arc::new(AtomicU64::new(0));
    let start_time = std::time::Instant::now();

    let progress_app = app.clone();
    let p_done = sections_done.clone();
    let p_bytes = dl_bytes.clone();
    let progress_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag = progress_running.clone();
    let progress_handle = tokio::spawn(async move {
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.5 { (bytes as f64 / elapsed) as u64 } else { 0 };
            let eta = if speed > 0 && total_compressed_bytes > bytes {
                ((total_compressed_bytes - bytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("Sections {}/{}", done, total_sections),
                current_file: done, total_files: total_sections,
                downloaded_bytes: bytes, total_bytes: grand_total_bytes,
                speed, eta, error: None,
            });
        }
    });

    let mut dl_handles = Vec::new();
    for (pkg_idx, pkg) in packages.iter().enumerate() {
        for &sec_id in &pkg.needed_sections {
            let client = client.clone();
            let url = format!("{}/section{}.dat", pkg.base_url, sec_id);
            let sem = dl_sem.clone();
            let done = sections_done.clone();
            let bytes = dl_bytes.clone();

            dl_handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
                let resp = client.get(&url).send().await
                    .map_err(|e| format!("section{}.dat: {}", sec_id, e))?;
                let data = resp.bytes().await
                    .map_err(|e| format!("section{}.dat read: {}", sec_id, e))?
                    .to_vec();
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                done.fetch_add(1, Ordering::Relaxed);
                Ok::<(usize, u32, Vec<u8>), String>((pkg_idx, sec_id, data))
            }));
        }
    }

    let mut pkg_sections: Vec<HashMap<u32, Vec<u8>>> = packages.iter().map(|_| HashMap::new()).collect();
    let mut errors: Vec<String> = Vec::new();
    for h in dl_handles {
        match h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok((pkg_idx, sec_id, data)) => { pkg_sections[pkg_idx].insert(sec_id, data); }
            Err(e) => errors.push(e),
        }
    }

    progress_running.store(false, Ordering::Relaxed);
    let _ = progress_handle.await;

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let all_entries: Vec<(IndexFileEntry, Arc<HashMap<u32, Vec<u8>>>)> = {
        let pkg_sections_arcs: Vec<Arc<HashMap<u32, Vec<u8>>>> =
            pkg_sections.into_iter().map(Arc::new).collect();
        packages
            .iter()
            .enumerate()
            .flat_map(|(i, pkg)| {
                let arc = pkg_sections_arcs[i].clone();
                pkg.files_to_download
                    .iter()
                    .map(move |e| (e.clone(), arc.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    let total_files = all_entries.len() as u32;
    let files_done = Arc::new(AtomicU32::new(0));
    let bytes_extracted = Arc::new(AtomicU64::new(0));
    let extract_start = std::time::Instant::now();

    {
        let mut dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for (entry, _) in &all_entries {
            let stripped = strip_first_component(&entry.path);
            let dir = if stripped.is_empty() {
                game_dir.clone()
            } else {
                game_dir.join(stripped)
            };
            dirs.insert(dir);
        }
        for dir in &dirs {
            std::fs::create_dir_all(dir).ok();
        }
    }

    let progress_app2 = app.clone();
    let p_files = files_done.clone();
    let p_exbytes = bytes_extracted.clone();
    let progress_running2 = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag2 = progress_running2.clone();
    let progress_handle2 = tokio::spawn(async move {
        while p_flag2.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let done = p_files.load(Ordering::Relaxed);
            let exbytes = p_exbytes.load(Ordering::Relaxed);
            let elapsed = extract_start.elapsed().as_secs_f64();
            let speed = if elapsed > 0.5 { (exbytes as f64 / elapsed) as u64 } else { 0 };
            let eta = if speed > 0 && total_uncompressed_bytes > exbytes {
                ((total_uncompressed_bytes - exbytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app2.emit("download-progress", DownloadEvent {
                status: "extracting".into(),
                file_name: format!("{}/{} files", done, total_files),
                current_file: done, total_files,
                downloaded_bytes: exbytes, total_bytes: grand_total_bytes,
                speed, eta, error: None,
            });
        }
    });

    let fd = files_done.clone();
    let bx = bytes_extracted.clone();
    let gd = game_dir.clone();
    let extract_errors: Vec<String> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
        all_entries
            .into_par_iter()
            .filter_map(|(entry, secs)| {
                let raw = match extract_raw(&entry, &secs) {
                    Ok(r) => r,
                    Err(e) => return Some(e),
                };
                let data = if is_lzma(&raw) {
                    decompress_lzma(&raw).unwrap_or(raw)
                } else {
                    raw
                };
                let stripped = strip_first_component(&entry.path);
                let dest = if stripped.is_empty() {
                    gd.join(&entry.file)
                } else {
                    gd.join(stripped).join(&entry.file)
                };
                let written = data.len() as u64;
                if let Err(e) = std::fs::write(&dest, &data) {
                    return Some(format!("write {}: {}", entry.file, e));
                }
                bx.fetch_add(written, Ordering::Relaxed);
                fd.fetch_add(1, Ordering::Relaxed);
                None
            })
            .collect()
        })
    })
    .await
    .unwrap_or_else(|e| vec![e.to_string()]);
    errors.extend(extract_errors);

    progress_running2.store(false, Ordering::Relaxed);
    let _ = progress_handle2.await;

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Download complete".into(),
        current_file: total_files, total_files,
        downloaded_bytes: total_uncompressed_bytes, total_bytes: total_uncompressed_bytes,
        speed: 0, eta: 0, error: None,
    });

    Ok(())
}


async fn verify_package(
    app: &AppHandle,
    client: &reqwest::Client,
    base_url: &str,
    game_dir: &Path,
    package_label: &str,
) -> Result<Vec<String>, String> {
    let checksums_url = format!("{}/unpacked/checksums.dat", base_url);
    let resp = client
        .get(&checksums_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch checksums.dat for {}: {}", package_label, e))?;

    if !resp.status().is_success() {
        return Ok(Vec::new());
    }

    let checksums_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read checksums.dat: {}", e))?;

    let lines: Vec<&str> = checksums_text.lines().collect();
    let total = lines.len() as u32;
    let mut corrupted = Vec::new();
    let mut scanned = 0u32;

    for line in lines {
        let parts: Vec<&str> = line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let expected_hash = parts[0].trim();
        let file_path = parts[1];

        let file_relative = file_path
            .trim_start_matches('/')
            .trim_start_matches('\\')
            .replace('/', std::path::MAIN_SEPARATOR_STR)
            .replace('\\', std::path::MAIN_SEPARATOR_STR);
        
        let full_path = game_dir.join(&file_relative);

        scanned += 1;

        let _ = app.emit("verify-progress", VerifyEvent {
            status: "scanning".into(),
            current_file: format!("[{}] {}", package_label, file_relative),
            current_index: scanned,
            total_files: total,
            corrupted_count: corrupted.len() as u32,
        });

        if !full_path.exists() {
            corrupted.push(file_relative.clone());
            continue;
        }

        match sha1_file(&full_path) {
            Ok(actual_hash) => {
                let actual = actual_hash.trim().to_uppercase();
                let expected = expected_hash.trim().to_uppercase();
                if actual != expected {
                    corrupted.push(file_relative.clone());
                }
            }
            Err(_) => {
                corrupted.push(file_relative.clone());
            }
        }
    }

    Ok(corrupted)
}

fn sha1_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 65536];
    
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    
    Ok(format!("{:X}", hasher.finalize()))
}

#[command]
pub async fn verify_game_files(
    app: AppHandle,
    cdn_url: String,
    game_path: String,
) -> Result<Vec<String>, String> {
    let client = build_cdn_client()?;
    let game_dir = PathBuf::from(&game_path);
    let base = cdn_url.trim_end_matches('/');

    let _ = app.emit("verify-progress", VerifyEvent {
        status: "scanning".into(),
        current_file: "Verifying...".into(),
        current_index: 0, total_files: 0, corrupted_count: 0,
    });

    let corrupted = verify_package(&app, &client, base, &game_dir, "Game").await?;

    let _ = app.emit("verify-progress", VerifyEvent {
        status: "completed".into(),
        current_file: String::new(),
        current_index: 0, total_files: 0,
        corrupted_count: corrupted.len() as u32,
    });

    Ok(corrupted)
}


async fn repair_package(
    app: &AppHandle,
    client: &reqwest::Client,
    base_url: &str,
    game_dir: &Path,
    corrupted_files: &[String],
    _package_label: &str,
) -> Result<(), String> {
    if corrupted_files.is_empty() {
        return Ok(());
    }

    let total = corrupted_files.len() as u32;
    let downloaded = Arc::new(AtomicU32::new(0));
    let total_bytes = Arc::new(AtomicU64::new(0));

    let num_cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    let dl_sem = Arc::new(Semaphore::new(num_cpus.min(4)));

    let mut dl_handles = Vec::new();
    for file_path in corrupted_files {
        let client = client.clone();
        let base = base_url.to_string();
        let game_path = game_dir.to_path_buf();
        let file_path = file_path.clone();
        let sem = dl_sem.clone();
        let done = downloaded.clone();
        let bytes_arc = total_bytes.clone();
        let app_clone = app.clone();
        let total_clone = total;

        dl_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;

            let url_path = file_path.replace(std::path::MAIN_SEPARATOR, "/");
            let url = format!("{}/unpacked/{}", base.trim_end_matches('/'), url_path.trim_start_matches('/'));

            let resp = client
                .get(&url)
                .send()
                .await
                .map_err(|e| format!("Download {}: {}", file_path, e))?;

            if !resp.status().is_success() {
                return Err(format!("HTTP {} for {}", resp.status(), file_path));
            }

            let data = resp
                .bytes()
                .await
                .map_err(|e| format!("Read {}: {}", file_path, e))?
                .to_vec();

            bytes_arc.fetch_add(data.len() as u64, Ordering::Relaxed);

            let full_path = game_path.join(&file_path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Create dir for {}: {}", file_path, e))?;
            }

            std::fs::write(&full_path, &data)
                .map_err(|e| format!("Write {}: {}", file_path, e))?;

            let done_val = done.fetch_add(1, Ordering::Relaxed) + 1;
            let _ = app_clone.emit("verify-progress", VerifyEvent {
                status: "repairing".into(),
                current_file: file_path.clone(),
                current_index: done_val,
                total_files: total_clone,
                corrupted_count: 0,
            });

            Ok::<(), String>(())
        }));
    }

    let mut errors: Vec<String> = Vec::new();
    for h in dl_handles {
        if let Err(e) = h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            errors.push(e);
        }
    }

    if !errors.is_empty() {
        return Err(format!("Repair errors: {}", errors.join("; ")));
    }

    Ok(())
}

#[command]
pub async fn repair_game_files(
    app: AppHandle,
    cdn_url: String,
    game_path: String,
    corrupted_files: Vec<String>,
) -> Result<(), String> {
    let client = build_cdn_client()?;
    let game_dir = PathBuf::from(&game_path);
    let base = cdn_url.trim_end_matches('/');

    repair_package(&app, &client, base, &game_dir, &corrupted_files, "Game").await?;

    let _ = app.emit(
        "download-progress",
        DownloadEvent {
            status: "completed".into(),
            file_name: "Repair complete".into(),
            current_file: 0,
            total_files: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
        },
    );

    Ok(())
}


fn sha256_hex_file(path: &Path) -> Result<String, String> {
    use sha2::{Sha256, Digest as Sha2Digest};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[command]
pub async fn download_modnet_modules(
    app: AppHandle,
    game_path: String,
    modnet_cdn: String,
) -> Result<(), String> {
    let client = build_cdn_client()?;
    let game_dir = PathBuf::from(&game_path);

    let modules_url = format!("{}/launcher-modules/modules.json", modnet_cdn.trim_end_matches('/'));
    let resp = client
        .get(&modules_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch modules.json: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("modules.json returned HTTP {}", resp.status()));
    }

    let body = resp.text().await.map_err(|e| e.to_string())?;
    let modules: HashMap<String, String> = serde_json::from_str(&body)
        .map_err(|e| format!("Failed to parse modules.json: {}", e))?;

    if modules.is_empty() {
        return Ok(());
    }

    let total = modules.len() as u32;

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: "Checking ModNet modules...".into(),
        current_file: 0, total_files: total,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    // Async sequential verification: one spawn_blocking per file + yield_now()
    let mut dlls_to_download: Vec<String> = Vec::new();
    let mut modnet_done: u32 = 0;
    for (name, expected) in &modules_vec {
        let path = game_dir.join(name);
        let exp = expected.clone();
        let needs = tokio::task::spawn_blocking(move || {
            if path.exists() {
                sha256_hex_file(&path).map(|actual| actual != exp).unwrap_or(true)
            } else {
                true
            }
        }).await.unwrap_or(true);
        if needs { dlls_to_download.push(name.clone()); }
        modnet_done += 1;
        let _ = app.emit("download-progress", DownloadEvent {
            status: "verifying".into(),
            file_name: format!("Checking ModNet modules... {}/{}", modnet_done, total),
            current_file: modnet_done, total_files: total,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });
        tokio::task::yield_now().await;
}

    if dlls_to_download.is_empty() {
        let _ = app.emit("download-progress", DownloadEvent {
            status: "completed".into(),
            file_name: "ModNet modules up to date".into(),
            current_file: total, total_files: total,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });
        return Ok(());
    }

    let dl_total = dlls_to_download.len() as u32;

    // Phase 1: fire all GET requests in parallel to collect Content-Length headers
    // before starting the progress loop, so total_bytes is known upfront.
    let pre_handles: Vec<_> = dlls_to_download.iter().map(|name| {
        let c = client.clone();
        let url = format!("{}/launcher-modules/{}", modnet_cdn.trim_end_matches('/'), name);
        let n = name.clone();
        tokio::spawn(async move {
            let resp = c.get(&url).send().await.map_err(|e| format!("{}: {}", n, e))?;
            let len = resp.content_length().unwrap_or(0);
            Ok::<(String, u64, reqwest::Response), String>((n, len, resp))
        })
    }).collect();

    let mut named_responses: Vec<Result<(String, reqwest::Response), String>> = Vec::new();
    let mut known_total = 0u64;
    for h in pre_handles {
        match h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok((name, len, resp)) => {
                known_total += len;
                named_responses.push(Ok((name, resp)));
            }
            Err(e) => named_responses.push(Err(e)),
        }
    }

    let total_modnet_bytes = Arc::new(AtomicU64::new(known_total));
    let dl_sem = Arc::new(Semaphore::new(16));
    let dl_done = Arc::new(AtomicU32::new(0));
    let dl_bytes = Arc::new(AtomicU64::new(0));
    let start_time = std::time::Instant::now();

    let progress_app = app.clone();
    let p_done = dl_done.clone();
    let p_bytes = dl_bytes.clone();
    let p_total = total_modnet_bytes.clone();
    let progress_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag = progress_running.clone();
    let progress_handle = tokio::spawn(async move {
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.5 { (bytes as f64 / elapsed) as u64 } else { 0 };
            let total_known = p_total.load(Ordering::Relaxed);
            let eta = if speed > 0 && total_known > bytes {
                ((total_known - bytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("[ModNet] {}/{}", done, dl_total),
                current_file: done, total_files: dl_total,
                downloaded_bytes: bytes, total_bytes: total_known,
                speed, eta, error: None,
            });
        }
    });

    // Phase 2: download bodies using already-open responses, limited by semaphore
    let mut dl_handles = Vec::new();
    for result in named_responses {
        let sem = dl_sem.clone();
        let done = dl_done.clone();
        let bytes = dl_bytes.clone();
        let gd = game_dir.clone();

        dl_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
            let (dll_name, response) = result?;
            let data = response.bytes().await
                .map_err(|e| format!("{} read: {}", dll_name, e))?;
            let dll_path = gd.join(&dll_name);
            let tmp_path = gd.join(format!(".{}.tmp", dll_name));
            tokio::fs::write(&tmp_path, &data).await
                .map_err(|e| format!("write {}: {}", dll_name, e))?;
            tokio::fs::rename(&tmp_path, &dll_path).await
                .map_err(|e| format!("rename {}: {}", dll_name, e))?;
            bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            done.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }));
    }

    let mut errors: Vec<String> = Vec::new();
    for h in dl_handles {
        if let Err(e) = h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            errors.push(e);
        }
    }

    progress_running.store(false, Ordering::Relaxed);
    let _ = progress_handle.await;

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "ModNet modules installed".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: dl_bytes.load(Ordering::Relaxed),
        total_bytes: dl_bytes.load(Ordering::Relaxed),
        speed: 0, eta: 0, error: None,
    });

    Ok(())
}


#[derive(Debug, Serialize, Deserialize)]
pub struct ModInfo {
    #[serde(alias = "basePath")]
    pub base_path: Option<String>,
    #[serde(alias = "serverID")]
    pub server_id: Option<String>,
    pub features: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModListEntry {
    #[serde(alias = "Name")]
    pub name: Option<String>,
    #[serde(alias = "Checksum")]
    pub checksum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModList {
    pub built_at: Option<String>,
    pub entries: Option<Vec<ModListEntry>>,
}

fn sha1_hex_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[command]
pub async fn fetch_mod_info(server_ip: String) -> Result<Option<ModInfo>, String> {
    let client = build_cdn_client()?;
    let url = format!("{}/Modding/GetModInfo", server_ip);

    match client.get(&url).send().await {
        Ok(resp) => {
            if resp.status().as_u16() == 404 {
                return Ok(None);
            }
            let body = resp.text().await.map_err(|e| e.to_string())?;
            match serde_json::from_str::<ModInfo>(&body) {
                Ok(info) => Ok(Some(info)),
                Err(_) => Ok(None),
            }
        }
        Err(_) => Ok(None),
    }
}

#[command]
pub async fn download_mods(
    app: AppHandle,
    base_path: String,
    server_id: String,
    game_path: String,
) -> Result<(), String> {
    let client = build_cdn_client()?;
    let game_dir = PathBuf::from(&game_path);

    let index_url = format!("{}/index.json", base_path.trim_end_matches('/'));
    let resp = client
        .get(&index_url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch index.json: {}", e))?;

    if resp.status().as_u16() == 404 {
        return Ok(());
    }

    let modlist: ModList = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse index.json: {}", e))?;

    let entries = modlist.entries.unwrap_or_default();
    if entries.is_empty() {
        return Ok(());
    }

    let server_hash = {
        let mut h = Md5::new();
        h.update(server_id.as_bytes());
        format!("{:x}", h.finalize())
    };
    let cache_dir = game_dir.join("MODS").join(&server_hash);
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let total_mods = entries.len() as u32;

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: "Verifying mods...".into(),
        current_file: 0, total_files: total_mods,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    let entries_vec: Vec<(String, Option<String>)> = entries.iter()
        .map(|e| (e.name.clone().unwrap_or_default(), e.checksum.clone()))
        .collect();

    // Async sequential verification: one spawn_blocking per file + yield_now()
    let mut mods_to_download: Vec<String> = Vec::new();
    let mut any_mismatch = false;
    let mut mods_done: u32 = 0;
    for (name, expected) in entries_vec {
        let cached_file = cache_dir.join(&name);
        let (needs_dl, is_mismatch) = tokio::task::spawn_blocking(move || {
            if !cached_file.exists() {
                (true, false)
            } else {
                match expected {
                    Some(exp) => {
                        let mismatch = sha1_hex_file(&cached_file)
                            .map(|actual| actual != exp.to_lowercase())
                            .unwrap_or(true);
                        (mismatch, mismatch)
                    }
                    None => (false, false),
                }
            }
        }).await.unwrap_or((true, false));
        if needs_dl { mods_to_download.push(name); }
        if is_mismatch { any_mismatch = true; }
        mods_done += 1;
        if mods_done % 10 == 0 || mods_done == total_mods {
            let _ = app.emit("download-progress", DownloadEvent {
                status: "verifying".into(),
                file_name: format!("Verifying mods... {}/{}", mods_done, total_mods),
                current_file: mods_done, total_files: total_mods,
                downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
            });
        }
        tokio::task::yield_now().await;
    }

    if mods_to_download.is_empty() {
        return Ok(());
    }

    let dl_total = mods_to_download.len() as u32;

    // Phase 1: fire all GET requests in parallel to collect Content-Length headers
    // before starting the progress loop, so total_bytes is known upfront.
    let pre_handles: Vec<_> = mods_to_download.iter().map(|name| {
        let c = client.clone();
        let url = format!("{}/{}", base_path.trim_end_matches('/'), name);
        let n = name.clone();
        tokio::spawn(async move {
            let resp = c.get(&url).send().await.map_err(|e| format!("mod {}: {}", n, e))?;
            let len = resp.content_length().unwrap_or(0);
            Ok::<(String, u64, reqwest::Response), String>((n, len, resp))
        })
    }).collect();

    let mut named_responses: Vec<Result<(String, reqwest::Response), String>> = Vec::new();
    let mut known_total = 0u64;
    for h in pre_handles {
        match h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            Ok((name, len, resp)) => {
                known_total += len;
                named_responses.push(Ok((name, resp)));
            }
            Err(e) => named_responses.push(Err(e)),
        }
    }

    let total_mods_bytes = Arc::new(AtomicU64::new(known_total));
    let dl_sem = Arc::new(Semaphore::new(16));
    let dl_done = Arc::new(AtomicU32::new(0));
    let dl_bytes = Arc::new(AtomicU64::new(0));
    let start_time = std::time::Instant::now();

    let progress_app = app.clone();
    let p_done = dl_done.clone();
    let p_bytes = dl_bytes.clone();
    let p_total = total_mods_bytes.clone();
    let progress_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag = progress_running.clone();
    let progress_handle = tokio::spawn(async move {
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            let elapsed = start_time.elapsed().as_secs_f64();
            let speed = if elapsed > 0.5 { (bytes as f64 / elapsed) as u64 } else { 0 };
            let total_known = p_total.load(Ordering::Relaxed);
            let eta = if speed > 0 && total_known > bytes {
                ((total_known - bytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("[MODS] {}/{}", done, dl_total),
                current_file: done, total_files: dl_total,
                downloaded_bytes: bytes, total_bytes: total_known,
                speed, eta, error: None,
            });
        }
    });

    // Phase 2: download bodies using already-open responses, limited by semaphore
    let mut dl_handles = Vec::new();
    for result in named_responses {
        let sem = dl_sem.clone();
        let done = dl_done.clone();
        let bytes = dl_bytes.clone();
        let cache = cache_dir.clone();

        dl_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
            let (name, response) = result?;
            let data = response.bytes().await
                .map_err(|e| format!("mod {} read: {}", name, e))?;
            let cached_file = cache.join(&name);
            if let Some(parent) = cached_file.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let tmp_path = cache.join(format!(".{}.tmp", &name));
            tokio::fs::write(&tmp_path, &data).await
                .map_err(|e| format!("write mod {}: {}", name, e))?;
            tokio::fs::rename(&tmp_path, &cached_file).await
                .map_err(|e| format!("rename mod {}: {}", name, e))?;
            bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            done.fetch_add(1, Ordering::Relaxed);
            Ok::<(), String>(())
        }));
    }

    let mut errors: Vec<String> = Vec::new();
    for h in dl_handles {
        if let Err(e) = h.await.map_err(|e| e.to_string()).and_then(|r| r) {
            errors.push(e);
        }
    }

    progress_running.store(false, Ordering::Relaxed);
    let _ = progress_handle.await;

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    if any_mismatch {
        let data_dir = game_dir.join(".data").join(&server_hash);
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir).ok();
        }
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Mods downloaded".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: dl_bytes.load(Ordering::Relaxed), total_bytes: dl_bytes.load(Ordering::Relaxed),
        speed: 0, eta: 0, error: None,
    });

    Ok(())
}


#[command]
pub fn clean_mods(game_path: String) -> Result<(), String> {
    let game_dir = PathBuf::from(&game_path);
    let links_file = game_dir.join(".links");

    if links_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&links_file) {
            for line in content.lines() {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() < 2 {
                    continue;
                }
                let loc = parts[0];
                let link_type: i32 = parts[1].parse().unwrap_or(-1);

                let real_loc = if std::path::Path::new(loc).is_absolute() {
                    PathBuf::from(loc)
                } else {
                    game_dir.join(loc)
                };
                let orig_path = {
                    let mut p = real_loc.as_os_str().to_os_string();
                    p.push(".orig");
                    PathBuf::from(p)
                };

                if link_type == 0 {
                    if real_loc.exists() || real_loc.symlink_metadata().is_ok() {
                        std::fs::remove_file(&real_loc).ok();
                    }
                    if orig_path.exists() {
                        std::fs::rename(&orig_path, &real_loc).ok();
                    }
                } else if link_type == 1 {
                    if real_loc.exists() {
                        std::fs::remove_dir_all(&real_loc).ok();
                    }
                }
            }
        }
        std::fs::remove_file(&links_file).ok();
    }

    fn restore_orig_files(dir: &std::path::Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                restore_orig_files(&path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) == Some("orig") {
                let mut original = path.clone();
                original.set_extension("");
                if original.exists() || original.symlink_metadata().is_ok() {
                    std::fs::remove_file(&original).ok();
                }
                std::fs::rename(&path, &original).ok();
            }
        }
    }
    restore_orig_files(&game_dir);

    let artifacts = ["lightfx.dll", "ModManager.dat", "PocoFoundation.dll"];
    for artifact in &artifacts {
        let p = game_dir.join(artifact);
        if p.exists() {
            std::fs::remove_file(&p).ok();
        }
    }

    let modules_dir = game_dir.join("modules");
    if modules_dir.exists() {
        std::fs::remove_dir_all(&modules_dir).ok();
    }

    std::fs::create_dir_all(game_dir.join("scripts")).ok();

    Ok(())
}


#[command]
pub async fn fetch_cdn_list(api_url: String) -> Result<String, String> {
    let client = build_cdn_client()?;
    let url = format!("{}/cdn_list.json", api_url.trim_end_matches('/'));

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch CDN list: {}", e))?;

    resp.text().await.map_err(|e| e.to_string())
}
