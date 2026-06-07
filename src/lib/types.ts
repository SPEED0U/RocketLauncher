export interface ServerInfo {
  id: string;
  name: string;
  ip: string;
  category: string;
  bannerUrl?: string;
  iconUrl?: string;
  onlineCount?: number;
  registeredCount?: number;
  maxOnline?: number;
  ping?: number;
  isOfficial?: boolean;
  discordAppId?: string;
  enforceLauncherProxy?: boolean;
  requireTicket?: boolean;
  homePageUrl?: string;
  discordUrl?: string;
  country?: string;
}

export interface ServerListEntry {
  id: string;
  name: string;
  ip_address: string;
  category: string;
  banner_url?: string;
  icon_url?: string;
  number_of_registered?: number;
  is_special?: boolean;
  discord_presence_key?: string;
  discord_application_id?: string;
  country?: string;
  distribution_url?: string | null;
  forceUserAgent?: string | null;
}

export interface ServerDetails {
  serverName: string;
  serverVersion: string;
  time?: number | string;
  homePageUrl: string;
  discordUrl: string;
  requireTicket: boolean;
  enforceLauncherProxy: boolean;
  allowedCountries: string;
  activatedHolidaySceneryGroups: string[];
  disactivatedHolidaySceneryGroups: string[];
  numberOfRegistered: number;
  onlineNumber: number;
  maxUsersAllowed: number;
  cashRewardMultiplier?: number;
  repRewardMultiplier?: number;
  webSignupUrl?: string;
  webRecoveryUrl?: string;
  modernAuthSupport?: boolean;
  authHash?: string;
  discordApplicationID?: string;
}

export interface CDNEntry {
  name: string;
  url: string;
  category?: string;
  isActive?: boolean;
}

export interface UserCredentials {
  email: string;
  password: string;
  rememberMe?: boolean;
}

export interface UserSession {
  userId: string;
  token: string;
  serverIp: string;
}

export interface RegisterData {
  email: string;
  password: string;
  repeatPassword: string;
  ticket?: string;
}

export interface DownloadProgress {
  fileName: string;
  currentFile: number;
  totalFiles: number;
  downloadedBytes: number;
  totalBytes: number;
  speed: number;
  status: DownloadStatus;
  eta?: number;
}

export type DownloadStatus =
  | "idle"
  | "downloading"
  | "extracting"
  | "verifying"
  | "completed"
  | "error"
  | "paused";

export interface VerifyHashProgress {
  currentFile: string;
  currentIndex: number;
  totalFiles: number;
  corruptedFiles: string[];
  status: "idle" | "scanning" | "repairing" | "completed" | "error";
}

export interface LauncherSettings {
  installationDirectory: string;
  selectedCDN: string;
  language: string;
  disableProxy: boolean;
  disableRPC: boolean;
  streamingSupport: boolean;
  themeSupport: boolean;
  insider: boolean;
  ignoreUpdateVersion: string;
  firewallStatus: "not_checked" | "enabled" | "disabled" | "error";
  defenderStatus: "not_checked" | "excluded" | "not_excluded" | "error";
  closeOnGameExit: boolean;
  disableSlideshow: boolean;
  permissionsGrantedFor: string;
}

export type LauncherPage =
  | "splash"
  | "welcome"
  | "main"
  | "servers"
  | "settings"
  | "security"
  | "verify"
  | "debug"
  | "register"
  | "usx-editor";

export interface LauncherVersion {
  current: string;
  latest: string;
  latestUrl: string;
  updateAvailable: boolean;
}

export interface LanguageEntry {
  code: string;
  name: string;
  nativeName: string;
}
