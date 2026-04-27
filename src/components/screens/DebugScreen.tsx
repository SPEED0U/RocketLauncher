"use client";

import { useState, useEffect } from "react";
import { useSettingsStore } from "@/stores/settingsStore";
import { useServerStore } from "@/stores/serverStore";
import { useLauncherStore } from "@/stores/launcherStore";
import { getSystemInfo, getHwidInfo } from "@/lib/tauri-api";
import { APP_VERSION, getAppVersion } from "@/lib/config";
import { Copy, Check, Monitor, Settings2, Server, Cpu, Fingerprint } from "lucide-react";
import { Button } from "@/components/ui/Button";
import type { SystemInfo } from "@/lib/tauri-api";

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex justify-between items-baseline gap-4 py-0.5 min-w-0">
      <span className="text-[11px] text-muted shrink-0">{label}</span>
      <span className="text-[11px] font-mono text-foreground/80 truncate min-w-0 text-right">{value}</span>
    </div>
  );
}

function Section({
  icon,
  title,
  description,
  children,
  className,
}: {
  icon: React.ReactNode;
  title: string;
  description?: string;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={`border border-border rounded-xl bg-surface overflow-hidden flex flex-col ${className ?? ""}`}>
      <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
        <span className="text-primary shrink-0">{icon}</span>
        <div>
          <h2 className="text-xs font-bold tracking-wide uppercase">{title}</h2>
          {description && <p className="text-[10px] text-muted">{description}</p>}
        </div>
      </div>
      <div className="px-4 py-3 space-y-0.5 flex-1">
        {children}
      </div>
    </section>
  );
}

export function DebugScreen() {
  const { settings } = useSettingsStore();
  const { selectedServer } = useServerStore();
  const { userEmail } = useLauncherStore();
  const [copied, setCopied] = useState(false);
  const [scrolled, setScrolled] = useState(false);
  const [sysInfo, setSysInfo] = useState<SystemInfo | null>(null);
  const [hwid, setHwid] = useState("Loading...");
  const [hiddenHwid, setHiddenHwid] = useState("Loading...");
  const [version, setVersion] = useState("APP_VERSION");

  useEffect(() => {
    getSystemInfo().then(setSysInfo).catch(() => setSysInfo(null));
    getHwidInfo()
      .then(([h, hh]) => { setHwid(h); setHiddenHwid(hh); })
      .catch(() => { setHwid("N/A"); setHiddenHwid("N/A"); });
    getAppVersion().then(setVersion).catch(() => setVersion(APP_VERSION));
  }, []);

  const fmtBytes = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(1)} GB`;

  // Function to extract CPU model from full brand string
  const extractCPUModel = (brand: string) => {
    if (!brand) return "Unknown";
    
    brand = brand.replace(/(AMD\s+)/, 'AMD ');
    brand = brand.replace(/(Intel\(R\) Core\(TM\)\s*)/, 'Intel® Core™ ');
    brand = brand.replace(/CPU\s+@\s+\d+(\.\d+)?[GM]Hz/i, '');
    brand = brand.trim();
    
    return brand || "Unknown";
  };

  function copyDebugInfo() {
    const lines = [
      `Operating System: ${sysInfo ? `${sysInfo.os_name} ${sysInfo.os_version}` : "Unknown"}`,
      `Kernel Version: ${sysInfo?.kernel_version ?? "Unknown"}`,
      `Hostname: ${sysInfo?.hostname ?? "Unknown"}`,
      `Screen Resolution: ${window.screen.width}x${window.screen.height}`,
      "",
      `Launcher Version: ${version}`,
      `Language: ${navigator.language || "en-US"}`,
      `Install Directory: ${settings.installationDirectory || "Not Set"}`,
      `Credentials Saved: ${userEmail ? "Yes" : "No"}`,
      `Proxy: ${settings.disableProxy ? "Disabled" : "Enabled"}`,
      `Discord RPC: ${settings.disableRPC ? "Disabled" : "Enabled"}`,
      `Insider: ${settings.insider ? "Insider Opt-In" : "Release Only"}`,
      "",
      `Server Name: ${selectedServer?.name ?? "None"}`,
      `Server Address: ${selectedServer?.ip ?? "N/A"}`,
      `CDN Address: ${settings.selectedCDN ?? "N/A"}`,
      `Client Method: HTTP`,
      "",
      `CPU: ${sysInfo?.cpu_brand ? extractCPUModel(sysInfo.cpu_brand) : "Unknown"}`,
      `CPU Cores: ${sysInfo?.cpu_cores?.toString() ?? "Unknown"}`,
      `RAM: ${sysInfo ? `${fmtBytes(sysInfo.total_memory)} (${fmtBytes(sysInfo.used_memory)} used)` : "Unknown"}`,
      `GPU: ${sysInfo?.gpu_name ?? "Unknown"}`,
      `GPU Driver: ${sysInfo?.gpu_driver ?? "Unknown"}`,
      `Disk Free: ${sysInfo ? fmtBytes(sysInfo.disk_free) : "Unknown"}`,
      `Disk Total: ${sysInfo ? fmtBytes(sysInfo.disk_total) : "Unknown"}`,
      `Disk Type: ${sysInfo?.disk_kind ?? "Unknown"}`,
      "",
      `X-HWID: ${hwid}`,
      `X-HiddenHWID: ${hiddenHwid}`,
    ];
    navigator.clipboard.writeText(lines.join("\n"));
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <div className="shrink-0 flex items-center justify-between px-4 py-3 sticky top-0 z-20">
        <h1 className="text-base font-bold">Diagnostics</h1>
        <Button variant="secondary" size="sm" onClick={copyDebugInfo} className="h-7 px-3 text-[11px]">
          {copied ? <Check size={11} className="mr-1" /> : <Copy size={11} className="mr-1" />}
          {copied ? "Copied!" : "Copy All"}
        </Button>
      </div>

      <div className="flex-1 relative min-h-0">
      <div className="h-full overflow-y-auto px-4 py-3 flex flex-col justify-center" onScroll={e => setScrolled((e.target as HTMLElement).scrollTop > 0)} style={{ maskImage: scrolled ? "linear-gradient(to bottom, transparent 0px, black 24px)" : undefined, WebkitMaskImage: scrolled ? "linear-gradient(to bottom, transparent 0px, black 24px)" : undefined, transition: "mask-image 0.2s" }}>
        <div className="grid grid-cols-3 gap-3">

          <Section icon={<Monitor size={15} />} title="System" description="OS and environment" className="col-span-2">
            <InfoRow label="Operating System" value={sysInfo ? `${sysInfo.os_name} ${sysInfo.os_version}` : "..."} />
            <InfoRow label="Kernel Version" value={sysInfo?.kernel_version ?? "..."} />
            <InfoRow label="Hostname" value={sysInfo?.hostname ?? "..."} />
            <InfoRow label="Screen Resolution" value={`${window.screen.width}x${window.screen.height}`} />
          </Section>

          <Section icon={<Fingerprint size={15} />} title="Identification" description="Hardware identifiers">
            <InfoRow label="X-HWID" value={hwid} />
            <InfoRow label="X-HiddenHWID" value={hiddenHwid} />
          </Section>

          <Section icon={<Settings2 size={15} />} title="Launcher" description="Configuration and state">
            <InfoRow label="Version" value={version} />
            <InfoRow label="Language" value={navigator.language || "en-US"} />
            <InfoRow label="Install Directory" value={settings.installationDirectory || "Not Set"} />
            <InfoRow label="Credentials Saved" value={userEmail ? "Yes" : "No"} />
            <InfoRow label="Proxy" value={settings.disableProxy ? "Disabled" : "Enabled"} />
            <InfoRow label="Discord RPC" value={settings.disableRPC ? "Disabled" : "Enabled"} />
            <InfoRow label="Insider" value={settings.insider ? "Insider Opt-In" : "Release Only"} />
          </Section>

          <Section icon={<Server size={15} />} title="Server" description="Connected server and CDN">
            <InfoRow label="Name" value={selectedServer?.name ?? "None"} />
            <InfoRow label="Address" value={selectedServer?.ip ?? "N/A"} />
            <InfoRow label="CDN" value={settings.selectedCDN ?? "N/A"} />
            <InfoRow label="Client Method" value="HTTP" />
          </Section>

          <Section icon={<Cpu size={15} />} title="Hardware" description="CPU, GPU and storage">
            <InfoRow label="CPU" value={sysInfo?.cpu_brand ? extractCPUModel(sysInfo.cpu_brand) : "..."} />
            <InfoRow label="CPU Cores" value={sysInfo?.cpu_cores?.toString() ?? "..."} />
            <InfoRow label="RAM" value={sysInfo ? `${fmtBytes(sysInfo.total_memory)} (${fmtBytes(sysInfo.used_memory)} used)` : "..."} />
            <InfoRow label="GPU" value={sysInfo?.gpu_name ?? "..."} />
            <InfoRow label="GPU Driver" value={sysInfo?.gpu_driver ?? "..."} />
            <InfoRow label="Disk Free" value={sysInfo ? fmtBytes(sysInfo.disk_free) : "..."} />
            <InfoRow label="Disk Total" value={sysInfo ? fmtBytes(sysInfo.disk_total) : "..."} />
            <InfoRow label="Disk Type" value={sysInfo?.disk_kind ?? "..."} />
          </Section>

        </div>
      </div>
      </div>
    </div>
  );
}

