use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use lzma_rs::lzma_decompress;
use md5::{Digest, Md5};
use sha1::Sha1;
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{AppHandle, Emitter, command};
use tokio::io::AsyncWriteExt;
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

/// Same as extract_raw but works with sections stored as Arc<Vec<u8>>.
fn extract_raw_arc(entry: &IndexFileEntry, sections: &HashMap<u32, Arc<Vec<u8>>>) -> Result<Vec<u8>, String> {
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
                sec, off, data.len()
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


/// Short-window speed estimator.
///
/// Uses the latest samples to estimate transfer speed with less lag than a
/// long EWMA, while still smoothing single-tick spikes.
struct SpeedEwma {
    samples: VecDeque<(std::time::Instant, u64)>,
    smoothed: f64,
    alpha: f64,
    window: std::time::Duration,
}

impl SpeedEwma {
    fn new(alpha: f64) -> Self {
        Self {
            samples: VecDeque::with_capacity(8),
            smoothed: 0.0,
            alpha,
            window: std::time::Duration::from_secs(2),
        }
    }

    /// Push the current cumulative byte count and return smoothed bytes/sec.
    fn push(&mut self, bytes: u64) -> u64 {
        let now = std::time::Instant::now();
        if let Some((_, last_bytes)) = self.samples.back().copied() {
            if bytes < last_bytes {
                self.samples.clear();
                self.smoothed = 0.0;
            }
        }

        self.samples.push_back((now, bytes));
        while self.samples.len() > 2 {
            if let Some((first_time, _)) = self.samples.front().copied() {
                if now.duration_since(first_time) > self.window {
                    self.samples.pop_front();
                    continue;
                }
            }
            break;
        }

        if self.samples.len() < 2 {
            return self.smoothed.max(0.0) as u64;
        }

        let (first_time, first_bytes) = self.samples.front().copied().unwrap();
        let (last_time, last_bytes) = self.samples.back().copied().unwrap();
        let dt = last_time.duration_since(first_time).as_secs_f64();
        if dt < 0.05 {
            return self.smoothed.max(0.0) as u64;
        }

        let instant = last_bytes.saturating_sub(first_bytes) as f64 / dt;
        let alpha = self.alpha.clamp(0.25, 0.60);
        self.smoothed = if self.smoothed == 0.0 {
            instant
        } else {
            alpha * instant + (1.0 - alpha) * self.smoothed
        };
        self.smoothed.max(0.0) as u64
    }
}

fn build_cdn_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Rocket Launcher 1.0.0 (+https://github.com/SoapBoxRaceWorld/GameLauncher_NFSW)")
        .timeout(std::time::Duration::from_secs(300))
        // The CDN serves raw binary .dat archives — disable automatic
        // gzip/brotli decoding to prevent "error decoding response body"
        // when the server sends a spurious Content-Encoding header or
        // the connection drops mid-decompression stream.
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .build()
        .map_err(|e| e.to_string())
}


struct PackageWork {
    base_url: String,
    cab_size: u64,
    files_to_download: Vec<IndexFileEntry>,
    needed_sections: std::collections::BTreeSet<u32>,
}

fn cpu_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}

fn adaptive_download_slots_game() -> usize {
    // Scale with CPU, but keep sane bounds so tiny configs do not thrash and
    // large configs can still saturate fast networks.
    (cpu_count().saturating_mul(2)).clamp(4, 24)
}

fn adaptive_download_slots_mods() -> usize {
    cpu_count().clamp(3, 12)
}

fn adaptive_rayon_threads() -> usize {
    cpu_count().clamp(2, 16)
}

/// Pool dédié à l'extraction : utilise la moitié des cœurs pour laisser
/// du headroom à l'UI, à l'antivirus et aux autres processus.
/// Plus rapide qu'un thread unique, sans saturer le CPU.
fn extraction_pool() -> rayon::ThreadPool {
    let threads = (cpu_count() / 2).clamp(2, 6);
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap())
}

/// Builds a rayon thread pool sized for broad hardware coverage.
fn limited_pool() -> rayon::ThreadPool {
    let threads = adaptive_rayon_threads();
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap())
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
    let packages_config = [
        ("", "Core"),
        ("/Tracks", "Tracks"),
        ("/en", "Speech"),
    ];

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: "Fetching package manifests...".into(),
        current_file: 0, total_files: 0,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    // ── Phase 1: fetch manifests + parallel file verification ─────────────
    let mut packages: Vec<PackageWork> = Vec::new();
    let mut pipeline_compressed: u64 = 0;
    let mut pipeline_uncompressed: u64 = 0;
    let mut total_dl_files: u32 = 0;

    for (pkg_path, label) in &packages_config {
        let pkg_url = if pkg_path.is_empty() {
            base.clone()
        } else {
            format!("{}{}", base, pkg_path)
        };
        let index_url = format!("{}/index.xml", pkg_url);
        let resp = match client.get(&index_url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let index_xml = resp.text().await.map_err(|e| e.to_string())?;
        let (header, entries) = parse_index_xml(&index_xml)?;
        let cab_size = header.firstcab;
        let total_entries = entries.len();

        let _ = app.emit("download-progress", DownloadEvent {
            status: "verifying".into(),
            file_name: format!("[{}] Verifying {} files...", label, total_entries),
            current_file: 0, total_files: total_entries as u32,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });

        // Parallel verification via rayon — much faster than sequential
        // spawn_blocking calls on a large file list.
        let game_dir_c = game_dir.clone();
        let entries_c = entries.clone();
        let needs: Vec<bool> = tokio::task::spawn_blocking(move || {
            use rayon::prelude::*;
            entries_c.into_par_iter().map(|e| {
                let path = game_dir_c.join(strip_first_component(&e.path)).join(&e.file);
                if !path.exists() { return true; }
                match &e.hash {
                    Some(expected) => md5_base64_file(&path)
                        .map(|actual| &actual != expected)
                        .unwrap_or(true),
                    None => false,
                }
            }).collect()
        }).await.unwrap_or_else(|_| vec![true; total_entries]);

        let files_to_download: Vec<IndexFileEntry> = entries
            .into_iter()
            .zip(needs)
            .filter_map(|(e, n)| if n { Some(e) } else { None })
            .collect();

        if files_to_download.is_empty() { continue; }

        let needed_sections: std::collections::BTreeSet<u32> = files_to_download
            .iter()
            .flat_map(|e| file_sections(e, cab_size))
            .collect();

        // Stable compressed total: section count × section size, known before
        // download starts. This avoids a moving total that can make progress
        // appear to go backwards on some machines/CDN responses.
        pipeline_compressed = pipeline_compressed
            .saturating_add((needed_sections.len() as u64).saturating_mul(cab_size));
        pipeline_uncompressed += files_to_download.iter().map(|e| e.length).sum::<u64>();
        total_dl_files += files_to_download.len() as u32;

        packages.push(PackageWork { base_url: pkg_url, cab_size, files_to_download, needed_sections });
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

    // ── Phase 2: parallel section downloads ──────────────────────────────────
    // Download all sections for all packages. Extraction is a separate phase
    // that starts only after every section has been fetched.
    let global_dl = Arc::new(AtomicU64::new(0));

    // Pre-create all output directories.
    {
        let mut dirs = std::collections::HashSet::new();
        for pkg in &packages {
            for e in &pkg.files_to_download {
                let stripped = strip_first_component(&e.path);
                let dir = if stripped.is_empty() { game_dir.clone() } else { game_dir.join(stripped) };
                dirs.insert(dir);
            }
        }
        for dir in dirs { std::fs::create_dir_all(dir).ok(); }
    }

    // Download progress ticker.
    let tick_app     = app.clone();
    let tick_dl      = global_dl.clone();
    let tick_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tick_flag    = tick_running.clone();
    let tick_handle  = tokio::spawn(async move {
        let mut sw = SpeedEwma::new(0.10);
        let mut last_bytes: u64 = 0;
        let mut last_progress_at = std::time::Instant::now();
        let mut last_nonzero_speed: u64 = 0;
        while tick_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let dl    = tick_dl.load(Ordering::Relaxed);
            if dl > last_bytes {
                last_progress_at = std::time::Instant::now();
            }
            let raw_speed = sw.push(dl);
            let remaining = pipeline_compressed.saturating_sub(dl);
            let near_finish = remaining <= (pipeline_compressed / 40).max(32 * 1024 * 1024);
            let speed = if raw_speed > 0 {
                last_nonzero_speed = raw_speed;
                raw_speed
            } else if near_finish
                && dl >= last_bytes
                && last_nonzero_speed > 0
                && last_progress_at.elapsed() <= std::time::Duration::from_secs(2)
            {
                // Avoid end-of-download speed collapsing to ~0 while final tasks flush.
                last_nonzero_speed
            } else {
                raw_speed
            };
            let eta = if speed > 0 { remaining / speed } else { 0 };
            let _ = tick_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("0/{} files", total_dl_files),
                current_file: 0,
                total_files: total_dl_files,
                downloaded_bytes: dl,
                total_bytes: pipeline_compressed,
                speed, eta, error: None,
            });
            last_bytes = dl;
        }
    });

    // Semaphore shared across all packages (adaptive concurrency).
    let dl_sem = Arc::new(Semaphore::new(adaptive_download_slots_game()));

    // Collect extraction work items produced during download.
    // Held in memory until all downloads complete, then extracted in Phase 3.
    type ExtractWork = (Vec<IndexFileEntry>, HashMap<u32, Arc<Vec<u8>>>);
    let work_batches: Arc<std::sync::Mutex<Vec<ExtractWork>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));

    // ── Parallel package downloads ───────────────────────────────────────────
    let pkg_err_store: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut pkg_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    for pkg in packages {
        let client    = client.clone();
        let dl_sem    = dl_sem.clone();
        let global_dl = global_dl.clone();
        let batches   = work_batches.clone();
        let err_store = pkg_err_store.clone();

        pkg_handles.push(tokio::spawn(async move {
            let cab_size     = pkg.cab_size;
            let needed_count = pkg.needed_sections.len();
            let mut pending: Vec<IndexFileEntry> = pkg.files_to_download;

            let sections_store: Arc<std::sync::Mutex<HashMap<u32, Arc<Vec<u8>>>>> =
                Arc::new(std::sync::Mutex::new(HashMap::new()));
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Result<u32, String>>();

            for &sec_id in &pkg.needed_sections {
                let client   = client.clone();
                let url      = format!("{}/section{}.dat", pkg.base_url, sec_id);
                let sem      = dl_sem.clone();
                let store    = sections_store.clone();
                let dl_bytes = global_dl.clone();
                let tx       = tx.clone();

                tokio::spawn(async move {
                    let _permit = match sem.acquire().await {
                        Ok(p)  => p,
                        Err(e) => { tx.send(Err(e.to_string())).ok(); return; }
                    };
                    const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
                    let mut last_err = String::new();
                    for attempt in 0..3u32 {
                        if attempt > 0 {
                            tokio::time::sleep(std::time::Duration::from_millis(800 * attempt as u64)).await;
                        }
                        let resp = match client.get(&url).send().await {
                            Ok(r) if r.status().is_success() => r,
                            Ok(r)  => { last_err = format!("HTTP {}", r.status()); continue; }
                            Err(e) => { last_err = e.to_string(); continue; }
                        };
                        let mut buf: Vec<u8> = Vec::new();
                        let mut body = resp;
                        let mut ok = true;
                        loop {
                            match tokio::time::timeout(CHUNK_TIMEOUT, body.chunk()).await {
                                Err(_) => {
                                    last_err = format!("section{}.dat: stalled", sec_id);
                                    dl_bytes.fetch_sub(buf.len() as u64, Ordering::Relaxed);
                                    ok = false; break;
                                }
                                Ok(Ok(Some(chunk))) => {
                                    dl_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                                    buf.extend_from_slice(&chunk);
                                }
                                Ok(Ok(None)) => break,
                                Ok(Err(e)) => {
                                    last_err = e.to_string();
                                    dl_bytes.fetch_sub(buf.len() as u64, Ordering::Relaxed);
                                    ok = false; break;
                                }
                            }
                        }
                        if !ok { buf.clear(); continue; }
                        store.lock().unwrap().insert(sec_id, Arc::new(buf));
                        tx.send(Ok(sec_id)).ok();
                        return;
                    }
                    tx.send(Err(format!("section{}.dat failed after 3 attempts: {}", sec_id, last_err))).ok();
                });
            }
            drop(tx);

            let mut received = 0;
            while received < needed_count {
                match rx.recv().await {
                    None => break,
                    Some(Err(e)) => {
                        err_store.lock().unwrap().push(e);
                        return;
                    }
                    Some(Ok(_sec_id)) => {
                        received += 1;

                        let ready_files: Vec<IndexFileEntry> = {
                            let store = sections_store.lock().unwrap();
                            let mut ready = Vec::new();
                            let mut still = Vec::new();
                            for e in pending.drain(..) {
                                if file_sections(&e, cab_size).iter().all(|s| store.contains_key(s)) {
                                    ready.push(e);
                                } else {
                                    still.push(e);
                                }
                            }
                            pending = still;
                            ready
                        };

                        if ready_files.is_empty() { continue; }

                        let needed_secs: std::collections::BTreeSet<u32> = ready_files
                            .iter().flat_map(|e| file_sections(e, cab_size)).collect();
                        let local_secs: HashMap<u32, Arc<Vec<u8>>> = {
                            let store = sections_store.lock().unwrap();
                            needed_secs.iter()
                                .filter_map(|s| store.get(s).map(|d| (*s, Arc::clone(d))))
                                .collect()
                        };

                        // Queue for extraction after all downloads finish.
                        batches.lock().unwrap().push((ready_files, local_secs));

                        let still_needed: std::collections::BTreeSet<u32> = pending
                            .iter().flat_map(|e| file_sections(e, cab_size)).collect();
                        sections_store.lock().unwrap().retain(|s, _| still_needed.contains(s));
                    }
                }
            }
        }));
    }

    for h in pkg_handles { let _ = h.await; }
    let mut all_errors: Vec<String> = Vec::new();
    all_errors.extend(pkg_err_store.lock().unwrap().drain(..));

    tick_running.store(false, Ordering::Relaxed);
    let _ = tick_handle.await;

    if !all_errors.is_empty() {
        return Err(all_errors.join("; "));
    }

    // ── Phase 3: extraction ──────────────────────────────────────────────────
    // Flatten all batches into a single work list, then run ONE spawn_blocking
    // + ONE rayon par_iter.  No per-batch task creation, no semaphore, no pool
    // rebuilds — rayon can freely spread every file across all CPU cores.
    // Each batch's section map is wrapped in Arc so all files in that batch
    // share it with zero copies.
    let batches = std::mem::take(&mut *work_batches.lock().unwrap());
    let global_ex    = Arc::new(AtomicU64::new(0));
    let global_files = Arc::new(AtomicU32::new(0));

    let _ = app.emit("download-progress", DownloadEvent {
        status: "extracting".into(),
        file_name: format!("0/{} files", total_dl_files),
        current_file: 0,
        total_files: total_dl_files,
        downloaded_bytes: 0,
        total_bytes: pipeline_uncompressed,
        speed: 0, eta: 0, error: None,
    });

    let tick_app2     = app.clone();
    let tick_ex2      = global_ex.clone();
    let tick_files2   = global_files.clone();
    let tick_running2 = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tick_flag2    = tick_running2.clone();
    let tick_handle2  = tokio::spawn(async move {
        let mut sw_bytes = SpeedEwma::new(0.20);
        let mut sw_files = SpeedEwma::new(0.30);
        let mut hist_bytes: VecDeque<(std::time::Instant, u64)> = VecDeque::with_capacity(64);
        let mut hist_files: VecDeque<(std::time::Instant, u32)> = VecDeque::with_capacity(64);
        let mut eta_smoothed: Option<f64> = None;
        let mut last_ex: u64 = 0;
        let mut last_files_done: u32 = 0;
        let mut last_progress_at = std::time::Instant::now();
        let mut last_nonzero_speed: u64 = 0;

        fn slope_u64(samples: &VecDeque<(std::time::Instant, u64)>) -> f64 {
            if samples.len() < 3 {
                return 0.0;
            }
            let t0 = match samples.front() {
                Some((t, _)) => *t,
                None => return 0.0,
            };
            let n = samples.len() as f64;
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut sum_xx = 0.0;
            let mut sum_xy = 0.0;
            for (t, y) in samples {
                let x = t.duration_since(t0).as_secs_f64();
                let yf = *y as f64;
                sum_x += x;
                sum_y += yf;
                sum_xx += x * x;
                sum_xy += x * yf;
            }
            let denom = n * sum_xx - sum_x * sum_x;
            if denom <= 1e-9 {
                return 0.0;
            }
            ((n * sum_xy - sum_x * sum_y) / denom).max(0.0)
        }

        fn slope_u32(samples: &VecDeque<(std::time::Instant, u32)>) -> f64 {
            if samples.len() < 3 {
                return 0.0;
            }
            let t0 = match samples.front() {
                Some((t, _)) => *t,
                None => return 0.0,
            };
            let n = samples.len() as f64;
            let mut sum_x = 0.0;
            let mut sum_y = 0.0;
            let mut sum_xx = 0.0;
            let mut sum_xy = 0.0;
            for (t, y) in samples {
                let x = t.duration_since(t0).as_secs_f64();
                let yf = *y as f64;
                sum_x += x;
                sum_y += yf;
                sum_xx += x * x;
                sum_xy += x * yf;
            }
            let denom = n * sum_xx - sum_x * sum_x;
            if denom <= 1e-9 {
                return 0.0;
            }
            ((n * sum_xy - sum_x * sum_y) / denom).max(0.0)
        }

        while tick_flag2.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let now = std::time::Instant::now();
            let ex         = tick_ex2.load(Ordering::Relaxed);
            let files_done = tick_files2.load(Ordering::Relaxed);

            hist_bytes.push_back((now, ex));
            hist_files.push_back((now, files_done));
            let max_window = std::time::Duration::from_secs(8);
            while let Some((t, _)) = hist_bytes.front().copied() {
                if now.duration_since(t) > max_window {
                    hist_bytes.pop_front();
                } else {
                    break;
                }
            }
            while let Some((t, _)) = hist_files.front().copied() {
                if now.duration_since(t) > max_window {
                    hist_files.pop_front();
                } else {
                    break;
                }
            }

            let byte_speed = sw_bytes.push(ex);
            let files_per_sec = sw_files.push(files_done as u64) as f64;
            let byte_speed_lr = slope_u64(&hist_bytes);
            let files_per_sec_lr = slope_u32(&hist_files);

            let byte_conf = ((hist_bytes.len().saturating_sub(2) as f64) / 10.0).clamp(0.0, 1.0);
            let file_conf = ((hist_files.len().saturating_sub(2) as f64) / 10.0).clamp(0.0, 1.0);

            let byte_speed_blend = if byte_speed_lr > 0.0 {
                (0.75 * byte_speed_lr + 0.25 * byte_speed as f64).max(0.0)
            } else {
                byte_speed as f64
            };
            let files_per_sec_blend = if files_per_sec_lr > 0.0 {
                (0.75 * files_per_sec_lr + 0.25 * files_per_sec).max(0.0)
            } else {
                files_per_sec.max(0.0)
            };
            let avg_bytes_per_file = if files_done > 0 {
                ex as f64 / files_done as f64
            } else {
                0.0
            };

            let fallback_speed = if byte_speed_blend <= 1.0 && files_per_sec_blend > 0.0 && avg_bytes_per_file > 0.0 {
                (files_per_sec_blend * avg_bytes_per_file) as u64
            } else {
                0
            };
            let mut effective_speed = (byte_speed_blend as u64).max(fallback_speed);

            let estimated_total_from_files = if files_done > 0 {
                (avg_bytes_per_file * total_dl_files as f64) as u64
            } else {
                0
            };
            let estimated_total = pipeline_uncompressed
                .max(estimated_total_from_files)
                .max(ex);

            if ex > last_ex || files_done > last_files_done {
                last_progress_at = now;
            }
            let near_finish =
                estimated_total.saturating_sub(ex) <= (pipeline_uncompressed / 40).max(32 * 1024 * 1024)
                || total_dl_files.saturating_sub(files_done) <= 3;
            if effective_speed > 0 {
                last_nonzero_speed = effective_speed;
            } else if near_finish
                && last_nonzero_speed > 0
                && last_progress_at.elapsed() <= std::time::Duration::from_secs(2)
            {
                // Near completion, extraction can briefly pause while final writes/join happen.
                // Keep displaying the last observed speed to avoid a fake "stopped then resumed" effect.
                effective_speed = last_nonzero_speed;
            }

            let eta_bytes = if effective_speed > 0 && estimated_total > ex {
                (estimated_total - ex) as f64 / effective_speed as f64
            } else {
                0.0
            };
            let eta_files = if files_per_sec_blend > 0.0 && total_dl_files > files_done {
                (total_dl_files - files_done) as f64 / files_per_sec_blend
            } else {
                0.0
            };
            let eta_raw = match (eta_bytes > 0.0, eta_files > 0.0) {
                (false, false) => 0.0,
                (true, false) => eta_bytes,
                (false, true) => eta_files,
                (true, true) => {
                    let wb = 0.60 + 0.40 * byte_conf;
                    let wf = 0.40 + 0.40 * file_conf;
                    (eta_bytes * wb + eta_files * wf) / (wb + wf)
                }
            };

            let eta = if eta_raw <= 0.0 {
                eta_smoothed = None;
                0
            } else {
                let prev = eta_smoothed.unwrap_or(eta_raw);
                let near_end = estimated_total.saturating_sub(ex) < 32 * 1024 * 1024;
                let alpha = if near_end {
                    0.55
                } else if eta_raw < prev {
                    0.40
                } else {
                    0.22
                };
                let mut smoothed = alpha * eta_raw + (1.0 - alpha) * prev;
                let max_up = prev * 1.15 + 1.0;
                if smoothed > max_up {
                    smoothed = max_up;
                }
                eta_smoothed = Some(smoothed);
                smoothed.round() as u64
            };

            let _ = tick_app2.emit("download-progress", DownloadEvent {
                status: "extracting".into(),
                file_name: format!("{}/{} files", files_done, total_dl_files),
                current_file: files_done,
                total_files: total_dl_files,
                downloaded_bytes: ex,
                total_bytes: pipeline_uncompressed,
                speed: effective_speed,
                eta,
                error: None,
            });
            last_ex = ex;
            last_files_done = files_done;
        }
    });

    // Build the flat work list (one entry per file, shared section map via Arc).
    type FlatItem = (IndexFileEntry, Arc<HashMap<u32, Arc<Vec<u8>>>>);
    let mut flat: Vec<FlatItem> = Vec::with_capacity(total_dl_files as usize);
    for (files, secs) in batches {
        let secs = Arc::new(secs);
        for entry in files {
            flat.push((entry, secs.clone()));
        }
    }

    let gd   = game_dir.clone();
    let ex_b = global_ex.clone();
    let ex_f = global_files.clone();
    let extract_errors: Vec<String> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        use std::io::BufWriter;
        // extraction_pool() utilise ~moitié des cœurs: rapide mais non-saturant.
        extraction_pool().install(|| {
            flat.into_par_iter().filter_map(|(entry, secs)| {
                let raw = match extract_raw_arc(&entry, &secs) {
                    Ok(r)  => r,
                    Err(e) => return Some(e),
                };
                let data: Vec<u8> = if is_lzma(&raw) {
                    let mut out = Vec::with_capacity(entry.length as usize);
                    let mut reader = Cursor::new(&raw[..]);
                    match lzma_decompress(&mut reader, &mut out) {
                        Ok(_) => out,
                        Err(_) => raw,
                    }
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
                // BufWriter évite les allers-retours syscall pour les fichiers
                // de petite taille qui arrivent par morceaux.
                let result = (|| -> std::io::Result<()> {
                    let f = std::fs::File::create(&dest)?;
                    let mut w = BufWriter::with_capacity(256 * 1024, f);
                    std::io::Write::write_all(&mut w, &data)?;
                    w.flush()
                })();
                if let Err(e) = result {
                    return Some(format!("write {}: {}", entry.file, e));
                }
                ex_b.fetch_add(written, Ordering::Relaxed);
                ex_f.fetch_add(1, Ordering::Relaxed);
                None
            }).collect()
        })
    }).await.unwrap_or_else(|e| vec![e.to_string()]);

    all_errors.extend(extract_errors);

    tick_running2.store(false, Ordering::Relaxed);
    let _ = tick_handle2.await;

    if !all_errors.is_empty() {
        return Err(all_errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Download complete".into(),
        current_file: total_dl_files, total_files: total_dl_files,
        downloaded_bytes: pipeline_uncompressed, total_bytes: pipeline_uncompressed,
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

    // Parse all (expected_hash, relative_path) pairs first.
    let entries: Vec<(String, String)> = checksums_text
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.len() < 2 { return None; }
            let hash = parts[0].trim().to_uppercase();
            let rel = parts[1]
                .trim_start_matches('/')
                .trim_start_matches('\\')
                .replace('/', std::path::MAIN_SEPARATOR_STR)
                .replace('\\', std::path::MAIN_SEPARATOR_STR);
            Some((hash, rel))
        })
        .collect();

    let total = entries.len() as u32;

    // Shared atomic counters so the progress ticker can read them at any time.
    let scanned_ctr  = Arc::new(AtomicU32::new(0));
    let corrupt_ctr  = Arc::new(AtomicU32::new(0));

    // Progress ticker: emits every 200 ms — avoids flooding the IPC channel
    // while still giving smooth updates.
    let tick_app     = app.clone();
    let tick_scanned = scanned_ctr.clone();
    let tick_corrupt = corrupt_ctr.clone();
    let label_str    = package_label.to_string();
    let tick_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tick_flag    = tick_running.clone();
    let tick_handle  = tokio::spawn(async move {
        while tick_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done  = tick_scanned.load(Ordering::Relaxed);
            let bad   = tick_corrupt.load(Ordering::Relaxed);
            let _ = tick_app.emit("verify-progress", VerifyEvent {
                status: "scanning".into(),
                current_file: format!("[{}] {}/{}", label_str, done, total),
                current_index: done,
                total_files: total,
                corrupted_count: bad,
            });
        }
    });

    // Run all SHA-1 checks in parallel via rayon.
    let game_dir_c = game_dir.to_path_buf();
    let sc = scanned_ctr.clone();
    let cc = corrupt_ctr.clone();
    let corrupted: Vec<String> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
            entries.into_par_iter().filter_map(|(expected, rel)| {
                let full = game_dir_c.join(&rel);
                let is_bad = if !full.exists() {
                    true
                } else {
                    match sha1_file(&full) {
                        Ok(actual) => actual.trim().to_uppercase() != expected,
                        Err(_) => true,
                    }
                };
                sc.fetch_add(1, Ordering::Relaxed);
                if is_bad {
                    cc.fetch_add(1, Ordering::Relaxed);
                    Some(rel)
                } else {
                    None
                }
            }).collect()
        })
    }).await.unwrap_or_default();

    tick_running.store(false, Ordering::Relaxed);
    let _ = tick_handle.await;

    // Final accurate event.
    let _ = app.emit("verify-progress", VerifyEvent {
        status: "scanning".into(),
        current_file: format!("[{}] {}/{}", package_label, total, total),
        current_index: total,
        total_files: total,
        corrupted_count: corrupted.len() as u32,
    });

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
    let mut buffer = [0u8; 262144];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Fire HEAD requests for all URLs in parallel and sum their Content-Length headers.
/// Fast (no body), used to know the total download size before streaming starts.
async fn head_total_bytes(client: &reqwest::Client, urls: &[String]) -> u64 {
    // 10 s timeout per HEAD so a slow CDN never blocks the download start.
    let handles: Vec<_> = urls.iter().map(|url| {
        let c = client.clone();
        let u = url.clone();
        tokio::spawn(async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                c.head(&u).send()
            ).await.ok()
                .and_then(|r| r.ok())
                .and_then(|r| r.content_length())
                .unwrap_or(0)
        })
    }).collect();
    let mut total = 0u64;
    for h in handles {
        total += h.await.unwrap_or(0);
    }
    total
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

    let verified_count = Arc::new(AtomicU32::new(0));
    let verify_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let vr_flag = verify_running.clone();
    let vr_count = verified_count.clone();
    let vr_app = app.clone();
    let verify_tick = tokio::spawn(async move {
        let frames = ["", ".", "..", "..."];
        let mut i = 0usize;
        while vr_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(220)).await;
            let done = vr_count.load(Ordering::Relaxed).min(total);
            let _ = vr_app.emit("download-progress", DownloadEvent {
                status: "verifying".into(),
                file_name: format!("Checking ModNet modules{}", frames[i % frames.len()]),
                current_file: done,
                total_files: total,
                downloaded_bytes: 0,
                total_bytes: 0,
                speed: 0,
                eta: 0,
                error: None,
            });
            i = i.wrapping_add(1);
        }
    });

    // Parallel SHA-256 check via rayon.
    let game_dir_c = game_dir.clone();
    let modules_vec: Vec<(String, String)> = modules.into_iter().collect();
    let modules_len = modules_vec.len();
    let modules_vec_c = modules_vec.clone();
    let verified_count_c = verified_count.clone();
    let needs_vec: Vec<bool> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
            modules_vec_c.par_iter().map(|(name, expected)| {
                let path = game_dir_c.join(name);
                let needs = if path.exists() {
                    sha256_hex_file(&path)
                        .map(|actual| !actual.eq_ignore_ascii_case(expected))
                        .unwrap_or(true)
                } else {
                    true
                };
                verified_count_c.fetch_add(1, Ordering::Relaxed);
                needs
            }).collect()
        })
    }).await.unwrap_or_else(|_| vec![true; modules_len]);
    verify_running.store(false, Ordering::Relaxed);
    let _ = verify_tick.await;

    let dlls_to_download: Vec<String> = modules_vec.into_iter()
        .zip(needs_vec)
        .filter_map(|((name, _), needs)| if needs { Some(name) } else { None })
        .collect();

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: format!("Checked {}/{} modules", total, total),
        current_file: total, total_files: total,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

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

    // Resolve total bytes upfront via HEAD requests so the progress bar percentage
    // is correct from the very first chunk (total_bytes never changes mid-download).
    let head_urls: Vec<String> = dlls_to_download.iter()
        .map(|n| format!("{}/launcher-modules/{}", modnet_cdn.trim_end_matches('/'), n))
        .collect();
    let known_total = head_total_bytes(&client, &head_urls).await;

    // Streaming download with per-chunk progress, bounded concurrency and per-file retries.
    let total_modnet_bytes = Arc::new(AtomicU64::new(known_total));
    let dl_sem = Arc::new(Semaphore::new(adaptive_download_slots_mods()));
    let dl_done = Arc::new(AtomicU32::new(0));
    let dl_bytes = Arc::new(AtomicU64::new(0));

    let progress_app = app.clone();
    let p_done = dl_done.clone();
    let p_bytes = dl_bytes.clone();
    let p_total = total_modnet_bytes.clone();
    let progress_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag = progress_running.clone();
    let progress_handle = tokio::spawn(async move {
        let mut sw = SpeedEwma::new(0.10);
        let mut last_bytes: u64 = 0;
        let mut last_progress_at = std::time::Instant::now();
        let mut last_nonzero_speed: u64 = 0;
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            if bytes > last_bytes {
                last_progress_at = std::time::Instant::now();
            }
            let raw_speed = sw.push(bytes);
            // When HEAD gave no size info (known_total == 0), emit total_bytes: 0
            // total_bytes=0 triggers the file-count % fallback in the UI;
            // downloaded_bytes always carries the real value so the size display works.
            let total_known = p_total.load(Ordering::Relaxed);
            let estimated_total = if total_known > 0 {
                total_known.max(bytes)
            } else if done > 0 {
                // HEAD gave no size — estimate total from avg bytes/file so far.
                (bytes / done as u64) * dl_total as u64
            } else {
                0
            };
            let emit_total = if total_known > 0 { estimated_total } else { 0 };
            let near_finish = estimated_total.saturating_sub(bytes)
                <= (estimated_total / 40).max(16 * 1024 * 1024);
            let speed = if raw_speed > 0 {
                last_nonzero_speed = raw_speed;
                raw_speed
            } else if near_finish
                && last_nonzero_speed > 0
                && last_progress_at.elapsed() <= std::time::Duration::from_secs(2)
            {
                last_nonzero_speed
            } else {
                raw_speed
            };
            let eta = if speed > 0 && estimated_total > bytes {
                ((estimated_total - bytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("[ModNet] {}/{}", done, dl_total),
                current_file: done, total_files: dl_total,
                downloaded_bytes: bytes, total_bytes: emit_total,
                speed, eta, error: None,
            });
            last_bytes = bytes;
        }
    });

    let mut dl_handles = Vec::new();
    for name in dlls_to_download {
        let sem = dl_sem.clone();
        let done_ctr = dl_done.clone();
        let bytes_ctr = dl_bytes.clone();
        let gd = game_dir.clone();
        let c = client.clone();
        let url = format!("{}/launcher-modules/{}", modnet_cdn.trim_end_matches('/'), &name);

        dl_handles.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
            let mut last_err = String::new();
            const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
            for attempt in 0..3u32 {
                if attempt > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64)).await;
                }
                let resp = match c.get(&url).send().await {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => { last_err = format!("HTTP {}", r.status()); continue; }
                    Err(e) => { last_err = e.to_string(); continue; }
                };
                let dll_path = gd.join(&name);
                let tmp_path = gd.join(format!(".{}.tmp", &name));
                let mut file = match tokio::fs::File::create(&tmp_path).await {
                    Ok(f) => f,
                    Err(e) => { last_err = e.to_string(); continue; }
                };
                let mut chunk_ok = true;
                let mut bytes_this_attempt: u64 = 0;
                let mut resp_body = resp;
                loop {
                    match tokio::time::timeout(CHUNK_TIMEOUT, resp_body.chunk()).await {
                        Err(_) => {
                            last_err = format!("ModNet {}: stalled", name);
                            bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                            chunk_ok = false;
                            break;
                        }
                        Ok(Ok(Some(chunk))) => {
                            if let Err(e) = file.write_all(&chunk).await {
                                last_err = e.to_string();
                                bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                                chunk_ok = false;
                                break;
                            }
                            let n = chunk.len() as u64;
                            bytes_ctr.fetch_add(n, Ordering::Relaxed);
                            bytes_this_attempt += n;
                        }
                        Ok(Ok(None)) => break,
                        Ok(Err(e)) => {
                            last_err = e.to_string();
                            bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                            chunk_ok = false;
                            break;
                        }
                    }
                }
                drop(file);
                if !chunk_ok {
                    tokio::fs::remove_file(&tmp_path).await.ok();
                    continue;
                }
                if let Err(e) = tokio::fs::rename(&tmp_path, &dll_path).await {
                    last_err = e.to_string();
                    continue;
                }
                done_ctr.fetch_add(1, Ordering::Relaxed);
                return Ok::<(), String>(());
            }
            Err(format!("{} failed after 3 attempts: {}", name, last_err))
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

    // Guarantee 100% — ticker may have sampled just before the last byte landed.
    let final_bytes = dl_bytes.load(Ordering::Relaxed);
    let _ = app.emit("download-progress", DownloadEvent {
        status: "downloading".into(),
        file_name: format!("[ModNet] {}/{}", dl_total, dl_total),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
        speed: 0, eta: 0, error: None,
    });

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "ModNet modules installed".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
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

/// Convert Windows-style backslash separators in a mod entry name to the
/// native path separator. Mod manifests are produced by Windows game servers
/// and frequently use `\` as directory separators. On Linux `\` is a valid
/// filename character, NOT a separator, which would otherwise cause all mod
/// files to be stored flat with backslashes in their names instead of in the
/// correct subdirectory tree — breaking the game's ModManager entirely.
fn normalize_entry_name(name: &str) -> String {
    #[cfg(not(windows))]
    { name.replace('\\', "/") }
    #[cfg(windows)]
    { name.to_owned() }
}

fn scripts_snapshot_key(game_dir: &Path) -> String {
    #[cfg(windows)]
    {
        game_dir
            .to_string_lossy()
            .replace('/', "\\")
            .to_lowercase()
    }
    #[cfg(not(windows))]
    {
        game_dir.to_string_lossy().to_string()
    }
}

fn scripts_snapshot_store() -> &'static Mutex<HashMap<String, HashSet<String>>> {
    static STORE: OnceLock<Mutex<HashMap<String, HashSet<String>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn snapshot_scripts_baseline(game_dir: &Path) -> Result<(), String> {
    let scripts_dir = game_dir.join("scripts");
    let known_before: HashSet<String> = if scripts_dir.exists() {
        std::fs::read_dir(&scripts_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect()
    } else {
        HashSet::new()
    };

    let key = scripts_snapshot_key(game_dir);
    let mut store = scripts_snapshot_store()
        .lock()
        .map_err(|e| format!("scripts snapshot lock poisoned: {}", e))?;
    store.insert(key, known_before);
    Ok(())
}

fn get_scripts_baseline(game_dir: &Path) -> Option<HashSet<String>> {
    let key = scripts_snapshot_key(game_dir);
    scripts_snapshot_store()
        .lock()
        .ok()
        .and_then(|store| store.get(&key).cloned())
}

fn clear_scripts_baseline(game_dir: &Path) {
    if let Ok(mut store) = scripts_snapshot_store().lock() {
        let key = scripts_snapshot_key(game_dir);
        store.remove(&key);
    }
}

fn sha1_hex_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 262144];
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Returns the number of hard links pointing to the same inode as `path`.
/// A value >= 2 means at least one other directory entry (e.g. in `.data/<hash>/`)
/// still references the same file — i.e. ModManager's hardlink is still in place.
/// Returns 1 on any error (safe default: assume the file is not a hardlink).
fn get_hardlink_count(path: &Path) -> u32 {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
        };
        let Ok(file) = std::fs::File::open(path) else { return 1 };
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
        if ok == 0 { return 1 }
        info.nNumberOfLinks
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path).map(|m| m.nlink() as u32).unwrap_or(1)
    }
}

fn remove_path_safely(path: &Path) {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return };

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        let is_rp = (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0;

        if is_rp {
            // On Windows, Rust's is_dir() returns false for both junctions AND
            // directory symlinks (they are reparse points, not "real" directories
            // from Rust's perspective). Always try remove_dir first (handles
            // junctions + directory symlinks), fall back to remove_file (file symlinks).
            if std::fs::remove_dir(path).is_err() {
                std::fs::remove_file(path).ok();
            }
        } else if meta.is_dir() {
            std::fs::remove_dir_all(path).ok();
        } else {
            std::fs::remove_file(path).ok();
        }

        return;
    }

    #[cfg(not(windows))]
    {
        if meta.is_dir() {
            if std::fs::remove_dir(path).is_err() {
                std::fs::remove_dir_all(path).ok();
            }
        } else {
            std::fs::remove_file(path).ok();
        }
    }
}

fn is_reparse_point(meta: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        return (meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0;
    }

    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn prune_runtime_cache_reparse_points(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };

        if meta.is_dir() {
            if is_reparse_point(&meta) {
                remove_path_safely(&path);
                continue;
            }
            prune_runtime_cache_reparse_points(&path);
        }
    }
}

#[command]
pub fn has_pending_mod_cleanup(game_path: String) -> Result<bool, String> {
    let game_dir = PathBuf::from(&game_path);
    if !game_dir.exists() {
        return Ok(false);
    }

    let modules_dir = game_dir.join("modules");
    let links_file = game_dir.join(".links");
    let artifacts = ["lightfx.dll", "ModManager.dat", "PocoFoundation.dll"];

    if links_file.exists() || modules_dir.exists() {
        return Ok(true);
    }

    if artifacts.iter().any(|artifact| game_dir.join(artifact).exists()) {
        return Ok(true);
    }

    Ok(false)
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
    let data_dir = game_dir.join(".data").join(&server_hash);
    std::fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let total_mods = entries.len() as u32;

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: "Verifying mods...".into(),
        current_file: 0, total_files: total_mods,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    let verified_count = Arc::new(AtomicU32::new(0));
    let verify_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let vr_flag = verify_running.clone();
    let vr_count = verified_count.clone();
    let vr_app = app.clone();
    let verify_tick = tokio::spawn(async move {
        let frames = ["", ".", "..", "..."];
        let mut i = 0usize;
        while vr_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(220)).await;
            let done = vr_count.load(Ordering::Relaxed).min(total_mods);
            let _ = vr_app.emit("download-progress", DownloadEvent {
                status: "verifying".into(),
                file_name: format!("Verifying mods{}", frames[i % frames.len()]),
                current_file: done,
                total_files: total_mods,
                downloaded_bytes: 0,
                total_bytes: 0,
                speed: 0,
                eta: 0,
                error: None,
            });
            i = i.wrapping_add(1);
        }
    });

    let entries_vec: Vec<(String, Option<String>)> = entries.iter()
        .map(|e| (normalize_entry_name(&e.name.clone().unwrap_or_default()), e.checksum.clone()))
        .collect();

    // Parallel verification via rayon (same pattern as download_game verify phase).
    let cache_dir_c = cache_dir.clone();
    let verified_count_c = verified_count.clone();
    let needs_vec: Vec<(bool, bool)> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
            entries_vec.into_par_iter().map(|(name, expected)| {
                let cached_file = cache_dir_c.join(&name);
                if !cached_file.exists() {
                    verified_count_c.fetch_add(1, Ordering::Relaxed);
                    return (true, false);
                }
                let out = match expected {
                    Some(exp) => {
                        let mismatch = sha1_hex_file(&cached_file)
                            .map(|actual| !actual.eq_ignore_ascii_case(&exp))
                            .unwrap_or(true);
                        (mismatch, mismatch)
                    }
                    None => (false, false),
                };
                verified_count_c.fetch_add(1, Ordering::Relaxed);
                out
            }).collect()
        })
    }).await.unwrap_or_else(|_| vec![(true, false); entries.len()]);
    verify_running.store(false, Ordering::Relaxed);
    let _ = verify_tick.await;

    let mut mods_to_download: Vec<(String, String)> = Vec::new(); // (original_name, local_name)
    let mut any_mismatch = false;
    for (i, (needs_dl, is_mismatch)) in needs_vec.into_iter().enumerate() {
        if needs_dl {
            let original = entries[i].name.clone().unwrap_or_default();
            let local = normalize_entry_name(&original);
            mods_to_download.push((original, local));
        }
        if is_mismatch { any_mismatch = true; }
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "verifying".into(),
        file_name: format!("Verified {}/{} mods", total_mods, total_mods),
        current_file: total_mods, total_files: total_mods,
        downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
    });

    // If the server's mod set changed, clear the extracted cache now so the
    // expensive directory removal does not happen after the download phase.
    if any_mismatch && data_dir.exists() {
        let _ = app.emit("download-progress", DownloadEvent {
            status: "finalizing".into(),
            file_name: "Preparing updated mods...".into(),
            current_file: total_mods,
            total_files: total_mods,
            downloaded_bytes: 0,
            total_bytes: 0,
            speed: 0,
            eta: 0,
            error: None,
        });
        std::fs::remove_dir_all(&data_dir).ok();
    }

    // Delete stale cache files that are no longer in the new index.
    // This mirrors the reference launcher's "File.Delete(file)" for entries not in json3.
    if cache_dir.exists() {
        let expected_names: std::collections::HashSet<String> = entries.iter()
            .filter_map(|e| e.name.as_deref())
            .map(normalize_entry_name)
            .collect();
        if let Ok(dir_iter) = std::fs::read_dir(&cache_dir) {
            for entry in dir_iter.flatten() {
                let p = entry.path();
                if !p.is_file() { continue; }
                let name = p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if !expected_names.contains(&name) {
                    std::fs::remove_file(&p).ok();
                }
            }
        }
    }

    if mods_to_download.is_empty() {
        let _ = app.emit("download-progress", DownloadEvent {
            status: "completed".into(),
            file_name: "Mods ready".into(),
            current_file: total_mods, total_files: total_mods,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });
        return Ok(());
    }

    let dl_total = mods_to_download.len() as u32;

    // Resolve total bytes upfront via HEAD requests so the progress bar percentage
    // is correct from the very first chunk (total_bytes never changes mid-download).
    // Use the ORIGINAL names for URLs (CDN may serve files using Windows-style paths).
    let head_urls: Vec<String> = mods_to_download.iter()
        .map(|(orig, _)| format!("{}/{}", base_path.trim_end_matches('/'), orig))
        .collect();
    let known_total = head_total_bytes(&client, &head_urls).await;

    // Streaming download with per-chunk progress, bounded concurrency and per-file retries.
    let total_mods_bytes = Arc::new(AtomicU64::new(known_total));
    let dl_sem = Arc::new(Semaphore::new(adaptive_download_slots_mods()));
    let dl_done = Arc::new(AtomicU32::new(0));
    let dl_bytes = Arc::new(AtomicU64::new(0));

    let progress_app = app.clone();
    let p_done = dl_done.clone();
    let p_bytes = dl_bytes.clone();
    let p_total = total_mods_bytes.clone();
    let progress_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let p_flag = progress_running.clone();
    let progress_handle = tokio::spawn(async move {
        let mut sw = SpeedEwma::new(0.10);
        let mut last_bytes: u64 = 0;
        let mut last_progress_at = std::time::Instant::now();
        let mut last_nonzero_speed: u64 = 0;
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            if bytes > last_bytes {
                last_progress_at = std::time::Instant::now();
            }
            let raw_speed = sw.push(bytes);
            // Same guard as ModNet: total_bytes=0 → file-count fallback for %; real bytes always shown.
            let total_known = p_total.load(Ordering::Relaxed);
            let estimated_total = if total_known > 0 {
                total_known.max(bytes)
            } else if done > 0 {
                // HEAD gave no size — estimate total from avg bytes/file so far.
                (bytes / done as u64) * dl_total as u64
            } else {
                0
            };
            let emit_total = if total_known > 0 { estimated_total } else { 0 };
            let near_finish = estimated_total.saturating_sub(bytes)
                <= (estimated_total / 40).max(16 * 1024 * 1024);
            let speed = if raw_speed > 0 {
                last_nonzero_speed = raw_speed;
                raw_speed
            } else if near_finish
                && last_nonzero_speed > 0
                && last_progress_at.elapsed() <= std::time::Duration::from_secs(2)
            {
                last_nonzero_speed
            } else {
                raw_speed
            };
            let eta = if speed > 0 && estimated_total > bytes {
                ((estimated_total - bytes) as f64 / speed as f64) as u64
            } else {
                0
            };
            let _ = progress_app.emit("download-progress", DownloadEvent {
                status: "downloading".into(),
                file_name: format!("[MODS] {}/{}", done, dl_total),
                current_file: done, total_files: dl_total,
                downloaded_bytes: bytes, total_bytes: emit_total,
                speed, eta, error: None,
            });
            last_bytes = bytes;
        }
    });

    let mut dl_handles = Vec::new();
    for (original_name, local_name) in mods_to_download {
        let sem = dl_sem.clone();
        let done_ctr = dl_done.clone();
        let bytes_ctr = dl_bytes.clone();
        let cache = cache_dir.clone();
        let c = client.clone();
        // Use the original (possibly backslash-containing) name for the CDN URL so
        // the request matches exactly what the server expects. Use the normalized
        // local_name for all filesystem operations so paths are valid on Linux.
        let url = format!("{}/{}", base_path.trim_end_matches('/'), &original_name);

        dl_handles.push(tokio::spawn(async move {
            let name = local_name; // local filesystem name (forward slashes)
            let _permit = sem.acquire().await.map_err(|e| e.to_string())?;
            let mut last_err = String::new();
            const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
            for attempt in 0..3u32 {
                if attempt > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64)).await;
                }
                let resp = match c.get(&url).send().await {
                    Ok(r) if r.status().is_success() => r,
                    Ok(r) => { last_err = format!("HTTP {}", r.status()); continue; }
                    Err(e) => { last_err = e.to_string(); continue; }
                };
                let cached_file = cache.join(&name);
                let file_parent = cached_file.parent().unwrap_or(cache.as_path());
                std::fs::create_dir_all(file_parent).ok();
                // Place the temp file in the SAME directory as the final file so
                // that rename() is always an atomic same-directory operation.
                let tmp_name = format!(".{}.tmp",
                    cached_file.file_name().unwrap_or_default().to_string_lossy());
                let tmp_path = file_parent.join(tmp_name);
                let mut file = match tokio::fs::File::create(&tmp_path).await {
                    Ok(f) => f,
                    Err(e) => { last_err = e.to_string(); continue; }
                };
                let mut chunk_ok = true;
                let mut bytes_this_attempt: u64 = 0;
                let mut resp_body = resp;
                loop {
                    match tokio::time::timeout(CHUNK_TIMEOUT, resp_body.chunk()).await {
                        Err(_) => {
                            last_err = format!("mod {}: stalled (no data for 30s)", name);
                            bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                            chunk_ok = false;
                            break;
                        }
                        Ok(Ok(Some(chunk))) => {
                            if let Err(e) = file.write_all(&chunk).await {
                                last_err = e.to_string();
                                bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                                chunk_ok = false;
                                break;
                            }
                            let n = chunk.len() as u64;
                            bytes_ctr.fetch_add(n, Ordering::Relaxed);
                            bytes_this_attempt += n;
                        }
                        Ok(Ok(None)) => break,
                        Ok(Err(e)) => {
                            last_err = e.to_string();
                            bytes_ctr.fetch_sub(bytes_this_attempt, Ordering::Relaxed);
                            chunk_ok = false;
                            break;
                        }
                    }
                }
                drop(file);
                if !chunk_ok {
                    tokio::fs::remove_file(&tmp_path).await.ok();
                    continue;
                }
                if let Err(e) = tokio::fs::rename(&tmp_path, &cached_file).await {
                    last_err = e.to_string();
                    continue;
                }
                done_ctr.fetch_add(1, Ordering::Relaxed);
                return Ok::<(), String>(());
            }
            Err(format!("mod {} failed after 3 attempts: {}", name, last_err))
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

    // Guarantee 100% — ticker may have sampled just before the last byte landed.
    let final_bytes = dl_bytes.load(Ordering::Relaxed);
    let _ = app.emit("download-progress", DownloadEvent {
        status: "downloading".into(),
        file_name: format!("[MODS] {}/{}", dl_total, dl_total),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
        speed: 0, eta: 0, error: None,
    });

    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Mods downloaded".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
        speed: 0, eta: 0, error: None,
    });

    Ok(())
}


fn clean_mods_internal(game_dir: &Path) -> Result<bool, String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use rayon::prelude::*;

    let links_file = game_dir.join(".links");
    let mods_dir = game_dir.join("MODS");
    let data_root_dir = game_dir.join(".data");
    let log_path = game_dir.join(".cleanup_log");

    if links_file.exists() {
        let content = std::fs::read_to_string(&links_file)
            .map_err(|e| format!("read .links: {}", e))?;

        // Parse entries (sequential, fast string ops).
        // Format: "<path>\t<type>" where type 0 = file replacement, 1 = directory junction.
        // Legacy single-field format (no tab) is also accepted: type is inferred from path.
        let entries: Vec<(u8, PathBuf)> = content.lines().filter_map(|line| {
            let mut parts = line.splitn(2, '\t');
            let loc = parts.next()?.trim();
            if loc.is_empty() { return None; }

            let entry_type: u8 = match parts.next() {
                Some(t) => t.trim().parse().ok()?,
                // Legacy: no type field — infer from whether the path looks like a directory path
                // (no file extension) or a file (has extension).
                None => if Path::new(loc).extension().is_none() { 1 } else { 0 },
            };

            let real_loc = if Path::new(loc).is_absolute() {
                PathBuf::from(loc)
            } else {
                game_dir.join(loc)
            };
            // Skip paths inside the mod cache directories.
            if real_loc.starts_with(&mods_dir) || real_loc.starts_with(&data_root_dir) {
                return None;
            }
            Some((entry_type, real_loc))
        }).collect();

        let has_recoverable = AtomicBool::new(false);

        // Process all entries in parallel.  Each entry touches a distinct path so
        // there are no cross-entry dependencies.
        // Returns per-entry log lines.
        let all_log: Vec<String> = {
            let mut header = vec![format!(
                "=== clean_mods_internal start, {} entries ===", entries.len()
            )];
            let per_entry: Vec<Vec<String>> = entries.par_iter().map(|(entry_type, real_loc)| {
                let mut log = Vec::new();

                if *entry_type == 0 {
                    // ── type 0: file replacement ──────────────────────────────────────────────
                    //
                    // On Windows, std::fs::rename calls MoveFileExW(MOVEFILE_REPLACE_EXISTING),
                    // which atomically replaces the destination in a single kernel call.
                    // This means we do NOT need a separate remove_file step: if .orig exists we
                    // just rename .orig → path, whether path currently exists (mod hardlink/copy)
                    // or not (already cleaned up) — both cases are handled correctly.

                    let orig_path = {
                        let mut s = real_loc.as_os_str().to_os_string();
                        s.push(".orig");
                        PathBuf::from(s)
                    };

                    if orig_path.is_file() {
                        // Primary path: atomic restore.
                        log.push(format!("  [F] RESTORE {} -> {}", orig_path.display(), real_loc.display()));
                        match std::fs::rename(&orig_path, real_loc) {
                            Ok(()) => log.push("    OK".into()),
                            Err(e) if e.raw_os_error() == Some(5) => {
                                // Access denied — AV or the game process still has file open.
                                // Keep .links → retry loop will call us again in 500ms.
                                has_recoverable.store(true, Ordering::Relaxed);
                                log.push(format!("    PENDING (access denied, will retry): {}", e));
                            }
                            Err(e) => {
                                // .orig vanished between check and rename, or other transient error.
                                // If the target already exists, ModManager restored it concurrently.
                                if real_loc.is_file() {
                                    log.push("    SKIP: target already present (concurrent restore)".into());
                                } else {
                                    log.push(format!("    WARN: rename failed: {}", e));
                                }
                            }
                        }
                    } else {
                        // No .orig.  Two cases:
                        //   nlinks >= 2 → path is still the mod hardlink (no .orig was written,
                        //                  e.g. ModManager crashed) → remove it.
                        //   nlinks == 1 → either already restored by a prior cleanup run,
                        //                  or was installed as a copy with no hardlink partner.
                        let exists = real_loc.is_file() || {
                            // Also count reparse points that aren't regular files.
                            real_loc.symlink_metadata().is_ok()
                        };
                        if !exists {
                            log.push(format!("  [F] NOT FOUND (already cleaned): {}", real_loc.display()));
                        } else {
                            let nlinks = get_hardlink_count(real_loc);
                            if nlinks >= 2 {
                                log.push(format!("  [F] REMOVE (nlinks={}, no .orig): {}", nlinks, real_loc.display()));
                                match std::fs::remove_file(real_loc) {
                                    Ok(()) => {}
                                    Err(e) if e.raw_os_error() == Some(5) => {
                                        has_recoverable.store(true, Ordering::Relaxed);
                                        log.push("    PENDING (access denied, will retry)".into());
                                    }
                                    Err(e) => log.push(format!("    WARN: remove failed: {}", e)),
                                }
                            } else {
                                log.push(format!("  [F] SKIP (nlinks=1, already restored): {}", real_loc.display()));
                            }
                        }
                    }
                } else {
                    // ── type 1: directory junction ────────────────────────────────────────────
                    #[cfg(windows)]
                    let is_rp = real_loc.symlink_metadata().map(|m| {
                        use std::os::windows::fs::MetadataExt;
                        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
                        (m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT) != 0
                    }).unwrap_or(false);
                    #[cfg(not(windows))]
                    let is_rp = real_loc.symlink_metadata()
                        .map(|m| m.file_type().is_symlink()).unwrap_or(false);

                    let dir_exists = real_loc.is_dir() || is_rp;
                    if !dir_exists {
                        log.push(format!("  [D] NOT FOUND (already cleaned): {}", real_loc.display()));
                    } else if is_rp {
                        log.push(format!("  [D] REMOVE (junction): {}", real_loc.display()));
                        let r = std::fs::remove_dir(real_loc).or_else(|_| std::fs::remove_file(real_loc));
                        if let Err(e) = r {
                            if e.raw_os_error() == Some(5) {
                                has_recoverable.store(true, Ordering::Relaxed);
                                log.push("    PENDING (access denied, will retry)".into());
                            } else {
                                log.push(format!("    WARN: remove failed: {}", e));
                            }
                        }
                    } else {
                        // Plain directory (shouldn't happen for mod dirs, but handle gracefully).
                        log.push(format!("  [D] REMOVE (plain dir): {}", real_loc.display()));
                        std::fs::remove_dir_all(real_loc).ok();
                    }
                }

                log
            }).collect();

            header.extend(per_entry.into_iter().flatten());
            let recoverable = has_recoverable.load(Ordering::Relaxed);
            header.push(format!("=== cleanup done, has_recoverable={} ===", recoverable));
            header
        };

        let _ = std::fs::write(&log_path, all_log.join("\n"));

        // Keep .links if any entry was temporarily locked — the retry loop will call us again.
        if !has_recoverable.load(Ordering::Relaxed) {
            std::fs::remove_file(&links_file).ok();
        }
    }
    if mods_dir.exists() {
        prune_runtime_cache_reparse_points(&mods_dir);
    }
    if data_root_dir.exists() {
        prune_runtime_cache_reparse_points(&data_root_dir);
    }

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

    // scripts/ may contain files placed by the user — only remove files that
    // were added during this mod session (i.e. not present in the pre-launch baseline
    // captured in memory at launch time).
    // IMPORTANT: if no baseline exists, do NOT touch scripts at all.
    let scripts_dir = game_dir.join("scripts");
    if scripts_dir.exists() {
        if let Some(known_before) = get_scripts_baseline(game_dir) {
            if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if !p.is_file() { continue; }
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    if !known_before.contains(&name) {
                        std::fs::remove_file(&p).ok();
                    }
                }
            }
        }
    }

    let artifacts = ["lightfx.dll", "ModManager.dat", "PocoFoundation.dll"];
    let still_pending = links_file.exists()
        || modules_dir.exists()
        || artifacts.iter().any(|artifact| game_dir.join(artifact).exists())
        ;

    if !still_pending {
        clear_scripts_baseline(game_dir);
    }

    Ok(still_pending)
}

pub(crate) fn clean_mods_and_check_pending(game_path: &str) -> Result<bool, String> {
    let game_dir = PathBuf::from(game_path);
    clean_mods_internal(&game_dir)
}

#[command]
pub fn clean_mods(game_path: String) -> Result<(), String> {
    clean_mods_and_check_pending(&game_path).map(|_| ())
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
