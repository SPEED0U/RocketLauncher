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
  ShieldCheck,
  ShieldAlert,
  ShieldOff,
  Palette,
  Eye,
  Download,
  CheckCircle,
  CheckCircle2,
  AlertTriangle,
  AlertCircle,
  XCircle,
  Sliders,
  Cpu,
  ImageOff,
  Layers,
  Trash2,
  RefreshCw,
  Loader2,
  Flame,
  Bug,
} from "lucide-react";
import { useState, useEffect, useRef } from "react";
import { pickGameFolder, validateGameFolder, fetchCDNListRaw, verifyGameFiles, repairGameFiles, getGameLanguage, setGameLanguage, removeServerMods } from "@/lib/tauri-api";
import { isCloudDrivePath } from "@/lib/utils";
import { useLauncherStore } from "@/stores/launcherStore";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { Select } from "@/components/ui/Select";
import { Modal } from "@/components/ui/Modal";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { GameSettingsEditor } from "./GameSettingsEditor";
import { invoke } from "@tauri-apps/api/core";
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
  const [verifyPanelVisible, setVerifyPanelVisible] = useState(false);
  const [verifyPanelExiting, setVerifyPanelExiting] = useState(false);
  const verifyPanelTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [isRemovingMods, setIsRemovingMods] = useState(false);
  const [gameSettingsOpen, setGameSettingsOpen] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const [dxvkInstalled, setDxvkInstalled] = useState<boolean | null>(null);
  const [dxvkVersion, setDxvkVersion] = useState<string | null>(null);
  const [dxvkLoading, setDxvkLoading] = useState(false);
  const [dxvkError, setDxvkError] = useState("");
  const [isWindows, setIsWindows] = useState<boolean | null>(null);
  const [systemLoaded, setSystemLoaded] = useState(false);
  const [homeDir, setHomeDir] = useState<string>("");
  const unlistenRef = useRef<(() => void) | null>(null);

  // Security state
  const gamePath = settings.installationDirectory ?? "";
  const [isScanning, setIsScanning] = useState(false);
  const [firewallApiOk, setFirewallApiOk] = useState<boolean | null>(null);
  const [firewallRows, setFirewallRows] = useState<{ launcher: string; game: string }>({ launcher: "unknown", game: "unknown" });
  const [isAddingFwLauncher, setIsAddingFwLauncher] = useState(false);
  const [isAddingFwGame, setIsAddingFwGame] = useState(false);
  const [isRemovingFwLauncher, setIsRemovingFwLauncher] = useState(false);
  const [isRemovingFwGame, setIsRemovingFwGame] = useState(false);
  const [defenderApiOk, setDefenderApiOk] = useState<boolean | null>(null);
  const [defenderRows, setDefenderRows] = useState<{ launcher: string; game: string }>({ launcher: "unknown", game: "unknown" });
  const [isAddingDefLauncher, setIsAddingDefLauncher] = useState(false);
  const [isAddingDefGame, setIsAddingDefGame] = useState(false);
  const [isRemovingDefLauncher, setIsRemovingDefLauncher] = useState(false);
  const [isRemovingDefGame, setIsRemovingDefGame] = useState(false);
  const [permRows, setPermRows] = useState<{ launcher: string; game: string }>({ launcher: "unknown", game: "unknown" });
  const [isFixingPermLauncher, setIsFixingPermLauncher] = useState(false);
  const [isFixingPermGame, setIsFixingPermGame] = useState(false);
  const [secConfirmDialog, setSecConfirmDialog] = useState<{ open: boolean; title: string; message: string; onConfirm: () => void }>({ open: false, title: "", message: "", onConfirm: () => {} });

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
    if (verifyPanelTimerRef.current) clearTimeout(verifyPanelTimerRef.current);
    setVerifyPanelExiting(false);
    setVerifyPanelVisible(true);
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
      verifyPanelTimerRef.current = setTimeout(() => {
        setVerifyPanelExiting(true);
        verifyPanelTimerRef.current = setTimeout(() => {
          setVerifyPanelVisible(false);
          setVerifyPanelExiting(false);
          setAutoVerifyStatus("idle");
        }, 300);
      }, 3000);
    }
  }

  async function handleRemoveServerMods() {
    if (!settings.installationDirectory) return;
    setIsRemovingMods(true);
    try {
      await removeServerMods(settings.installationDirectory);
    } catch {}
    finally {
      setIsRemovingMods(false);
    }
  }

  async function runSecurityChecks() {
    setIsScanning(true);
    setFirewallRows({ launcher: "scanning", game: "scanning" });
    setDefenderRows({ launcher: "scanning", game: "scanning" });
    setPermRows({ launcher: "scanning", game: "scanning" });
    await Promise.allSettled([
      (async () => {
        try {
          await invoke("check_firewall_api");
          setFirewallApiOk(true);
          const r = await invoke<{ has_launcher: boolean; has_game: boolean }>("check_firewall_rules");
          setFirewallRows({ launcher: r.has_launcher ? "ok" : "missing", game: r.has_game ? "ok" : "missing" });
        } catch { setFirewallApiOk(false); setFirewallRows({ launcher: "error", game: "error" }); }
      })(),
      (async () => {
        try {
          await invoke("check_defender_api");
          setDefenderApiOk(true);
          const r = await invoke<{ has_launcher: boolean; has_game: boolean }>("check_defender_exclusions", { gamePath });
          setDefenderRows({ launcher: r.has_launcher ? "ok" : "missing", game: r.has_game ? "ok" : "missing" });
        } catch { setDefenderApiOk(false); setDefenderRows({ launcher: "error", game: "error" }); }
      })(),
      (async () => {
        try {
          const r = await invoke<{ launcher_ok: boolean; game_ok: boolean }>("check_folder_permissions", { gamePath });
          setPermRows({ launcher: r.launcher_ok ? "ok" : "missing", game: r.game_ok ? "ok" : "missing" });
        } catch { setPermRows({ launcher: "error", game: "error" }); }
      })(),
    ]);
    setIsScanning(false);
  }

  useEffect(() => { if (isWindows) runSecurityChecks(); }, [gamePath, isWindows]);

  async function addFwLauncher() { setIsAddingFwLauncher(true); try { await invoke("add_firewall_rules", { gamePath, which: "launcher" }); setFirewallRows(r => ({ ...r, launcher: "ok" })); } catch { setFirewallRows(r => ({ ...r, launcher: "error" })); } finally { setIsAddingFwLauncher(false); } }
  async function addFwGame() { setIsAddingFwGame(true); try { await invoke("add_firewall_rules", { gamePath, which: "game" }); setFirewallRows(r => ({ ...r, game: "ok" })); } catch { setFirewallRows(r => ({ ...r, game: "error" })); } finally { setIsAddingFwGame(false); } }
  function confirmRemoveFw(which: "launcher" | "game") {
    setSecConfirmDialog({ open: true, title: "Remove Firewall Rule", message: `Remove the ${which} firewall rule?`, onConfirm: async () => {
      setSecConfirmDialog(d => ({ ...d, open: false }));
      if (which === "launcher") { setIsRemovingFwLauncher(true); try { await invoke("remove_firewall_rules", { which: "launcher" }); setFirewallRows(r => ({ ...r, launcher: "missing" })); } catch { setFirewallRows(r => ({ ...r, launcher: "error" })); } finally { setIsRemovingFwLauncher(false); } }
      else { setIsRemovingFwGame(true); try { await invoke("remove_firewall_rules", { which: "game" }); setFirewallRows(r => ({ ...r, game: "missing" })); } catch { setFirewallRows(r => ({ ...r, game: "error" })); } finally { setIsRemovingFwGame(false); } }
    }});
  }
  async function addDefLauncher() { setIsAddingDefLauncher(true); try { await invoke("add_defender_exclusions", { gamePath, which: "launcher" }); setDefenderRows(r => ({ ...r, launcher: "ok" })); } catch { setDefenderRows(r => ({ ...r, launcher: "error" })); } finally { setIsAddingDefLauncher(false); } }
  async function addDefGame() { setIsAddingDefGame(true); try { await invoke("add_defender_exclusions", { gamePath, which: "game" }); setDefenderRows(r => ({ ...r, game: "ok" })); } catch { setDefenderRows(r => ({ ...r, game: "error" })); } finally { setIsAddingDefGame(false); } }
  function confirmRemoveDef(which: "launcher" | "game") {
    setSecConfirmDialog({ open: true, title: "Remove Defender Exclusion", message: `Remove the ${which} Defender exclusion?`, onConfirm: async () => {
      setSecConfirmDialog(d => ({ ...d, open: false }));
      if (which === "launcher") { setIsRemovingDefLauncher(true); try { await invoke("remove_defender_exclusions", { gamePath, which: "launcher" }); setDefenderRows(r => ({ ...r, launcher: "missing" })); } catch { setDefenderRows(r => ({ ...r, launcher: "error" })); } finally { setIsRemovingDefLauncher(false); } }
      else { setIsRemovingDefGame(true); try { await invoke("remove_defender_exclusions", { gamePath, which: "game" }); setDefenderRows(r => ({ ...r, game: "missing" })); } catch { setDefenderRows(r => ({ ...r, game: "error" })); } finally { setIsRemovingDefGame(false); } }
    }});
  }
  async function fixPermLauncher() { setIsFixingPermLauncher(true); try { await invoke("fix_folder_permissions", { gamePath: "" }); setPermRows(r => ({ ...r, launcher: "ok" })); } catch { setPermRows(r => ({ ...r, launcher: "error" })); } finally { setIsFixingPermLauncher(false); } }
  async function fixPermGame() { setIsFixingPermGame(true); try { await invoke("fix_folder_permissions", { gamePath }); setPermRows(r => ({ ...r, game: "ok" })); } catch { setPermRows(r => ({ ...r, game: "error" })); } finally { setIsFixingPermGame(false); } }

  function secSectionStatus(rows: { launcher: string; game: string }) {
    if (rows.launcher === "scanning" || rows.game === "scanning") return "scanning";
    if (rows.launcher === "error" || rows.game === "error") return "error";
    if (rows.launcher === "unknown" && rows.game === "unknown") return "unknown";
    if (rows.launcher === "ok" && rows.game === "ok") return "ok";
    return "missing";
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
    <div className="flex-1 flex flex-col min-h-0">
      {!systemLoaded ? (
        <div className="flex-1 flex flex-col items-center justify-center gap-3">
          <div className="w-8 h-8 border-2 border-primary border-t-transparent rounded-full animate-spin" />
          <p className="text-xs text-muted">Loading system info...</p>
        </div>
      ) : (
      <div className="animate-fade-in flex-1 flex flex-col min-h-0">
      <div className="shrink-0 flex items-center justify-between px-4 py-3 sticky top-0 z-20">
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
      <div className="flex-1 relative min-h-0">
      <div className="h-full overflow-y-auto px-4 py-3" onScroll={e => setScrolled((e.target as HTMLElement).scrollTop > 0)} style={{ maskImage: scrolled ? "linear-gradient(to bottom, transparent 0px, black 24px)" : undefined, WebkitMaskImage: scrolled ? "linear-gradient(to bottom, transparent 0px, black 24px)" : undefined, transition: "mask-image 0.2s" }}>
      <div className="w-full grid grid-cols-3 gap-3">
        <section className="col-span-2 border border-border rounded-xl bg-surface overflow-hidden flex flex-col relative">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <FolderOpen size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Installation</h2>
              <p className="text-[10px] text-muted">Game directory and file integrity</p>
            </div>
            <div className="ml-auto flex items-center gap-2">
              <Button
                variant="ghost"
                size="sm"
                onClick={handleRemoveServerMods}
                disabled={!settings.installationDirectory || isRemovingMods || isAutoVerifying}
                isLoading={isRemovingMods}
                className="h-7 px-3 text-[11px] text-danger hover:bg-danger/10"
              >
                <Trash2 size={11} className="mr-1" />
                Remove Mods
              </Button>
              <Button
                size="sm"
                onClick={() => runAutoVerify(settings.installationDirectory, settings.selectedCDN)}
                disabled={folderStatus !== "valid" || !settings.selectedCDN || isAutoVerifying}
                isLoading={autoVerifyStatus === "verifying" || autoVerifyStatus === "repairing"}
                className="h-7 px-3 text-[11px]"
              >
                <Shield size={11} className="mr-1" />
                {autoVerifyStatus === "verifying" ? "Scanning..." : autoVerifyStatus === "repairing" ? "Repairing..." : "Verify Files"}
              </Button>
            </div>
          </div>
          <div className="px-4 py-3 flex-1">
            <div className="flex items-center gap-2 mb-1.5">
              <label className="text-[11px] text-muted">Game Directory</label>
              {folderStatus === "valid" && (
                <span className="text-[10px] text-success flex items-center gap-1">
                  <CheckCircle size={10} /> nfsw.exe found
                </span>
              )}
              {folderStatus === "invalid" && settings.installationDirectory && (
                <span className="text-[10px] text-accent flex items-center gap-1">
                  <AlertTriangle size={10} /> nfsw.exe not found — game will be downloaded
                </span>
              )}
              {folderStatus === "cloud" && (
                <span className="text-[10px] text-danger flex items-center gap-1">
                  <AlertTriangle size={10} /> Cloud storage not supported
                </span>
              )}
            </div>
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
          </div>
          {verifyPanelVisible && (
            <div className={`absolute bottom-0 left-0 right-0 px-4 py-2 bg-surface ${verifyPanelExiting ? "animate-fade-out" : "animate-fade-in"}`}>
              {(autoVerifyStatus === "verifying" || autoVerifyStatus === "repairing") && (
                <div key={autoVerifyStatus} className="space-y-1 animate-fade-in">
                  <div className="flex items-center gap-1.5">
                    {autoVerifyStatus === "repairing"
                      ? <Download size={11} className="text-primary shrink-0 animate-pulse" />
                      : <Shield size={11} className="text-primary shrink-0" />}
                    <p className="text-[11px] font-medium text-primary">
                      {autoVerifyMessage}
                    </p>
                  </div>
                  <ProgressBar value={verifyPercent} variant="primary" size="sm" showPercent />
                  {verifyCurrentFile && (
                    <p className="text-[10px] text-muted truncate font-mono">{verifyCurrentFile}</p>
                  )}
                </div>
              )}
              {autoVerifyStatus === "done" && (
                <p key="done" className="text-[10px] text-success flex items-center gap-1 animate-fade-in">
                  <CheckCircle size={10} /> {autoVerifyMessage}
                </p>
              )}
              {autoVerifyStatus === "error" && (
                <p key="error" className="text-[10px] text-danger flex items-center gap-1 animate-fade-in">
                  <AlertTriangle size={10} /> {autoVerifyMessage}
                </p>
              )}
            </div>
          )}
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

      {isWindows && (() => {
        const fwStatus = secSectionStatus(firewallRows);
        const defStatus = secSectionStatus(defenderRows);
        const permStatus = secSectionStatus(permRows);
        const allOk = fwStatus === "ok" && defStatus === "ok" && permStatus === "ok";
        const anyScanning = fwStatus === "scanning" || defStatus === "scanning" || permStatus === "scanning";

        function SecSectionIcon({ status }: { status: string }) {
          switch (status) {
            case "ok":       return <ShieldCheck size={15} className="text-success shrink-0" />;
            case "missing":  return <ShieldAlert  size={15} className="text-warning shrink-0" />;
            case "error":    return <ShieldOff    size={15} className="text-danger  shrink-0" />;
            case "scanning": return <Shield       size={15} className="text-primary animate-pulse shrink-0" />;
            default:         return <Shield       size={15} className="text-muted   shrink-0" />;
          }
        }

        function SecStatusBadge({ status }: { status: string }) {
          switch (status) {
            case "scanning": return <span className="flex items-center gap-1 text-[11px] text-primary whitespace-nowrap"><Loader2 size={11} className="animate-spin" /> Scanning...</span>;
            case "ok":       return <span className="flex items-center gap-1 text-[11px] text-success whitespace-nowrap"><CheckCircle2 size={11} /> Protected</span>;
            case "missing":  return <span className="flex items-center gap-1 text-[11px] text-warning whitespace-nowrap"><AlertCircle size={11} /> Missing</span>;
            case "error":    return <span className="flex items-center gap-1 text-[11px] text-danger whitespace-nowrap"><XCircle size={11} /> Error</span>;
            default:         return <span className="text-[11px] text-muted">-</span>;
          }
        }

        function SecItemRow({ icon, name, detail, status, addLabel, onAdd, isAdding, onRemove, isRemoving, canRemove = true }: { icon: React.ReactNode; name: string; detail?: string; status: string; addLabel: string; onAdd: () => void; isAdding: boolean; onRemove: () => void; isRemoving: boolean; canRemove?: boolean }) {
          const busy = status === "scanning";
          return (
            <div className="flex items-center gap-3 py-2.5 border-b border-border/40 last:border-0">
              <div className="shrink-0 text-muted">{icon}</div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <p className="text-[13px] font-medium leading-tight">{name}</p>
                  <SecStatusBadge status={status} />
                </div>
                {detail && <p className="text-[10px] text-muted truncate mt-0.5">{detail}</p>}
              </div>
              <div className="shrink-0 ml-2 w-20 flex justify-end">
                {status !== "ok" && <Button variant="primary" size="sm" onClick={onAdd} isLoading={isAdding} disabled={busy || isRemoving} className="h-7 px-3 text-[11px] w-full">{addLabel}</Button>}
                {status === "ok" && canRemove && <Button variant="ghost" size="sm" onClick={onRemove} isLoading={isRemoving} disabled={busy || isAdding} className="h-7 px-3 text-[11px] w-full text-muted hover:text-danger"><XCircle size={12} className="mr-1" />Remove</Button>}
                {status === "ok" && !canRemove && <CheckCircle2 size={16} className="text-success mx-auto" />}
              </div>
            </div>
          );
        }

        return (
          <div className="w-full mt-3">
            <div className="grid grid-cols-3 gap-3">
              <section className="border border-border rounded-xl bg-surface overflow-hidden">
                <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50">
                  <SecSectionIcon status={fwStatus} />
                  <div className="flex-1 min-w-0">
                    <h2 className="text-xs font-bold tracking-wide uppercase truncate">Windows Firewall</h2>
                    <p className="text-[10px] text-muted truncate">Network access rules</p>
                  </div>
                  {firewallApiOk === false && <span className="text-[10px] text-danger border border-danger/30 rounded px-1.5 py-0.5 shrink-0">N/A</span>}
                </div>
                <div className="px-4">
                  <SecItemRow icon={<Flame size={13} />} name="Launcher" detail="Connection to game servers" status={firewallRows.launcher} addLabel="Add" onAdd={addFwLauncher} isAdding={isAddingFwLauncher} onRemove={() => confirmRemoveFw("launcher")} isRemoving={isRemovingFwLauncher} />
                  <SecItemRow icon={<Flame size={13} />} name="Game" detail="nfsw.exe" status={firewallRows.game} addLabel="Add" onAdd={addFwGame} isAdding={isAddingFwGame} onRemove={() => confirmRemoveFw("game")} isRemoving={isRemovingFwGame} />
                </div>
              </section>
              <section className="border border-border rounded-xl bg-surface overflow-hidden">
                <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50">
                  <SecSectionIcon status={defStatus} />
                  <div className="flex-1 min-w-0">
                    <h2 className="text-xs font-bold tracking-wide uppercase truncate">Windows Defender</h2>
                    <p className="text-[10px] text-muted truncate">Antivirus exclusions</p>
                  </div>
                  {defenderApiOk === false && <span className="text-[10px] text-danger border border-danger/30 rounded px-1.5 py-0.5 shrink-0">N/A</span>}
                </div>
                <div className="px-4">
                  <SecItemRow icon={<Bug size={13} />} name="Launcher" detail="Launcher folder exclusion" status={defenderRows.launcher} addLabel="Exclude" onAdd={addDefLauncher} isAdding={isAddingDefLauncher} onRemove={() => confirmRemoveDef("launcher")} isRemoving={isRemovingDefLauncher} />
                  <SecItemRow icon={<Bug size={13} />} name="Game" detail="Game folder exclusion" status={defenderRows.game} addLabel="Exclude" onAdd={addDefGame} isAdding={isAddingDefGame} onRemove={() => confirmRemoveDef("game")} isRemoving={isRemovingDefGame} />
                </div>
              </section>
              <section className="border border-border rounded-xl bg-surface overflow-hidden">
                <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50">
                  <SecSectionIcon status={permStatus} />
                  <div className="flex-1 min-w-0">
                    <h2 className="text-xs font-bold tracking-wide uppercase truncate">Folder Permissions</h2>
                    <p className="text-[10px] text-muted truncate">Write access</p>
                  </div>
                </div>
                <div className="px-4">
                  <SecItemRow icon={<FolderOpen size={13} />} name="Launcher" detail="Updates & configuration" status={permRows.launcher} addLabel="Fix" onAdd={fixPermLauncher} isAdding={isFixingPermLauncher} onRemove={() => {}} isRemoving={false} canRemove={false} />
                  <SecItemRow icon={<FolderOpen size={13} />} name="Game" detail={gamePath || "Set game path in settings"} status={permRows.game} addLabel="Fix" onAdd={fixPermGame} isAdding={isFixingPermGame} onRemove={() => {}} isRemoving={false} canRemove={false} />
                </div>
              </section>
            </div>
          </div>
        );
      })()}

      <GameSettingsEditor
        isOpen={gameSettingsOpen}
        onClose={() => setGameSettingsOpen(false)}
      />
      <ConfirmDialog
        isOpen={secConfirmDialog.open}
        onClose={() => setSecConfirmDialog(d => ({ ...d, open: false }))}
        onConfirm={secConfirmDialog.onConfirm}
        title={secConfirmDialog.title}
        message={secConfirmDialog.message}
        confirmText="Remove"
        cancelText="Cancel"
        variant="warning"
      />
      </div>
      </div>
      </div>
      )}
    </div>
  );
}
