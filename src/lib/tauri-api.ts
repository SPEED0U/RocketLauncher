import { invoke } from "@tauri-apps/api/core";
import type { ServerInfo, ServerListEntry, ServerDetails, CDNEntry } from "./types";
import { URLS } from "./urls";
import path from "path";

interface FetchResponse {
  status: number;
  body: string;
}

async function tauriFetch(
  url: string,
  options?: {
    method?: string;
    body?: string;
    contentType?: string;
  }
): Promise<FetchResponse> {
  return invoke<FetchResponse>("fetch_url", {
    url,
    method: options?.method,
    body: options?.body,
    contentType: options?.contentType,
  });
}

async function tauriFetchJSON<T>(url: string): Promise<T> {
  const res = await tauriFetch(url);
  if (res.status >= 400) throw new Error(`HTTP ${res.status}`);
  return JSON.parse(res.body) as T;
}

export async function fetchServerList(): Promise<ServerInfo[]> {
  const body = await invoke<string>("fetch_server_list", {
    apiUrl: URLS.API_MAIN,
  });
  const raw = JSON.parse(body) as ServerListEntry[];
  return raw.map((s) => ({
    id: s.id,
    name: s.name,
    ip: s.ip_address,
    category: s.category || "Uncategorized",
    bannerUrl: s.banner_url,
    iconUrl: s.icon_url,
    registeredCount: s.number_of_registered,
    isOfficial: s.is_special,
    discordAppId: s.discord_presence_key,
    country: s.country,
  }));
}

export async function fetchServerDetails(
  serverIp: string
): Promise<ServerDetails> {
  return tauriFetchJSON<ServerDetails>(URLS.serverInfo(serverIp));
}

export async function fetchCDNList(): Promise<CDNEntry[]> {
  return tauriFetchJSON<CDNEntry[]>(URLS.cdnList(URLS.API_MAIN));
}

async function hashPassword(
  password: string,
  email: string,
  authHash?: string
): Promise<string> {
  const encoder = new TextEncoder();

  async function sha1(data: string): Promise<string> {
    const hash = await crypto.subtle.digest("SHA-1", encoder.encode(data));
    return Array.from(new Uint8Array(hash)).map(b => b.toString(16).padStart(2, "0")).join("");
  }
  async function sha256(data: string): Promise<string> {
    const hash = await crypto.subtle.digest("SHA-256", encoder.encode(data));
    return Array.from(new Uint8Array(hash)).map(b => b.toString(16).padStart(2, "0")).join("");
  }
  async function md5(data: string): Promise<string> {
    const res = await invoke<string>("hash_string_md5", { input: data });
    return res;
  }

  switch (authHash) {
    case "1.0":
    case "true":
      return password;
    case "1.1":
      return md5(password);
    case "1.2":
    case "false":
    case undefined:
      return sha1(password);
    case "1.3":
      return sha256(password);
    case "2.0":
      return md5(email + password);
    case "2.1":
      return sha1(email + password);
    case "2.2":
      return sha256(email + password);
    default:
      return sha1(password);
  }
}

interface LoginResult {
  success: boolean;
  token?: string;
  userId?: string;
  warning?: string;
  error?: string;
  banned?: { reason: string; expires: string };
}

export async function loginToServer(
  serverIp: string,
  email: string,
  password: string,
  modernAuth?: boolean,
  authHash?: string
): Promise<LoginResult> {
  try {
    const hashedPassword = await hashPassword(password, email, authHash);

    if (modernAuth) {
      const modernPassword = authHash ? hashedPassword : password;
      const res = await tauriFetch(URLS.authModernLogin(serverIp), {
        method: "POST",
        contentType: "application/json",
        body: JSON.stringify({ email, password: modernPassword, upgrade: true }),
      });

      let json: Record<string, unknown>;
      try {
        json = JSON.parse(res.body);
      } catch {
        return { success: false, error: res.body || `HTTP ${res.status}` };
      }

      const get = (key: string): unknown => {
        const lower = key.toLowerCase();
        for (const k of Object.keys(json)) {
          if (k.toLowerCase() === lower) return json[k];
        }
        return undefined;
      };

      const err = get("error");
      if (err) {
        return { success: false, error: String(err) };
      }

      const token = get("token");
      const userId = get("userid");

      return {
        success: true,
        token: String(token ?? ""),
        userId: String(userId ?? "0"),
        warning: get("warning") ? String(get("warning")) : undefined,
      };
    } else {
      const url = `${URLS.authLogin(serverIp)}?email=${encodeURIComponent(email)}&password=${encodeURIComponent(hashedPassword)}`;
      const res = await tauriFetch(url);

      const xml = res.body;
      const userIdMatch = xml.match(/<UserId>(.+?)<\/UserId>/);
      const tokenMatch = xml.match(/<LoginToken>(.+?)<\/LoginToken>/);
      const errorMatch = xml.match(/<Description>(.+?)<\/Description>/);
      const warningMatch = xml.match(/<Warning>(.+?)<\/Warning>/);
const banReasonMatch = xml.match(/<Ban>[\s\S]*?<Reason>(.+?)<\/Reason>/);
  const banExpiresMatch = xml.match(/<Ban>[\s\S]*?<Expires>(.+?)<\/Expires>/);

      if (banReasonMatch?.[1]) {
        return {
          success: false,
          error: "Compte banni",
          banned: {
            reason: banReasonMatch[1],
            expires: banExpiresMatch?.[1] || "Permanent",
          },
        };
      }

      if (tokenMatch?.[1] && userIdMatch?.[1] && userIdMatch[1] !== "0") {
        return {
          success: true,
          token: tokenMatch[1],
          userId: userIdMatch[1],
          warning: warningMatch?.[1] || undefined,
        };
      }

      return {
        success: false,
        error: errorMatch?.[1] || "Authentication failed",
      };
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

export async function registerOnServer(
  serverIp: string,
  email: string,
  password: string,
  ticket?: string,
  modernAuth?: boolean,
  authHash?: string
): Promise<{ success: boolean; error?: string }> {
  try {
    const hashedPassword = await hashPassword(password, email, authHash);

    if (modernAuth) {
      const modernPassword = authHash ? hashedPassword : password;
      const res = await tauriFetch(URLS.authModernRegister(serverIp), {
        method: "POST",
        contentType: "application/json",
        body: JSON.stringify({ email, password: modernPassword, ticket: ticket || null }),
      });

      let json: Record<string, unknown>;
      try {
        json = JSON.parse(res.body);
      } catch {
        return { success: false, error: res.body || `HTTP ${res.status}` };
      }
      const err = json.error || json.Error;
      if (err) {
        return { success: false, error: String(err) };
      }
      return { success: true };
    } else {
      let url = `${URLS.authRegister(serverIp)}?email=${encodeURIComponent(email)}&password=${encodeURIComponent(hashedPassword)}`;
      if (ticket) url += `&inviteTicket=${encodeURIComponent(ticket)}`;
      const res = await tauriFetch(url);

      const errorMatch = res.body.match(/<Description>(.+?)<\/Description>/);
      if (errorMatch?.[1]) {
        return { success: false, error: errorMatch[1] };
      }
      return { success: true };
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

export async function recoverPassword(
  serverIp: string,
  email: string
): Promise<{ success: boolean; message: string }> {
  try {
    const res = await tauriFetch(URLS.recoveryPassword(serverIp), {
      method: "POST",
      contentType: "application/x-www-form-urlencoded",
      body: `email=${encodeURIComponent(email)}`,
    });

    const body = res.body.toUpperCase();
    if (body.includes("INVALID EMAIL") || body.includes("INVALID")) {
      return { success: true, message: "If an account exists with that email, a recovery link has been sent." };
    }
    if (body.includes("RECOVERY PASSWORD LINK ALREADY SENT") || body.includes("ALREADY SENT")) {
      return { success: true, message: "A recovery link has already been sent. Please check your spam folder." };
    }
    if (body.includes("RESET PASSWORD SENT TO") || body.includes("SENT TO")) {
      return { success: true, message: "A password reset link has been sent to your email." };
    }
    return { success: true, message: res.body };
  } catch (err) {
    return {
      success: false,
      message: err instanceof Error ? err.message : String(err),
    };
  }
}

export async function pingServer(serverIp: string): Promise<number> {
  return invoke<number>("ping_server", { serverIp });
}

export async function launchGame(
  gamePath: string,
  serverId: string,
  serverName: string,
  serverIp: string,
  loginToken: string,
  userId: string,
  discordAppId?: string,
  closeOnExit?: boolean,
  disableProxy?: boolean
): Promise<void> {
  const params = {
    gamePath,
    serverId,
    serverName,
    serverIp,
    loginToken,
    userId: String(userId),
    discordAppId: discordAppId || null,
    closeOnExit: closeOnExit ?? false,
    disableProxy: disableProxy ?? false,
  };
  return invoke("launch_game", params);
}

export async function checkFileExists(path: string): Promise<boolean> {
  return invoke<boolean>("check_file_exists", { path });
}

export async function checkProcessRunning(processName: string): Promise<boolean> {
  return invoke<boolean>("check_process_running", { processName });
}

export async function removeServerMods(gamePath: string): Promise<void> {
  return invoke<void>("remove_server_mods", { gamePath });
}

export async function readSettingsFile(path: string): Promise<string> {
  return invoke<string>("read_settings_file", { path });
}

export async function writeSettingsFile(
  path: string,
  content: string
): Promise<void> {
  return invoke("write_settings_file", { path, content });
}

export async function hashFileMd5(path: string): Promise<string> {
  return invoke<string>("hash_file_md5", { path });
}

export async function listGameFiles(directory: string): Promise<string[]> {
  return invoke<string[]>("list_game_files", { directory });
}

export interface SystemInfo {
  os_name: string;
  os_version: string;
  kernel_version: string;
  hostname: string;
  cpu_brand: string;
  cpu_cores: number;
  total_memory: number;
  used_memory: number;
  total_swap: number;
  used_swap: number;
  gpu_name: string;
  gpu_driver: string;
  disk_free: number;
  disk_total: number;
  disk_kind: string;
}

export async function getSystemInfo(): Promise<SystemInfo> {
  return invoke<SystemInfo>("get_system_info");
}

export async function pickGameFolder(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Select game folder",
  });
  return selected as string | null;
}

export async function validateGameFolder(nfsw_path: string): Promise<boolean> {
  return invoke<boolean>("check_file_exists", { path: path.join(nfsw_path, "nfsw.exe") });
}

export async function fetchCDNListRaw(): Promise<CDNEntry[]> {
  const body = await invoke<string>("fetch_cdn_list", { apiUrl: URLS.API_MAIN });
  try {
    return JSON.parse(body) as CDNEntry[];
  } catch {
    return [];
  }
}

export async function downloadGame(
  cdnUrl: string,
  gamePath: string
): Promise<void> {
  return invoke("download_game", { cdnUrl, gamePath });
}

export async function verifyGameFiles(
  cdnUrl: string,
  gamePath: string
): Promise<string[]> {
  return invoke<string[]>("verify_game_files", { cdnUrl, gamePath });
}

export async function repairGameFiles(
  cdnUrl: string,
  gamePath: string,
  corruptedFiles: string[]
): Promise<void> {
  return invoke("repair_game_files", { cdnUrl, gamePath, corruptedFiles });
}

export interface ModInfo {
  base_path: string | null;
  server_id: string | null;
  features: unknown[] | null;
}

export async function downloadModNetModules(
  gamePath: string,
  modnetCdn: string
): Promise<void> {
  return invoke("download_modnet_modules", { gamePath, modnetCdn });
}

export async function fetchModInfo(
  serverIp: string
): Promise<ModInfo | null> {
  return invoke<ModInfo | null>("fetch_mod_info", { serverIp });
}

export async function downloadMods(
  basePath: string,
  serverId: string,
  gamePath: string
): Promise<void> {
  return invoke("download_mods", { basePath, serverId, gamePath });
}

export async function hasPendingModCleanup(gamePath: string): Promise<boolean> {
  return invoke<boolean>("has_pending_mod_cleanup", { gamePath });
}

export async function cleanMods(gamePath: string): Promise<void> {
  return invoke("clean_mods", { gamePath });
}

export async function grantFolderPermissions(path: string): Promise<void> {
  return invoke("grant_folder_permissions", { path });
}

export async function setGameLanguage(language: string): Promise<void> {
  return invoke("set_game_language", { language });
}

export async function getGameLanguage(): Promise<string> {
  return invoke("get_game_language");
}

export async function getHwidInfo(): Promise<[string, string]> {
  return invoke("get_hwid_info");
}

export interface GameSettings {
  screen_width: number;
  screen_height: number;
  screen_windowed: boolean;
  brightness: number;
  vsync: boolean;
  performance_level: number;
  
  base_texture_filter: number;
  base_texture_max_anisotropy: number;
  road_texture_filter: number;
  road_texture_max_anisotropy: number;
  car_environment_map: number;
  global_detail_level: number;
  road_reflection: number;
  shader_detail: number;
  shadow_detail: number;
  
  fsaa_level: number;
  motion_blur: boolean;
  particle_system: boolean;
  post_processing: boolean;
  rain: boolean;
  water_sim: boolean;
  visual_treatment: boolean;
  max_skid_marks: number;
  
  audio_mode: number;
  audio_quality: number;
  master_volume: number;
  sfx_volume: number;
  car_volume: number;
  speech_volume: number;
  music_volume: number;
  frontend_music_volume: number;
  
  camera: number;
  transmission: number;
  damage: boolean;
  speed_units: number;
}

export async function getGameSettings(): Promise<GameSettings> {
  return invoke("get_game_settings");
}

export async function setGameSettings(settings: GameSettings): Promise<void> {
  return invoke("set_game_settings", { settings });
}

export async function discordRpcInit(): Promise<void> {
  return invoke("discord_rpc_init");
}

export async function discordRpcUpdate(params: {
  state?: string;
  details?: string;
  largeImage?: string;
  largeText?: string;
  smallImage?: string;
  smallText?: string;
  button1Label?: string;
  button1Url?: string;
  button2Label?: string;
  button2Url?: string;
}): Promise<void> {
  return invoke("discord_rpc_update", {
    state: params.state,
    details: params.details,
    largeImage: params.largeImage,
    largeText: params.largeText,
    smallImage: params.smallImage,
    smallText: params.smallText,
    button1Label: params.button1Label,
    button1Url: params.button1Url,
    button2Label: params.button2Label,
    button2Url: params.button2Url,
  });
}

export async function discordRpcReconnect(appId?: string): Promise<void> {
  return invoke("discord_rpc_reconnect", { appId });
}

export async function discordRpcStop(): Promise<void> {
  return invoke("discord_rpc_stop");
}
