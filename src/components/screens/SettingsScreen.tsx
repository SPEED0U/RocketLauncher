"use client";

import { Button } from "@/components/ui/Button";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  Save,
  FolderOpen,
  Globe,
  MessageSquare,
  Gamepad2,
  Monitor,
  Shield,
  Palette,
  Eye,
  Download,
  CheckCircle,
  AlertTriangle,
  Sliders,
  Cpu,
  ImageOff,
  Layers,
} from "lucide-react";
import { useState, useEffect, useRef } from "react";
import { pickGameFolder, validateGameFolder, fetchCDNListRaw, verifyGameFiles, repairGameFiles, getGameLanguage, setGameLanguage } from "@/lib/tauri-api";
import { isCloudDrivePath } from "@/lib/utils";
import { useLauncherStore } from "@/stores/launcherStore";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { Select } from "@/components/ui/Select";
import { Modal } from "@/components/ui/Modal";
import { GameSettingsEditor } from "./GameSettingsEditor";
import type { CDNEntry } from "@/lib/types";

function Toggle({
  checked,
  onChange,
  label,
  description,
  icon,
}: {
  checked: boolean;
  onChange: (val: boolean) => void;
  label: string;
  description?: string;
  icon?: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between py-3">
      <div className="flex items-center gap-3">
        {icon && <span className="text-muted">{icon}</span>}
        <div>
          <p className="text-sm font-medium">{label}</p>
          {description && (
            <p className="text-xs text-muted mt-0.5">{description}</p>
          )}
        </div>
      </div>
      <button
        onClick={() => onChange(!checked)}
        className="relative w-11 h-6 rounded-full cursor-pointer active:scale-90"
        style={{
          backgroundColor: checked ? "var(--color-primary)" : "var(--color-border)",
          boxShadow: checked
            ? "0 0 12px 3px color-mix(in srgb, var(--color-primary) 45%, transparent)"
            : "0 0 0px 0px transparent",
          transition: "background-color 300ms ease, box-shadow 300ms ease, transform 150ms ease",
        }}
      >
        <span
          className="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white shadow-sm"
          style={{
            transform: checked ? "translateX(1.25rem)" : "translateX(0)",
            transition: "transform 300ms cubic-bezier(0.34, 1.56, 0.64, 1)",
          }}
        />
      </button>
    </div>
  );
}

export function SettingsScreen() {
  const { settings, setSettings, resetSettings } = useSettingsStore();
  const [showProxyWarning, setShowProxyWarning] = useState(false);
  const { isAutoVerifying, setAutoVerifying } = useLauncherStore();
  const [saved, setSaved] = useState(false);
  const [folderStatus, setFolderStatus] = useState<"unknown" | "valid" | "invalid" | "cloud">("unknown");
  const [cdnList, setCdnList] = useState<CDNEntry[]>([]);
  const [autoVerifyStatus, setAutoVerifyStatus] = useState<"idle" | "verifying" | "repairing" | "done" | "error">("idle");
  const [autoVerifyMessage, setAutoVerifyMessage] = useState("");
  const [verifyPercent, setVerifyPercent] = useState(0);
  const [verifyCurrentFile, setVerifyCurrentFile] = useState("");
  const [gameSettingsOpen, setGameSettingsOpen] = useState(false);
  const [dxvkInstalled, setDxvkInstalled] = useState<boolean | null>(null);
  const [dxvkVersion, setDxvkVersion] = useState<string | null>(null);
  const [dxvkLoading, setDxvkLoading] = useState(false);
  const [dxvkError, setDxvkError] = useState("");
  const [isWindows, setIsWindows] = useState<boolean | null>(null);
  const [systemLoaded, setSystemLoaded] = useState(false);
  const [homeDir, setHomeDir] = useState<string>("");
  const unlistenRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    fetchCDNListRaw().then(setCdnList).catch(() => {});
    getGameLanguage().then((lang) => {
      if (lang && lang !== settings.language) {
        setSettings({ language: lang });
      }
    }).catch(() => {});

    (async () => {
      try {
        const { getSystemInfo } = await import("@/lib/tauri-api");
        const { homeDir } = await import("@tauri-apps/api/path");
        const sysInfo = await getSystemInfo();
        const home = await homeDir();
        
        const isWindows = sysInfo.os_name.toLowerCase().includes("windows");
        setIsWindows(isWindows);
        setHomeDir(home);
        setSystemLoaded(true);
      } catch {
        setIsWindows(false);
        setHomeDir("");
        setSystemLoaded(true);
      }
    })();
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<{
          status: string;
          current_file: string;
          current_index: number;
          total_files: number;
          corrupted_count: number;
        }>("verify-progress", (event) => {
          if (cancelled) return;
          const d = event.payload;
          const pct = d.total_files > 0 ? (d.current_index / d.total_files) * 100 : 0;
          setVerifyPercent(pct);
          setVerifyCurrentFile(d.current_file);
          if (d.total_files > 0) {
            setAutoVerifyMessage(`${d.current_index} / ${d.total_files} files`);
          }
        });
        unlistenRef.current = unlisten;
      } catch {
      }
    })();
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, []);

  const prevDir = useRef(settings.installationDirectory);

  useEffect(() => {
    if (settings.installationDirectory) {
      validateGameFolder(settings.installationDirectory).then((valid) => {
        setFolderStatus(valid ? "valid" : "invalid");

        const dirChanged = prevDir.current !== settings.installationDirectory;
        prevDir.current = settings.installationDirectory;
        if (dirChanged && valid && settings.selectedCDN) {
          runAutoVerify(settings.installationDirectory, settings.selectedCDN);
        }
      });
    } else {
      setFolderStatus("unknown");
    }
  }, [settings.installationDirectory]);

  async function runAutoVerify(gamePath: string, cdnUrl: string) {
    setAutoVerifyStatus("verifying");
    setAutoVerifyMessage("Scanning game files...");
    setVerifyPercent(0);
    setVerifyCurrentFile("");
    setAutoVerifying(true);
    try {
      const corrupted = await verifyGameFiles(cdnUrl, gamePath);
      if (corrupted.length === 0) {
        setAutoVerifyStatus("done");
        setAutoVerifyMessage("All files verified — no issues found.");
        setVerifyPercent(100);
      } else {
        setAutoVerifyStatus("repairing");
        setAutoVerifyMessage(`Found ${corrupted.length} corrupted file${corrupted.length > 1 ? "s" : ""}, repairing...`);
        setVerifyPercent(0);
        await repairGameFiles(cdnUrl, gamePath, corrupted);
        setAutoVerifyStatus("done");
        setAutoVerifyMessage(`Repaired ${corrupted.length} file${corrupted.length > 1 ? "s" : ""} successfully.`);
        setVerifyPercent(100);
      }
    } catch (err) {
      setAutoVerifyStatus("error");
      setAutoVerifyMessage(err instanceof Error ? err.message : String(err));
    } finally {
      setAutoVerifying(false);
    }
  }

  async function checkDxvk() {
    if (!settings.installationDirectory) return;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const status = await invoke<{ installed: boolean; version: string | null }>("check_dxvk", { gamePath: settings.installationDirectory });
      setDxvkInstalled(status.installed);
      setDxvkVersion(status.version);
    } catch {
      setDxvkInstalled(false);
    }
  }

  useEffect(() => { checkDxvk(); }, [settings.installationDirectory]);

  async function handleInstallDxvk() {
    if (!settings.installationDirectory) return;
    setDxvkLoading(true);
    setDxvkError("");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("install_dxvk", { gamePath: settings.installationDirectory });
      await checkDxvk();
    } catch (e) {
      setDxvkError(e instanceof Error ? e.message : String(e));
    } finally {
      setDxvkLoading(false);
    }
  }

  async function handleRemoveDxvk() {
    if (!settings.installationDirectory) return;
    setDxvkLoading(true);
    setDxvkError("");
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("remove_dxvk", { gamePath: settings.installationDirectory });
      await checkDxvk();
    } catch (e) {
      setDxvkError(e instanceof Error ? e.message : String(e));
    } finally {
      setDxvkLoading(false);
    }
  }

  async function handlePickFolder() {
    const folder = await pickGameFolder();
    if (folder) {
      if (isCloudDrivePath(folder)) {
        setFolderStatus("cloud");
        return;
      }
      setSettings({ installationDirectory: folder });
    }
  }

  async function handleSave() {
    try {
      await setGameLanguage(settings.language);
    } catch {}
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  }

  return (
    <div className="flex-1 relative flex items-center justify-center p-4 overflow-hidden">
      {!systemLoaded ? (
        <div className="flex flex-col items-center justify-center gap-3">
          <div className="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin" />
          <p className="text-xs text-muted">Loading system info...</p>
        </div>
      ) : (
      <>
      <div className="absolute top-4 left-4 right-4 flex items-center justify-between">
        <h1 className="text-base font-bold">Settings</h1>
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="sm" onClick={resetSettings} disabled={isAutoVerifying} className="h-7 px-3 text-[11px]">
            Reset
          </Button>
          <Button size="sm" onClick={handleSave} disabled={isAutoVerifying} className="h-7 px-3 text-[11px]">
            <Save size={11} className="mr-1" />
            {saved ? "Saved!" : "Save"}
          </Button>
        </div>
      </div>
      <div className="w-full grid grid-cols-3 gap-3">
        <section className="col-span-2 border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <FolderOpen size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Installation</h2>
              <p className="text-[10px] text-muted">Game directory and file integrity</p>
            </div>
          </div>
          <div className="px-4 py-3 space-y-3 flex-1">
            <div>
              <label className="text-[11px] text-muted block mb-1.5">Game Directory</label>
              <div className="flex gap-2 items-center">
                <input
                  type="text"
                  value={settings.installationDirectory}
                  onChange={(e) => {
                    const val = e.target.value;
                    if (isCloudDrivePath(val)) {
                      setFolderStatus("cloud");
                    } else {
                      if (folderStatus === "cloud") setFolderStatus("unknown");
                      setSettings({ installationDirectory: val });
                    }
                  }}
                  placeholder={isWindows ? "C:\\Games\\NFSW" : `${homeDir}/Games/NFSW`}
                  className="flex-1 rounded-lg border border-border bg-background/50 px-3 py-1.5 text-xs text-foreground placeholder:text-muted/50 focus:outline-none focus:ring-2 focus:ring-primary/30 focus:border-primary/50"
                />
                <Button variant="secondary" size="sm" onClick={handlePickFolder} disabled={isAutoVerifying} className="h-7 px-2.5">
                  <FolderOpen size={13} />
                </Button>
              </div>
              {folderStatus === "valid" && (
                <p className="text-[10px] text-success mt-1 flex items-center gap-1">
                  <CheckCircle size={10} /> nfsw.exe found
                </p>
              )}
              {folderStatus === "invalid" && settings.installationDirectory && (
                <p className="text-[10px] text-accent mt-1 flex items-center gap-1">
                  <AlertTriangle size={10} /> nfsw.exe not found — game will be downloaded
                </p>
              )}
              {folderStatus === "cloud" && (
                <p className="text-[10px] text-danger mt-1 flex items-center gap-1">
                  <AlertTriangle size={10} /> Cloud storage folders (OneDrive, Google Drive…) are not supported
                </p>
              )}
            </div>
            {(autoVerifyStatus === "verifying" || autoVerifyStatus === "repairing") && (
              <div className="space-y-1.5">
                <div className="flex items-center gap-1.5">
                  {autoVerifyStatus === "repairing"
                    ? <Download size={11} className="text-accent shrink-0 animate-pulse" />
                    : <Shield size={11} className="text-primary shrink-0" />}
                  <p className={`text-[11px] font-medium ${autoVerifyStatus === "repairing" ? "text-accent" : "text-primary"}`}>
                    {autoVerifyMessage}
                  </p>
                </div>
                <ProgressBar value={verifyPercent} variant={autoVerifyStatus === "repairing" ? "accent" : "primary"} size="sm" showPercent />
                {verifyCurrentFile && (
                  <p className="text-[10px] text-muted truncate font-mono">{verifyCurrentFile}</p>
                )}
              </div>
            )}
            {autoVerifyStatus === "done" && (
              <p className="text-[10px] text-success flex items-center gap-1">
                <CheckCircle size={10} /> {autoVerifyMessage}
              </p>
            )}
            {autoVerifyStatus === "error" && (
              <p className="text-[10px] text-danger flex items-center gap-1">
                <AlertTriangle size={10} /> {autoVerifyMessage}
              </p>
            )}
          </div>
        </section>
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <Globe size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">CDN & Network</h2>
              <p className="text-[10px] text-muted">Download server and proxy</p>
            </div>
          </div>
          <div className="px-4 py-3 space-y-3 flex-1">
            <div>
              <label className="text-[11px] text-muted block mb-1.5">Download CDN</label>
              <Select
                value={settings.selectedCDN}
                onChange={(val) => setSettings({ selectedCDN: val })}
                placeholder="Select a CDN..."
                options={[
                  { value: "", label: "Select a CDN..." },
                  ...cdnList.map((cdn) => ({ value: cdn.url, label: cdn.name }))
                ]}
              />
            </div>
            <Toggle
              checked={!settings.disableProxy}
              onChange={(val) => {
                if (val === false) {
                  setShowProxyWarning(true);
                } else {
                  setSettings({ disableProxy: !val });
                }
              }}
              label="Launcher Proxy"
              description="Route game traffic through local proxy"
              icon={<Shield size={13} />}
            />
            <Modal
              isOpen={showProxyWarning}
              onClose={() => setShowProxyWarning(false)}
              title="Warning: Proxy Disabled"
              size="sm"
            >
              <div className="flex items-start gap-3">
                <AlertTriangle size={22} className="text-danger mt-1 shrink-0" />
                <div>
                  <p className="text-danger font-semibold mb-1">Some servers require the proxy to connect.</p>
                  <p className="text-xs mb-2">
                    Disabling the proxy will prevent the launcher from connecting to servers that use the <b>HTTPS</b> protocol.<br />
                    <b>Recommended:</b> Keep the proxy enabled unless you know your server supports direct connection.
                  </p>
                  <div className="flex gap-2 justify-end mt-2">
                    <Button size="sm" variant="secondary" onClick={() => setShowProxyWarning(false)}>
                      Cancel
                    </Button>
                    <Button size="sm" variant="danger" onClick={() => {
                      setSettings({ disableProxy: true });
                      setShowProxyWarning(false);
                    }}>
                      Disable Proxy Anyway
                    </Button>
                  </div>
                </div>
              </div>
            </Modal>
          </div>
        </section>
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <Palette size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Launcher Settings</h2>
              <p className="text-[10px] text-muted">Appearance and behaviour</p>
            </div>
          </div>
          <div className="px-4 py-3 divide-y divide-border/50 flex-1">
            <Toggle
              checked={settings.closeOnGameExit}
              onChange={(val) => setSettings({ closeOnGameExit: val })}
              label="Close on Game Exit"
              description="Close launcher when the game exits"
              icon={<Monitor size={13} />}
            />
            <Toggle
              checked={settings.disableSlideshow}
              onChange={(val) => setSettings({ disableSlideshow: val })}
              label="Static Background"
              description="Random image instead of slideshow"
              icon={<ImageOff size={13} />}
            />
            <div className="opacity-40 pointer-events-none select-none">
              <Toggle
                checked={settings.themeSupport}
                onChange={(val) => setSettings({ themeSupport: val })}
                label="Custom Themes"
                description="Coming soon"
                icon={<Palette size={13} />}
              />
            </div>
          </div>
        </section>
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <Gamepad2 size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Features</h2>
              <p className="text-[10px] text-muted">Integrations and extras</p>
            </div>
          </div>
          <div className="px-4 py-3 divide-y divide-border/50 flex-1">
            <Toggle
              checked={!settings.disableRPC}
              onChange={(val) => setSettings({ disableRPC: !val })}
              label="Discord Rich Presence"
              description="Show game status in Discord"
              icon={<MessageSquare size={13} />}
            />
            <Toggle
              checked={settings.streamingSupport}
              onChange={(val) => setSettings({ streamingSupport: val })}
              label="Streaming Mode"
              description="Optimized for video capture"
              icon={<Monitor size={13} />}
            />
            <Toggle
              checked={settings.insider}
              onChange={(val) => setSettings({ insider: val })}
              label="Insider Program"
              description="Receive beta updates"
              icon={<Eye size={13} />}
            />
          </div>
        </section>
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <Sliders size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Game Settings</h2>
              <p className="text-[10px] text-muted">Language, graphics, audio and gameplay</p>
            </div>
          </div>
          <div className="flex-1 flex flex-col">
            <div className="px-4 py-3 divide-y divide-border/50">
              <div className="flex items-center justify-between py-3">
                <div className="flex items-center gap-3">
                  <span className="text-muted"><Globe size={13} /></span>
                  <div>
                    <p className="text-sm font-medium">Game Language</p>
                    <p className="text-xs text-muted mt-0.5">In-game language</p>
                  </div>
                </div>
                <Select
                  value={settings.language}
                  onChange={(val) => setSettings({ language: val })}
                  className="w-32"
                  options={[
                    { value: "EN", label: "English" },
                    { value: "DE", label: "Deutsch" },
                    { value: "ES", label: "Español" },
                    { value: "FR", label: "Français" },
                    { value: "PL", label: "Polski" },
                    { value: "PT", label: "Português" },
                    { value: "RU", label: "Русский" },
                    { value: "TC", label: "繁體中文" },
                    { value: "SC", label: "简体中文" },
                    { value: "TH", label: "ภาษาไทย" },
                    { value: "TR", label: "Türkçe" },
                  ]}
                />
              </div>
              {isWindows && (
                <>
                  <div className="flex items-center justify-between py-3">
                    <div className="flex items-center gap-3">
                      <span className={dxvkInstalled ? "text-success" : "text-muted"}>
                        <Cpu size={13} />
                      </span>
                      <div>
                        <div className="flex items-center gap-1.5">
                          <p className="text-sm font-medium">DXVK</p>
                          {dxvkInstalled && (
                            <span className="text-[9px] font-mono text-muted">v{dxvkVersion ?? "2.4"}</span>
                          )}
                        </div>
                        <p className="text-xs text-muted mt-0.5">
                          {dxvkInstalled ? "DX9 → Vulkan active" : "DX9 → Vulkan translation"}
                        </p>
                      </div>
                    </div>
                    {dxvkInstalled ? (
                      <Button variant="ghost" size="sm" onClick={handleRemoveDxvk} isLoading={dxvkLoading}
                        disabled={dxvkLoading || !settings.installationDirectory}
                        className="h-7 px-2.5 text-[11px] text-danger hover:bg-danger/10">
                        Remove
                      </Button>
                    ) : (
                      <Button variant="secondary" size="sm" onClick={handleInstallDxvk} isLoading={dxvkLoading}
                        disabled={dxvkLoading || !settings.installationDirectory || folderStatus === "cloud"}
                        className="h-7 px-2.5 text-[11px]">
                        {dxvkLoading ? "Installing..." : "Install"}
                      </Button>
                    )}
                  </div>

                  {dxvkError && (
                    <p className="text-[10px] text-danger flex items-center gap-1 py-2">
                      <AlertTriangle size={10} /> {dxvkError}
                    </p>
                  )}
                  {folderStatus === "cloud" && (
                    <p className="text-[10px] text-danger flex items-center gap-1 py-2">
                      <AlertTriangle size={10} /> DXVK cannot be installed in a cloud storage folder
                    </p>
                  )}
                </>
              )}

              {isWindows == false && (
                            <div className="flex items-center justify-between py-3">
                <div className="flex items-center gap-3">
                  <span className="text-muted"><Layers size={13} /></span>
                  <div>
                    <p className="text-sm font-medium">Windows Layer</p>
                    <p className="text-xs text-muted mt-0.5">Game compatibility environment</p>
                  </div>
                </div>
                <Select
                  value="wine"
                  onChange={(val) => setWindowsLayer({ language: val })}
                  className="w-32"
                  options={[
                    { value: "wine", label: "Wine" },
                    { value: "proton", label: "Proton" },
                  ]}
                />
              </div>
              )}

              <div className="py-3">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => setGameSettingsOpen(true)}
                  className="w-full h-9 text-xs"
                >
                  <Sliders size={11} className="mr-1.5" />
                  Advanced Settings Editor
                </Button>
              </div>
            </div>
          </div>
        </section>

      </div>

      <GameSettingsEditor
        isOpen={gameSettingsOpen}
        onClose={() => setGameSettingsOpen(false)}
      />
      </>
      )}
    </div>
  );
}
