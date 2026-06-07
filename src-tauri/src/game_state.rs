use crate::discord_rpc;
use std::sync::Mutex;

struct GameState {
    server_name: String,
    persona_name: String,
    persona_level: String,
    persona_avatar_id: String,
    persona_car_name: String,
    persona_ids: Vec<String>,
    carslots_xml: String,
    event_id: i32,
    in_safehouse: bool,
    treasure_collected: i32,
    treasure_total: i32,
    treasure_day: i32,
    logged_persona_id: String,
}

impl Default for GameState {
    fn default() -> Self {
        Self {
            server_name: String::new(),
            persona_name: String::new(),
            persona_level: String::new(),
            persona_avatar_id: String::new(),
            persona_car_name: String::new(),
            persona_ids: Vec::new(),
            carslots_xml: String::new(),
            event_id: 0,
            in_safehouse: false,
            treasure_collected: 0,
            treasure_total: 15,
            treasure_day: 0,
            logged_persona_id: String::new(),
        }
    }
}

static GAME_STATE: Mutex<Option<GameState>> = Mutex::new(None);
static GAME_RPC_ENABLED: Mutex<bool> = Mutex::new(true);

pub fn set_server_name(name: &str) {
    let mut state = GAME_STATE.lock().unwrap();
    let s = state.get_or_insert_with(GameState::default);
    s.server_name = name.to_string();
}

pub fn set_rpc_enabled(enabled: bool) {
    let mut lock = GAME_RPC_ENABLED.lock().unwrap();
    *lock = enabled;
}

pub fn _reset() {
    let mut state = GAME_STATE.lock().unwrap();
    *state = None;
}

pub fn handle_game_response(path: &str, query: &str, response_body: &str) {
    let mut lock = GAME_STATE.lock().unwrap();
    let state = match lock.as_mut() {
        Some(s) => s,
        None => {
            eprintln!("[GAME_STATE] Ignoring (not initialized): {}", path);
            return;
        }
    };

    let uri = path
        .strip_prefix("/nfsw/Engine.svc")
        .or_else(|| path.strip_prefix("/Engine.svc"))
        .unwrap_or(path);

    eprintln!("[GAME_STATE] uri={} query={} body_len={}", uri, query, response_body.len());

    if uri == "/User/GetPermanentSession" {
        match parse_permanent_session(response_body) {
            Some((name, level, avatar, pid)) => {
                eprintln!("[GAME_STATE] Persona: {} lvl {} avatar {} id {}", name, level, avatar, pid);
                state.persona_name = name;
                state.persona_level = level;
                state.persona_avatar_id = format!("avatar_{}", avatar);
                state.persona_ids.clear();
                for id in parse_all_persona_ids(response_body) {
                    eprintln!("[GAME_STATE] persona_id registered: {}", id);
                    state.persona_ids.push(id);
                }
                if !state.persona_ids.contains(&pid) {
                    state.persona_ids.push(pid);
                }
            }
            None => {
                eprintln!("[GAME_STATE] Failed to parse GetPermanentSession (body_len={})", response_body.len());
            }
        }
    }

    if uri == "/User/SecureLoginPersona" {
        let pid = query
            .split('&')
            .last()
            .and_then(|p| p.split('=').last())
            .unwrap_or("");
        eprintln!("[GAME_STATE] SecureLoginPersona: logged_persona_id={}", pid);
        state.logged_persona_id = pid.to_string();
    }

    if uri == "/User/SecureLogoutPersona" {
        eprintln!("[GAME_STATE] SecureLogoutPersona: resetting persona data");
        state.persona_name = String::new();
        state.persona_level = String::new();
        state.persona_avatar_id = String::new();
        state.persona_car_name = String::new();
        state.carslots_xml = String::new();
        state.treasure_collected = 0;
        state.logged_persona_id = String::new();
    }

    if uri == "/DriverPersona/GetPersonaInfo" {
        if let Some((name, level, avatar, pid)) = parse_persona_info(response_body) {
            let should_update = state.logged_persona_id.is_empty() || pid == state.logged_persona_id;
            if should_update {
                eprintln!("[GAME_STATE] GetPersonaInfo: updating to {} lvl {} avatar {} pid {}", name, level, avatar, pid);
                state.persona_name = name;
                state.persona_level = level;
                state.persona_avatar_id = format!("avatar_{}", avatar);
                state.logged_persona_id = pid;
            } else {
                eprintln!("[GAME_STATE] GetPersonaInfo: skipping (pid={} != logged={})", pid, state.logged_persona_id);
            }
        }
    }

    {
        let parts: Vec<&str> = uri.split('/').collect();
        if parts.len() >= 4 && parts[1].eq_ignore_ascii_case("personas") {
            let persona_id_in_path = parts[2];
            let action = parts[3];
            if action.eq_ignore_ascii_case("carslots") {
                state.carslots_xml = response_body.to_string();
                if let Some(car_id) = parse_default_car(response_body) {
                    state.persona_car_name = crate::rpc_data::get_car_name(&car_id);
                    eprintln!("[GAME_STATE] carslots({}): car={}", persona_id_in_path, state.persona_car_name);
                }
            } else if action.eq_ignore_ascii_case("defaultcar") && parts.len() >= 5 {
                let car_id = parts[4];
                if let Some(raw_id) = find_car_by_id(&state.carslots_xml, car_id) {
                    state.persona_car_name = crate::rpc_data::get_car_name(&raw_id);
                    eprintln!("[GAME_STATE] defaultcar({}): changed to {}", persona_id_in_path, state.persona_car_name);
                }
            }
        }
    }

    if uri == "/DriverPersona/UpdatePersonaPresence" {
        let presence_val = query
            .split('&')
            .find_map(|p| p.strip_prefix("presence="))
            .unwrap_or("0");

        if presence_val == "1" {
            state.in_safehouse = false;
            let car = if state.persona_car_name.is_empty() {
                "Unknown car".to_string()
            } else {
                state.persona_car_name.clone()
            };
            update_rpc(
                &format!("Driving {}", car),
                &state.server_name.clone(),
                &state.persona_name.clone(),
                &state.persona_level.clone(),
                &state.persona_avatar_id.clone(),
                "Freeroaming",
                "gamemode_freeroam",
            );
        } else {
            state.in_safehouse = true;
            update_rpc(
                "In safehouse",
                &state.server_name.clone(),
                &state.persona_name.clone(),
                &state.persona_level.clone(),
                &state.persona_avatar_id.clone(),
                "In safehouse",
                "gamemode_safehouse",
            );
        }
        return;
    }

    if uri == "/events/gettreasurehunteventsession" {
        if let Some((collected, total, day)) = parse_treasure_hunt(response_body) {
            state.treasure_collected = collected;
            state.treasure_total = total;
            state.treasure_day = day;
        }
    }

    if uri == "/events/notifycoincollected" {
        state.treasure_collected += 1;
        let detail = if state.treasure_collected >= state.treasure_total {
            format!("Finished collecting gems ({} of {})", state.treasure_collected, state.treasure_total)
        } else {
            format!("Collecting gems ({} of {})", state.treasure_collected, state.treasure_total)
        };
        update_rpc(
            &detail,
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            &format!("Treasure hunt - Day: {}", state.treasure_day),
            "gamemode_treasure",
        );
        return;
    }

    if uri == "/matchmaking/leavelobby" || uri == "/matchmaking/declineinvite" {
        let car = if state.persona_car_name.is_empty() { "Unknown Car" } else { &state.persona_car_name };
        update_rpc(
            &format!("Driving {}", car),
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "Freeroaming",
            "gamemode_freeroam",
        );
        return;
    }

    if uri == "/matchmaking/acceptinvite" {
        if let Some(eid) = parse_event_id_from_lobby(response_body) {
            state.event_id = eid;
            let event_name = crate::rpc_data::get_event_name(eid as u32);
            let event_type = crate::rpc_data::get_event_type(eid as u32);
            update_rpc(
                &format!("In Lobby: {}", event_name),
                &state.server_name.clone(),
                &state.persona_name.clone(),
                &state.persona_level.clone(),
                &state.persona_avatar_id.clone(),
                "In multiplayer lobby",
                &event_type,
            );
        }
        return;
    }

    if uri == "/matchmaking/joinqueueracenow" {
        update_rpc(
            "Searching for event",
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "Freeroaming",
            "gamemode_freeroam",
        );
        return;
    }

    if uri.starts_with("/matchmaking/launchevent") {
        let parts: Vec<&str> = uri.split('/').collect();
        if let Some(id_str) = parts.get(3) {
            if let Ok(eid) = id_str.parse::<i32>() {
                state.event_id = eid;
            }
        }
        let event_name = crate::rpc_data::get_event_name(state.event_id as u32);
        let event_type = crate::rpc_data::get_event_type(state.event_id as u32);
        update_rpc(
            &format!("Loading: {}", event_name),
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "Loading event",
            &event_type,
        );
        return;
    }

    if uri == "/event/launched" {
        let event_name = crate::rpc_data::get_event_name(state.event_id as u32);
        let event_type = crate::rpc_data::get_event_type(state.event_id as u32);
        update_rpc(
            &format!("Racing: {}", event_name),
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "In event",
            &event_type,
        );
        return;
    }

    if uri == "/event/arbitration" {
        let event_name = crate::rpc_data::get_event_name(state.event_id as u32);
        let event_type = crate::rpc_data::get_event_type(state.event_id as u32);
        update_rpc(
            &format!("Finished: {}", event_name),
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "Finished event",
            &event_type,
        );
        return;
    }

    if uri.contains("catalog") && state.in_safehouse {
        let detail = if query.contains("NFSW_NA_EP_VINYLS_Category") {
            "Creating a livery"
        } else if query.contains("PAINTS_BODY") {
            "Painting a car"
        } else if query.contains("PERFORMANCEPART") {
            "Tuning a car"
        } else if query.contains("VISUALPART") {
            "Ricing a car"
        } else if query.contains("SKILLMODPART") {
            "Skillmodding a car (skill issue)"
        } else if query.contains("PRESETCAR") {
            "Purchasing a car"
        } else if query.contains("BoosterPacks") {
            "Opening cardpacks"
        } else {
            return;
        };
        update_rpc(
            detail,
            &state.server_name.clone(),
            &state.persona_name.clone(),
            &state.persona_level.clone(),
            &state.persona_avatar_id.clone(),
            "In safehouse",
            "gamemode_safehouse",
        );
    }
}

fn update_rpc(
    details: &str,
    server_name: &str,
    persona_name: &str,
    persona_level: &str,
    persona_avatar: &str,
    small_text: &str,
    small_image: &str,
) {
    if !*GAME_RPC_ENABLED.lock().unwrap() {
        eprintln!("[GAME_STATE] RPC disabled for this server category, skipping update");
        return;
    }

    let large_text = if !persona_name.is_empty() {
        format!("{} - Level: {}", persona_name, persona_level)
    } else {
        "Need for Speed: World".to_string()
    };
    let large_image = if !persona_avatar.is_empty() {
        persona_avatar.to_string()
    } else {
        "nfsw".to_string()
    };

    eprintln!("[GAME_STATE] RPC update: details='{}' state='{}' large_img='{}' large_txt='{}' small_img='{}' small_txt='{}'", 
        details, server_name, large_image, large_text, small_image, small_text);
    match discord_rpc::discord_rpc_update(
        Some(server_name.to_string()),
        Some(details.to_string()),
        Some(large_image),
        Some(large_text),
        Some(small_image.to_string()),
        Some(small_text.to_string()),
        Some("Project site".to_string()),
        Some("https://soapboxrace.world".to_string()),
        None,
        None,
    ) {
        Ok(_) => eprintln!("[GAME_STATE] RPC updated successfully"),
        Err(e) => eprintln!("[GAME_STATE] RPC update FAILED: {}", e),
    }
}


fn xml_inner_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(&xml[start..end])
}

fn parse_permanent_session(xml: &str) -> Option<(String, String, String, String)> {
    let profile_start = xml.find("<ProfileData")?;
    let profile_end = xml[profile_start..].find("</ProfileData>")? + profile_start + "</ProfileData>".len();
    let profile = &xml[profile_start..profile_end];

    let name = xml_inner_text(profile, "Name")?.to_string();
    let level = xml_inner_text(profile, "Level")?.to_string();
    let icon = xml_inner_text(profile, "IconIndex")?.to_string();
    let pid = xml_inner_text(profile, "PersonaId")?.to_string();
    Some((name, level, icon, pid))
}

fn parse_all_persona_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut search_start = 0;
    while let Some(rel) = xml[search_start..].find("<ProfileData") {
        let abs_start = search_start + rel;
        let end_tag = "</ProfileData>";
        let abs_end = match xml[abs_start..].find(end_tag) {
            Some(e) => abs_start + e + end_tag.len(),
            None => break,
        };
        let block = &xml[abs_start..abs_end];
        if let Some(pid) = xml_inner_text(block, "PersonaId") {
            ids.push(pid.to_string());
        }
        search_start = abs_end;
    }
    ids
}

fn parse_persona_info(xml: &str) -> Option<(String, String, String, String)> {
    let name = xml_inner_text(xml, "Name")?.to_string();
    let level = xml_inner_text(xml, "Level")?.to_string();
    let icon = xml_inner_text(xml, "IconIndex")?.to_string();
    let pid = xml_inner_text(xml, "PersonaId")?.to_string();
    Some((name, level, icon, pid))
}

fn parse_default_car(xml: &str) -> Option<String> {
    let default_idx_str = xml_inner_text(xml, "DefaultOwnedCarIndex")?;
    let default_idx: usize = default_idx_str.parse().ok()?;

    let mut idx = 0usize;
    let mut search_start = 0usize;
    loop {
        let owned_start = xml[search_start..].find("<OwnedCarTrans")?;
        let abs_start = search_start + owned_start;
        let owned_end = xml[abs_start..].find("</OwnedCarTrans>")? + abs_start + "</OwnedCarTrans>".len();
        let block = &xml[abs_start..owned_end];

        if idx == default_idx {
            if let Some(custom_start) = block.find("<CustomCar") {
                let custom_block = &block[custom_start..];
                return xml_inner_text(custom_block, "Name").map(|s| s.to_string());
            }
        }
        idx += 1;
        search_start = owned_end;
    }
}

fn find_car_by_id(carslots_xml: &str, car_id: &str) -> Option<String> {
    let mut search_start = 0usize;
    loop {
        let owned_start = carslots_xml[search_start..].find("<OwnedCarTrans")?;
        let abs_start = search_start + owned_start;
        let owned_end = carslots_xml[abs_start..].find("</OwnedCarTrans>")? + abs_start + "</OwnedCarTrans>".len();
        let block = &carslots_xml[abs_start..owned_end];

        if let Some(id) = xml_inner_text(block, "Id") {
            if id == car_id {
                if let Some(custom_start) = block.find("<CustomCar") {
                    let custom_block = &block[custom_start..];
                    return xml_inner_text(custom_block, "Name").map(|s| s.to_string());
                }
            }
        }
        search_start = owned_end;
    }
}

fn parse_event_id_from_lobby(xml: &str) -> Option<i32> {
    xml_inner_text(xml, "EventId")?.parse().ok()
}

fn parse_treasure_hunt(xml: &str) -> Option<(i32, i32, i32)> {
    let coins_bitmask: i32 = xml_inner_text(xml, "CoinsCollected")?.parse().ok()?;
    let total: i32 = xml_inner_text(xml, "NumCoins")?.parse().ok()?;
    let day: i32 = xml_inner_text(xml, "Streak")?.parse().ok()?;

    let mut collected = 0;
    for i in 0..15 {
        if (coins_bitmask & (1 << (15 - i))) != 0 {
            collected += 1;
        }
    }

    Some((collected, total, day))
}
