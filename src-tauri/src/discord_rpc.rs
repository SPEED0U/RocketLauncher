use discord_rich_presence::{
    activity::{Activity, Assets, Button, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::command;

const LAUNCHER_APP_ID: &str = "1494263333266653335";
const DEFAULT_GAME_APP_ID: &str = "540651192179752970";

static RPC_CLIENT: Mutex<Option<DiscordIpcClient>> = Mutex::new(None);
static RPC_START_TIME: Mutex<Option<i64>> = Mutex::new(None);
static CURRENT_APP_ID: Mutex<String> = Mutex::new(String::new());

fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn connect_client(app_id: &str) -> Result<DiscordIpcClient, String> {
    let mut client = DiscordIpcClient::new(app_id).map_err(|e| format!("new: {}", e))?;

    let mut last_err = String::new();
    for _ in 0..3 {
        match client.connect() {
            Ok(_) => {
                std::thread::sleep(Duration::from_millis(1000));
                return Ok(client);
            }
            Err(e) => {
                last_err = format!("{}", e);
                std::thread::sleep(Duration::from_millis(1000));
                client = DiscordIpcClient::new(app_id).map_err(|e| format!("new: {}", e))?;
            }
        }
    }
    Err(format!("connect failed after 3 attempts: {}", last_err))
}

fn set_activity_with_retry(
    state: Option<String>,
    details: Option<String>,
    large_image: Option<String>,
    large_text: Option<String>,
    small_image: Option<String>,
    small_text: Option<String>,
    button1_label: Option<String>,
    button1_url: Option<String>,
    button2_label: Option<String>,
    button2_url: Option<String>,
) -> Result<(), String> {
    for attempt in 0..2 {
        let result = do_set_activity(
            &state, &details, &large_image, &large_text,
            &small_image, &small_text, &button1_label, &button1_url,
            &button2_label, &button2_url,
        );
        match result {
            Ok(_) => return Ok(()),
            Err(e) => {
                if attempt == 0 {
                    let app_id = CURRENT_APP_ID.lock().map_err(|e| e.to_string())?.clone();
                    if app_id.is_empty() {
                        return Err("Discord RPC not initialized".into());
                    }
                    let old_client = {
                        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
                        client_lock.take()
                    };
                    if let Some(mut old) = old_client {
                        let _ = old.close();
                    }
                    std::thread::sleep(Duration::from_millis(500));
                    let new_client =
                        connect_client(&app_id).map_err(|e2| format!("reconnect failed: {}", e2))?;
                    {
                        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
                        *client_lock = Some(new_client);
                    }
                } else {
                    return Err(e);
                }
            }
        }
    }
    Err("unexpected".into())
}

fn do_set_activity(
    state: &Option<String>,
    details: &Option<String>,
    large_image: &Option<String>,
    large_text: &Option<String>,
    small_image: &Option<String>,
    small_text: &Option<String>,
    button1_label: &Option<String>,
    button1_url: &Option<String>,
    button2_label: &Option<String>,
    button2_url: &Option<String>,
) -> Result<(), String> {
    let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
    let client = client_lock.as_mut().ok_or("Discord RPC not initialized")?;

    let start_time = RPC_START_TIME
        .lock()
        .map_err(|e| e.to_string())?
        .unwrap_or_else(now_epoch);

    let timestamps = Timestamps::new().start(start_time);

    let mut assets = Assets::new();
    if let Some(ref key) = large_image {
        assets = assets.large_image(key.as_str());
    }
    if let Some(ref text) = large_text {
        assets = assets.large_text(text.as_str());
    }
    if let Some(ref key) = small_image {
        assets = assets.small_image(key.as_str());
    }
    if let Some(ref text) = small_text {
        assets = assets.small_text(text.as_str());
    }

    let mut activity = Activity::new()
        .timestamps(timestamps)
        .assets(assets);

    if let Some(ref s) = state {
        activity = activity.state(s.as_str());
    }
    if let Some(ref d) = details {
        activity = activity.details(d.as_str());
    }

    let mut buttons: Vec<Button> = Vec::new();
    if let (Some(ref label), Some(ref url)) = (button1_label, button1_url) {
        if !label.is_empty() && !url.is_empty() {
            buttons.push(Button::new(label.as_str(), url.as_str()));
        }
    }
    if let (Some(ref label), Some(ref url)) = (button2_label, button2_url) {
        if !label.is_empty() && !url.is_empty() {
            buttons.push(Button::new(label.as_str(), url.as_str()));
        }
    }
    if !buttons.is_empty() {
        activity = activity.buttons(buttons);
    }

    client
        .set_activity(activity)
        .map_err(|e| format!("{}", e))
}

fn discord_rpc_init_sync() -> Result<(), String> {
    let current_id_value = CURRENT_APP_ID
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let has_client = RPC_CLIENT
        .lock()
        .map_err(|e| e.to_string())?
        .is_some();
    if current_id_value == LAUNCHER_APP_ID && has_client {
        return Ok(());
    }

    let old_client = {
        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
        client_lock.take()
    };
    if let Some(mut old) = old_client {
        let _ = old.clear_activity();
        let _ = old.close();
        drop(old);
        std::thread::sleep(Duration::from_millis(2000));
    }

    let client = connect_client(LAUNCHER_APP_ID)?;
    {
        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
        *client_lock = Some(client);
    }
    {
        let mut current_id = CURRENT_APP_ID.lock().map_err(|e| e.to_string())?;
        *current_id = LAUNCHER_APP_ID.to_string();
    }
    *RPC_START_TIME.lock().map_err(|e| e.to_string())? = Some(now_epoch());
    Ok(())
}

fn discord_rpc_reconnect_sync(app_id: Option<String>) -> Result<(), String> {
    let target_id = app_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_GAME_APP_ID);

    let current_id_value = CURRENT_APP_ID
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let has_client = RPC_CLIENT
        .lock()
        .map_err(|e| e.to_string())?
        .is_some();
    if current_id_value == target_id && has_client {
        return Ok(());
    }

    let old_client = {
        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
        client_lock.take()
    };
    if let Some(mut old) = old_client {
        let _ = old.clear_activity();
        let _ = old.close();
        drop(old);
        std::thread::sleep(Duration::from_millis(2000));
    }

    let client = connect_client(target_id)?;
    {
        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
        *client_lock = Some(client);
    }
    {
        let mut current_id = CURRENT_APP_ID.lock().map_err(|e| e.to_string())?;
        *current_id = target_id.to_string();
    }
    *RPC_START_TIME.lock().map_err(|e| e.to_string())? = Some(now_epoch());
    Ok(())
}

fn discord_rpc_update_sync(
    state: Option<String>,
    details: Option<String>,
    large_image: Option<String>,
    large_text: Option<String>,
    small_image: Option<String>,
    small_text: Option<String>,
    button1_label: Option<String>,
    button1_url: Option<String>,
    button2_label: Option<String>,
    button2_url: Option<String>,
) -> Result<(), String> {
    set_activity_with_retry(
        state, details, large_image, large_text, small_image, small_text, button1_label,
        button1_url, button2_label, button2_url,
    )
}

fn discord_rpc_stop_sync() -> Result<(), String> {
    let old_client = {
        let mut client_lock = RPC_CLIENT.lock().map_err(|e| e.to_string())?;
        client_lock.take()
    };

    if let Some(mut client) = old_client {
        let _ = client.clear_activity();
        let _ = client.close();
    }

    *CURRENT_APP_ID.lock().map_err(|e| e.to_string())? = String::new();
    *RPC_START_TIME.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}

#[command]
pub fn discord_rpc_init() -> Result<(), String> {
    discord_rpc_init_sync()
}

#[command]
pub fn discord_rpc_reconnect(app_id: Option<String>) -> Result<(), String> {
    discord_rpc_reconnect_sync(app_id)
}

#[command]
pub fn discord_rpc_update(
    state: Option<String>,
    details: Option<String>,
    large_image: Option<String>,
    large_text: Option<String>,
    small_image: Option<String>,
    small_text: Option<String>,
    button1_label: Option<String>,
    button1_url: Option<String>,
    button2_label: Option<String>,
    button2_url: Option<String>,
) -> Result<(), String> {
    discord_rpc_update_sync(
        state,
        details,
        large_image,
        large_text,
        small_image,
        small_text,
        button1_label,
        button1_url,
        button2_label,
        button2_url,
    )
}

#[command]
pub fn discord_rpc_stop() -> Result<(), String> {
    discord_rpc_stop_sync()
}
