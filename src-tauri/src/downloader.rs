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


/// Exponentially-weighted moving-average speed estimator.
///
/// Each call to `push(cumulative_bytes)` measures the instantaneous speed
/// since the previous call and blends it into a smoothed value using:
///   `smoothed = α × instant + (1−α) × smoothed`
///
/// With α=0.10 and a 200 ms ticker the effective time-constant is ~2 s,
/// which is responsive enough to follow real slowdowns while preventing
/// the sudden spikes caused by TCP burst / slow-start phases or momentary
/// pauses between chunks.
struct SpeedEwma {
    last_bytes: u64,
    last_time: Option<std::time::Instant>,
    smoothed: f64,
    alpha: f64,
}

impl SpeedEwma {
    fn new(alpha: f64) -> Self {
        Self { last_bytes: 0, last_time: None, smoothed: 0.0, alpha }
    }

    /// Push the current cumulative byte count and return smoothed bytes/sec.
    fn push(&mut self, bytes: u64) -> u64 {
        let now = std::time::Instant::now();
        if let Some(last_t) = self.last_time {
            let dt = now.duration_since(last_t).as_secs_f64();
            if dt >= 0.05 {
                let instant = bytes.saturating_sub(self.last_bytes) as f64 / dt;
                self.smoothed = if self.smoothed == 0.0 {
                    instant
                } else {
                    self.alpha * instant + (1.0 - self.alpha) * self.smoothed
                };
                self.last_bytes = bytes;
                self.last_time = Some(now);
            }
        } else {
            self.last_bytes = bytes;
            self.last_time = Some(now);
        }
        self.smoothed as u64
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

fn adaptive_extract_inflight_batches() -> usize {
    match cpu_count() {
        0..=4 => 1,
        5..=8 => 2,
        _ => 3,
    }
}

fn adaptive_rayon_threads() -> usize {
    cpu_count().clamp(2, 16)
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

    // ── Phase 2: streaming download + extract pipeline ─────────────────────
    //
    // Instead of: [download ALL sections] → [extract ALL files]  (two-phase, bar resets)
    // We do:      for each section that arrives → immediately extract its files
    //
    // This means download and extraction overlap, the progress bar advances
    // monotonically from 0→100%, and the speed display never drops to 0 between phases.
    //
    // pipeline_total = total_compressed + total_uncompressed.
    // global counter = global_dl (compressed bytes fetched) + global_ex (uncompressed bytes written).
    // bar % = (global_dl + global_ex) / pipeline_total — always increasing.
    let pipeline_total = pipeline_compressed.saturating_add(pipeline_uncompressed);
    let global_dl    = Arc::new(AtomicU64::new(0));
    let global_ex    = Arc::new(AtomicU64::new(0));
    let global_files = Arc::new(AtomicU32::new(0));

    // Pre-create all output directories before spawning extraction tasks.
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

    // Single unified progress ticker — no second ticker, no phase transition.
    let tick_app   = app.clone();
    let tick_dl    = global_dl.clone();
    let tick_ex    = global_ex.clone();
    let tick_files = global_files.clone();
    let tick_running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let tick_flag    = tick_running.clone();
    let tick_handle  = tokio::spawn(async move {
        let mut sw = SpeedEwma::new(0.10);
        while tick_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let dl         = tick_dl.load(Ordering::Relaxed);
            let ex         = tick_ex.load(Ordering::Relaxed);
            let files_done = tick_files.load(Ordering::Relaxed);
            let total_done = dl.saturating_add(ex);
            let speed      = sw.push(total_done);
            let remaining  = pipeline_total.saturating_sub(total_done);
            let eta        = if speed > 0 { remaining / speed } else { 0 };
            // Status transitions naturally: "downloading" while sections are
            // still being fetched, "extracting" only during the tail phase where
            // all downloads are done but some large LZMA files are still processing.
            let status = if dl < pipeline_compressed { "downloading" } else { "extracting" };
            let _ = tick_app.emit("download-progress", DownloadEvent {
                status: status.into(),
                file_name: format!("{}/{} files", files_done, total_dl_files),
                current_file: files_done,
                total_files: total_dl_files,
                downloaded_bytes: total_done,
                total_bytes: pipeline_total,
                speed, eta, error: None,
            });
        }
    });

    // Semaphore shared across all packages (adaptive concurrency).
    let dl_sem = Arc::new(Semaphore::new(adaptive_download_slots_game()));
    let mut all_errors: Vec<String> = Vec::new();

    // ── Extraction worker ───────────────────────────────────────────────────
    // Decoupled from the download loop via a channel: sending work is instant,
    // so rx.recv() is never blocked by rayon/LZMA and sections keep arriving
    // at full speed even when a large batch is being extracted.
    type ExtractWork = (Vec<IndexFileEntry>, HashMap<u32, Arc<Vec<u8>>>);
    let (extract_tx, mut extract_rx) = tokio::sync::mpsc::unbounded_channel::<ExtractWork>();
    let ex_err_store: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    // Single shared rayon pool for all extraction batches — avoids creating a
    // new pool per batch (expensive) and prevents thread-count multiplication
    // when multiple batches run concurrently.
    let extract_pool = Arc::new({
        let threads = adaptive_rayon_threads();
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(4).build().unwrap())
    });
    let extract_inflight = Arc::new(Semaphore::new(adaptive_extract_inflight_batches()));

    let extract_worker = {
        let gd        = game_dir.clone();
        let ex_b      = global_ex.clone();
        let ex_f      = global_files.clone();
        let err_store = ex_err_store.clone();
        let ex_sem = extract_inflight.clone();
        tokio::spawn(async move {
            // Dispatch every batch immediately without awaiting — all batches run
            // concurrently on tokio's blocking pool, sharing the rayon pool.
            // In-flight batch count is capped adaptively to avoid saturating
            // low-end CPUs and spinning disks.
            let mut batch_handles: Vec<tokio::task::JoinHandle<Vec<String>>> = Vec::new();
            while let Some((files, secs)) = extract_rx.recv().await {
                let gd2   = gd.clone();
                let ex_b2 = ex_b.clone();
                let ex_f2 = ex_f.clone();
                let pool  = extract_pool.clone();
                let permit = match ex_sem.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(e) => {
                        err_store.lock().unwrap().push(e.to_string());
                        break;
                    }
                };
                batch_handles.push(tokio::spawn(async move {
                    let _permit = permit;
                    tokio::task::spawn_blocking(move || {
                        use rayon::prelude::*;
                        pool.install(|| {
                            files.into_par_iter().filter_map(|entry| {
                                let raw = match extract_raw_arc(&entry, &secs) {
                                    Ok(r)  => r,
                                    Err(e) => return Some(e),
                                };
                                let data = if is_lzma(&raw) {
                                    decompress_lzma(&raw).unwrap_or(raw)
                                } else { raw };
                                let stripped = strip_first_component(&entry.path);
                                let dest = if stripped.is_empty() {
                                    gd2.join(&entry.file)
                                } else {
                                    gd2.join(stripped).join(&entry.file)
                                };
                                let written = data.len() as u64;
                                if let Err(e) = std::fs::write(&dest, &data) {
                                    return Some(format!("write {}: {}", entry.file, e));
                                }
                                ex_b2.fetch_add(written, Ordering::Relaxed);
                                ex_f2.fetch_add(1, Ordering::Relaxed);
                                None
                            }).collect()
                        })
                    }).await.unwrap_or_else(|e| vec![e.to_string()])
                }));
            }
            // Channel closed — wait for every batch to finish then collect errors.
            let mut all_errs: Vec<String> = Vec::new();
            for h in batch_handles {
                match h.await {
                    Ok(errs) => all_errs.extend(errs),
                    Err(e)   => all_errs.push(e.to_string()),
                }
            }
            err_store.lock().unwrap().extend(all_errs);
        })
    };

    // ── Parallel package downloads ───────────────────────────────────────────
    // Every package spawns its own task; the shared semaphore caps the total
    // number of concurrent section downloads across ALL packages at once.
    let pkg_err_store: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut pkg_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    for pkg in packages {
        let client    = client.clone();
        let dl_sem    = dl_sem.clone();
        let global_dl = global_dl.clone();
        let etx       = extract_tx.clone();
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

                        etx.send((ready_files, local_secs)).ok();

                        let still_needed: std::collections::BTreeSet<u32> = pending
                            .iter().flat_map(|e| file_sections(e, cab_size)).collect();
                        sections_store.lock().unwrap().retain(|s, _| still_needed.contains(s));
                    }
                }
            }
        }));
    }

    for h in pkg_handles { let _ = h.await; }
    all_errors.extend(pkg_err_store.lock().unwrap().drain(..));

    // Signal extraction worker that no more batches are coming, then await it
    // so all in-flight spawn_blocking calls finish before we check errors.
    drop(extract_tx);
    let _ = extract_worker.await;
    all_errors.extend(ex_err_store.lock().unwrap().drain(..));

    tick_running.store(false, Ordering::Relaxed);
    let _ = tick_handle.await;

    if !all_errors.is_empty() {
        return Err(all_errors.join("; "));
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Download complete".into(),
        current_file: total_dl_files, total_files: total_dl_files,
        downloaded_bytes: pipeline_total, total_bytes: pipeline_total,
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
    let mut buffer = [0u8; 65536];
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

    // Parallel SHA-256 check via rayon.
    let game_dir_c = game_dir.clone();
    let modules_vec: Vec<(String, String)> = modules.into_iter().collect();
    let modules_len = modules_vec.len();
    let modules_vec_c = modules_vec.clone();
    let needs_vec: Vec<bool> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
            modules_vec_c.par_iter().map(|(name, expected)| {
                let path = game_dir_c.join(name);
                if path.exists() {
                    sha256_hex_file(&path).map(|actual| actual != *expected).unwrap_or(true)
                } else {
                    true
                }
            }).collect()
        })
    }).await.unwrap_or_else(|_| vec![true; modules_len]);

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
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            let speed = sw.push(bytes);
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

fn materialize_mod_data_dir(
    cache_dir: &Path,
    data_dir: &Path,
    entries: &[ModListEntry],
) -> Result<(), String> {
    if data_dir.exists() {
        std::fs::remove_dir_all(data_dir)
            .map_err(|e| format!("Failed to reset {}: {}", data_dir.display(), e))?;
    }

    for entry in entries {
        let Some(raw_name) = entry.name.as_ref().filter(|name| !name.trim().is_empty()) else {
            continue;
        };
        let name = normalize_entry_name(raw_name);
        let name = name.as_str();

        let relative_path = Path::new(name);
        let source_path = cache_dir.join(relative_path);
        if !source_path.exists() {
            return Err(format!("Missing cached mod file: {}", source_path.display()));
        }

        let target_path = data_dir.join(relative_path);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
        }

        match std::fs::hard_link(&source_path, &target_path) {
            Ok(_) => {}
            Err(_) => {
                std::fs::copy(&source_path, &target_path)
                    .map_err(|e| format!("Failed to materialize {}: {}", target_path.display(), e))?;
            }
        }
    }

    Ok(())
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

fn normalize_link_target(path: PathBuf) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix("\\\\?\\") {
        PathBuf::from(stripped)
    } else {
        PathBuf::from(raw.as_ref())
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

fn resolves_into_mod_runtime(path: &Path, base_dir: &Path, mods_dir: &Path, data_root_dir: &Path) -> bool {
    let Some(target) = std::fs::read_link(path)
        .ok()
        .map(normalize_link_target)
        .map(|target| {
            if target.is_absolute() {
                target
            } else {
                path.parent().unwrap_or(base_dir).join(target)
            }
        }) else {
        return false;
    };

    target.starts_with(mods_dir) || target.starts_with(data_root_dir)
}

fn is_runtime_reparse_point(path: &Path, base_dir: &Path, mods_dir: &Path, data_root_dir: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if is_reparse_point(&meta) => {
            resolves_into_mod_runtime(path, base_dir, mods_dir, data_root_dir)
                || (!path.starts_with(mods_dir) && !path.starts_with(data_root_dir))
        }
        _ => false,
    }
}

fn has_runtime_mod_links(dir: &Path, mods_dir: &Path, data_root_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.starts_with(mods_dir) || path.starts_with(data_root_dir) {
            continue;
        }

        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if is_reparse_point(&meta) {
            if is_runtime_reparse_point(&path, dir, mods_dir, data_root_dir) {
                return true;
            }
            continue;
        }

        if meta.is_dir() && has_runtime_mod_links(&path, mods_dir, data_root_dir) {
            return true;
        }
    }

    false
}

fn has_orig_files_outside_runtime(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };

        if meta.is_dir() {
            let skip_dir = path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| matches!(name, "MODS" | ".data"))
                .unwrap_or(false);
            if skip_dir || is_reparse_point(&meta) {
                continue;
            }
            if has_orig_files_outside_runtime(&path) {
                return true;
            }
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) == Some("orig") {
            return true;
        }
    }

    false
}

fn remove_runtime_mod_links(dir: &Path, mods_dir: &Path, data_root_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.starts_with(mods_dir) || path.starts_with(data_root_dir) {
            continue;
        }

        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        let is_link = is_reparse_point(&meta);

        if is_link {
            if is_runtime_reparse_point(&path, dir, mods_dir, data_root_dir) {
                remove_path_safely(&path);
            }

            continue;
        }

        if meta.is_dir() {
            remove_runtime_mod_links(&path, mods_dir, data_root_dir);
        }
    }
}

fn prune_runtime_cache_orig_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };

        if meta.is_dir() {
            if is_reparse_point(&meta) {
                remove_path_safely(&path);
                continue;
            }
            prune_runtime_cache_orig_files(&path);
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) == Some("orig") {
            std::fs::remove_file(&path).ok();
        }
    }
}

#[command]
pub fn has_pending_mod_cleanup(game_path: String) -> Result<bool, String> {
    let game_dir = PathBuf::from(&game_path);
    if !game_dir.exists() {
        return Ok(false);
    }

    let mods_dir = game_dir.join("MODS");
    let data_root_dir = game_dir.join(".data");
    let modules_dir = game_dir.join("modules");
    let links_file = game_dir.join(".links");
    let artifacts = ["lightfx.dll", "ModManager.dat", "PocoFoundation.dll"];

    if links_file.exists() || modules_dir.exists() {
        return Ok(true);
    }

    if artifacts.iter().any(|artifact| game_dir.join(artifact).exists()) {
        return Ok(true);
    }

    if has_orig_files_outside_runtime(&game_dir) {
        return Ok(true);
    }

    Ok(has_runtime_mod_links(&game_dir, &mods_dir, &data_root_dir))
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

    let entries_vec: Vec<(String, Option<String>)> = entries.iter()
        .map(|e| (normalize_entry_name(&e.name.clone().unwrap_or_default()), e.checksum.clone()))
        .collect();

    // Parallel verification via rayon (same pattern as download_game verify phase).
    let cache_dir_c = cache_dir.clone();
    let needs_vec: Vec<(bool, bool)> = tokio::task::spawn_blocking(move || {
        use rayon::prelude::*;
        let pool = limited_pool();
        pool.install(|| {
            entries_vec.into_par_iter().map(|(name, expected)| {
                let cached_file = cache_dir_c.join(&name);
                if !cached_file.exists() {
                    return (true, false);
                }
                match expected {
                    Some(exp) => {
                        let mismatch = sha1_hex_file(&cached_file)
                            .map(|actual| actual != exp.to_lowercase())
                            .unwrap_or(true);
                        (mismatch, mismatch)
                    }
                    None => (false, false),
                }
            }).collect()
        })
    }).await.unwrap_or_else(|_| vec![(true, false); entries.len()]);

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

    if mods_to_download.is_empty() {
        let _ = app.emit("download-progress", DownloadEvent {
            status: "extracting".into(),
            file_name: "Materializing mods...".into(),
            current_file: total_mods, total_files: total_mods,
            downloaded_bytes: 0, total_bytes: 0, speed: 0, eta: 0, error: None,
        });
        materialize_mod_data_dir(&cache_dir, &data_dir, &entries)?;
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
        while p_flag.load(Ordering::Relaxed) {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let done = p_done.load(Ordering::Relaxed);
            let bytes = p_bytes.load(Ordering::Relaxed);
            let speed = sw.push(bytes);
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

    if any_mismatch {
        if data_dir.exists() {
            std::fs::remove_dir_all(&data_dir).ok();
        }
    }

    let _ = app.emit("download-progress", DownloadEvent {
        status: "extracting".into(),
        file_name: "Materializing mods...".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
        speed: 0, eta: 0, error: None,
    });
    materialize_mod_data_dir(&cache_dir, &data_dir, &entries)?;
    let _ = app.emit("download-progress", DownloadEvent {
        status: "completed".into(),
        file_name: "Mods downloaded".into(),
        current_file: dl_total, total_files: dl_total,
        downloaded_bytes: final_bytes, total_bytes: final_bytes.max(1),
        speed: 0, eta: 0, error: None,
    });

    Ok(())
}


#[command]
pub fn clean_mods(game_path: String) -> Result<(), String> {
    let game_dir = PathBuf::from(&game_path);
    let links_file = game_dir.join(".links");
    let mods_dir = game_dir.join("MODS");
    let data_root_dir = game_dir.join(".data");

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

                if real_loc.starts_with(&mods_dir) || real_loc.starts_with(&data_root_dir) {
                    continue;
                }

                let orig_path = {
                    let mut p = real_loc.as_os_str().to_os_string();
                    p.push(".orig");
                    PathBuf::from(p)
                };

                if link_type == 0 {
                    if real_loc.exists() || real_loc.symlink_metadata().is_ok() {
                        remove_path_safely(&real_loc);
                    }
                    // Use symlink_metadata so broken-symlink .orig files are caught too.
                    if orig_path.symlink_metadata().is_ok() {
                        std::fs::rename(&orig_path, &real_loc).ok();
                    }
                } else if link_type == 1 {
                    if real_loc.exists() || real_loc.symlink_metadata().is_ok() {
                        remove_path_safely(&real_loc);
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
            let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if meta.is_dir() {
                if let Some(original_name) = file_name.strip_suffix(".orig") {
                    let mut original = path.clone();
                    original.set_file_name(original_name);
                    // Whatever is at the target (junction, real dir, absent) — remove
                    // it and restore the backup. If the rename already happened, no
                    // .orig would be present, so seeing .orig always means "not yet restored".
                    if original.symlink_metadata().is_ok() {
                        remove_path_safely(&original);
                    }
                    std::fs::rename(&path, &original).ok();
                    continue;
                }
                let skip_dir = matches!(file_name, "MODS" | ".data");
                if skip_dir || is_reparse_point(&meta) {
                    continue;
                }
                restore_orig_files(&path);
                continue;
            }
            // File backed up as "filename.ext.orig" or "filename.orig".
            // If .orig still exists the original was never restored — always restore.
            if let Some(original_name) = file_name.strip_suffix(".orig") {
                let mut original = path.clone();
                original.set_file_name(original_name);
                if original.symlink_metadata().is_ok() {
                    remove_path_safely(&original);
                }
                std::fs::rename(&path, &original).ok();
            }
        }
    }
    // Remove all mod junctions/symlinks FIRST so that restore_orig_files never
    // encounters a live reparse point at the restore target — preventing the
    // race where the rename fails silently while the junction is still there.
    remove_runtime_mod_links(&game_dir, &mods_dir, &data_root_dir);
    restore_orig_files(&game_dir);
    if mods_dir.exists() {
        prune_runtime_cache_orig_files(&mods_dir);
    }
    if data_root_dir.exists() {
        prune_runtime_cache_orig_files(&data_root_dir);
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
    // were added during this mod session (i.e. not present in the pre-launch snapshot).
    let scripts_dir = game_dir.join("scripts");
    let snapshot_path = game_dir.join(".scripts_snapshot");
    if scripts_dir.exists() {
        let known_before: std::collections::HashSet<String> = std::fs::read_to_string(&snapshot_path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect();
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
    } else {
        std::fs::create_dir_all(&scripts_dir).ok();
    }
    std::fs::remove_file(&snapshot_path).ok();

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
