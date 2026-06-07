import { invoke } from "@tauri-apps/api/core";

export interface UpdateInfo {
  version: string;
  exe: string;
  publishDate: string;
  productName: string;
}

export async function checkForUpdates(betaMode?: boolean): Promise<UpdateInfo | null> {
  return invoke<UpdateInfo | null>(betaMode ? "check_for_beta_updates" : "check_for_updates");
}

export async function downloadUpdate(exeName: string): Promise<string> {
  return invoke<string>("download_update", { exeName });
}

export async function installUpdate(installerPath: string): Promise<void> {
  return invoke<void>("install_update", { installerPath });
}
