import type { LauncherSettings } from "./types";
import { getVersion } from "@tauri-apps/api/app";

export const APP_NAME = "RocketLauncher";
export const APP_VERSION = "Loading...";

let cachedVersion: string | null = null;

export function formatVersionForDisplay(version: string): string {
  const v = version.trim();
  const betaMatch = v.match(/^(\d+\.\d+\.\d+)-beta(?:[.-]?(\d+))?$/i);
  if (betaMatch) {
    return `${betaMatch[1]}b`;
  }
  return v;
}

export async function getAppVersion(): Promise<string> {
  if (cachedVersion) return cachedVersion;
  try {
    cachedVersion = await getVersion();
    return cachedVersion;
  } catch (error) {
    console.error("Failed to get app version:", error);
    return "Unknown";
  }
}

export async function getDisplayAppVersion(): Promise<string> {
  const rawVersion = await getAppVersion();
  return formatVersionForDisplay(rawVersion);
}

export const DEFAULT_SETTINGS: LauncherSettings = {
  installationDirectory: "",
  selectedCDN: "https://cdn.worldunited.gg",
  language: "EN",
  disableProxy: false,
  disableRPC: false,
  streamingSupport: false,
  themeSupport: false,
  insider: false,
  ignoreUpdateVersion: "",
  firewallStatus: "not_checked",
  defenderStatus: "not_checked",
  closeOnGameExit: false,
  disableSlideshow: false,
  permissionsGrantedFor: "",
};

export const DOWNLOAD_CONFIG = {
  maxThreads: 3,
  chunkCount: 16,
  retryAttempts: 3,
  retryDelay: 2000,
};
