export type { SystemInfo, GameSettings } from "./tauri-api";

async function getTauriApi() {
  return await import("./tauri-api");
}

export async function fetchServerList() {
  const tauri = await getTauriApi();
  return tauri.fetchServerList();
}

export async function fetchServerDetails(serverIp: string) {
  const tauri = await getTauriApi();
  return tauri.fetchServerDetails(serverIp);
}

export async function fetchCDNList() {
  const tauri = await getTauriApi();
  return tauri.fetchCDNList();
}

export async function loginToServer(
  serverIp: string,
  email: string,
  password: string,
  modernAuth?: boolean,
  authHash?: string
) {
  const tauri = await getTauriApi();
  return tauri.loginToServer(serverIp, email, password, modernAuth, authHash);
}

export async function registerOnServer(
  serverIp: string,
  email: string,
  password: string,
  ticket?: string,
  modernAuth?: boolean,
  authHash?: string
) {
  const tauri = await getTauriApi();
  return tauri.registerOnServer(serverIp, email, password, ticket, modernAuth, authHash);
}

export async function recoverPassword(serverIp: string, email: string) {
  const tauri = await getTauriApi();
  return tauri.recoverPassword(serverIp, email);
}

export async function pingServer(serverIp: string) {
  const tauri = await getTauriApi();
  return tauri.pingServer(serverIp);
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
  disableProxy?: boolean,
  serverCategory?: string
) {
  const tauri = await getTauriApi();
  return tauri.launchGame(gamePath, serverId, serverName, serverIp, loginToken, userId, discordAppId, closeOnExit, disableProxy, serverCategory);
}

export async function checkFileExists(path: string) {
  const tauri = await getTauriApi();
  return tauri.checkFileExists(path);
}

export async function checkProcessRunning(processName: string) {
  const tauri = await getTauriApi();
  return tauri.checkProcessRunning(processName);
}

export async function removeServerMods(gamePath: string) {
  const tauri = await getTauriApi();
  return tauri.removeServerMods(gamePath);
}

export async function hashFileMd5(path: string) {
  const tauri = await getTauriApi();
  return tauri.hashFileMd5(path);
}

export async function listGameFiles(directory: string) {
  const tauri = await getTauriApi();
  return tauri.listGameFiles(directory);
}

export async function getSystemInfo() {
  const tauri = await getTauriApi();
  return tauri.getSystemInfo();
}

export async function pickGameFolder() {
  const tauri = await getTauriApi();
  return tauri.pickGameFolder();
}

export async function validateGameFolder(path: string) {
  const tauri = await getTauriApi();
  return tauri.validateGameFolder(path);
}

export async function fetchCDNListRaw() {
  const tauri = await getTauriApi();
  return tauri.fetchCDNListRaw();
}

export async function downloadGame(cdnUrl: string, gamePath: string) {
  const tauri = await getTauriApi();
  return tauri.downloadGame(cdnUrl, gamePath);
}

export async function verifyGameFiles(cdnUrl: string, gamePath: string) {
  const tauri = await getTauriApi();
  return tauri.verifyGameFiles(cdnUrl, gamePath);
}

export async function repairGameFiles(cdnUrl: string, gamePath: string, corruptedFiles: string[]) {
  const tauri = await getTauriApi();
  return tauri.repairGameFiles(cdnUrl, gamePath, corruptedFiles);
}

export async function downloadModNetModules(gamePath: string, modnetCdn: string) {
  const tauri = await getTauriApi();
  return tauri.downloadModNetModules(gamePath, modnetCdn);
}

export async function fetchModInfo(serverIp: string) {
  const tauri = await getTauriApi();
  return tauri.fetchModInfo(serverIp);
}

export async function downloadMods(basePath: string, serverId: string, gamePath: string) {
  const tauri = await getTauriApi();
  return tauri.downloadMods(basePath, serverId, gamePath);
}

export async function cleanMods(gamePath: string) {
  const tauri = await getTauriApi();
  return tauri.cleanMods(gamePath);
}

export async function grantFolderPermissions(path: string) {
  const tauri = await getTauriApi();
  return tauri.grantFolderPermissions(path);
}

export async function setGameLanguage(language: string) {
  const tauri = await getTauriApi();
  return tauri.setGameLanguage(language);
}

export async function getGameLanguage() {
  const tauri = await getTauriApi();
  return tauri.getGameLanguage();
}

export async function getHwidInfo() {
  const tauri = await getTauriApi();
  return tauri.getHwidInfo();
}

export async function getGameSettings() {
  const tauri = await getTauriApi();
  return tauri.getGameSettings();
}

export async function setGameSettings(settings: import("./tauri-api").GameSettings) {
  const tauri = await getTauriApi();
  return tauri.setGameSettings(settings);
}

export async function discordRpcInit() {
  const tauri = await getTauriApi();
  return tauri.discordRpcInit();
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
}) {
  const tauri = await getTauriApi();
  return tauri.discordRpcUpdate(params);
}

export async function discordRpcReconnect(appId?: string) {
  const tauri = await getTauriApi();
  return tauri.discordRpcReconnect(appId);
}

export async function discordRpcStop() {
  const tauri = await getTauriApi();
  return tauri.discordRpcStop();
}
