"use client";

import { useEffect, useState, useRef } from "react";
import { open } from "@tauri-apps/plugin-shell";
import { Button } from "@/components/ui/Button";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { LoginForm } from "@/components/forms/LoginForm";
import { useLauncherStore } from "@/stores/launcherStore";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { usePlaytimeStore, formatPlaytime } from "@/stores/playtimeStore";
import {
  fetchServerDetails,
  downloadGame,
  validateGameFolder,
  pickGameFolder,
  verifyGameFiles,
  repairGameFiles,
} from "@/lib/tauri-api";
import { formatBytes, formatSpeed, formatETA } from "@/lib/utils";
import { Tooltip } from "@/components/ui/Tooltip";
import {
  Play,
  Download,
  Users,
  Globe,
  ExternalLink,
  FolderOpen,
  AlertTriangle,
  Server,
  Gamepad2,
  CheckCircle,
} from "lucide-react";

export function MainScreen() {
  const {
    isLoggedIn,
    downloadProgress,
    setPage,
    setDownloadProgress,
    setAutoVerifying,
  } = useLauncherStore();
  const {
    selectedServer,
    selectedServerDetails,
    setServerDetails,
  } = useServerStore();
  const { settings } = useSettingsStore();
  const { getSeconds } = usePlaytimeStore();
  const [needsGameFiles, setNeedsGameFiles] = useState(false);
  const [isDownloadActive, setIsDownloadActive] = useState(false);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [showDownloadError, setShowDownloadError] = useState(false);
  const downloadErrorRef = useRef<string | null>(null);
  const unlistenRef = useRef<(() => void) | null>(null);

  const [autoVerifyStatus, setAutoVerifyStatus] = useState<"idle" | "verifying" | "repairing" | "done" | "error">("idle");
  const [autoVerifyMessage, setAutoVerifyMessage] = useState("");
  const [verifyPercent, setVerifyPercent] = useState(0);
  const [verifyCurrentFile, setVerifyCurrentFile] = useState("");
  const [verifyShow, setVerifyShow] = useState(false);
  const verifyDismissTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const verifyUnlistenRef = useRef<(() => void) | null>(null);

  const [displayedServer, setDisplayedServer] = useState(selectedServer);
  const [displayedDetails, setDisplayedDetails] = useState(selectedServerDetails);
  const [contentVisible, setContentVisible] = useState(selectedServerDetails !== null);
  const [onlineFading, setOnlineFading] = useState(false);
  const [versionFading, setVersionFading] = useState(false);
  const onlineFadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const versionFadeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isTransitioning = useRef(false);
  const prevDetailsRef = useRef(selectedServerDetails);
  const contentRef = useRef<HTMLDivElement>(null);
  const latestServerRef = useRef(selectedServer);
  const latestDetailsRef = useRef(selectedServerDetails);
  useEffect(() => { latestServerRef.current = selectedServer; }, [selectedServer]);
  useEffect(() => { latestDetailsRef.current = selectedServerDetails; }, [selectedServerDetails]);

  const fadeOutDoneRef = useRef(false);
  const pendingRevealRef = useRef<{ server: typeof selectedServer; details: typeof selectedServerDetails } | null>(null);
  const revealFallbackRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const contentVisibleRef = useRef(selectedServerDetails !== null);
  contentVisibleRef.current = contentVisible;

  const doReveal = (server: typeof selectedServer, details: typeof selectedServerDetails) => {
    if (revealFallbackRef.current) clearTimeout(revealFallbackRef.current);
    prevDetailsRef.current = details;
    setDisplayedServer(server);
    setDisplayedDetails(details);
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        setContentVisible(true);
        setTimeout(() => {
          isTransitioning.current = false;
          prevDetailsRef.current = latestDetailsRef.current;
          setDisplayedDetails(latestDetailsRef.current);
          setDisplayedServer(latestServerRef.current);
        }, 420);
      });
    });
  };

  const selectedServerId = selectedServer?.id ?? null;
  const displayedServerId = displayedServer?.id ?? null;

  useEffect(() => {
    if (isTransitioning.current && selectedServerId !== displayedServerId && selectedServerDetails !== null) {
      pendingRevealRef.current = { server: latestServerRef.current, details: selectedServerDetails };
      if (fadeOutDoneRef.current) {
        doReveal(latestServerRef.current, selectedServerDetails);
      }
      return;
    }

    if (!isTransitioning.current && selectedServerId === displayedServerId) {
      const prev = prevDetailsRef.current;
      const next = selectedServerDetails;
      prevDetailsRef.current = next;

      if (prev === null && next !== null && !contentVisible) {
        setDisplayedDetails(next);
        setDisplayedServer(selectedServer);
        requestAnimationFrame(() => {
          requestAnimationFrame(() => setContentVisible(true));
        });
        return;
      }

      const onlineChanged =
        prev != null && next != null &&
        (prev.onlineNumber !== next.onlineNumber || prev.maxUsersAllowed !== next.maxUsersAllowed);
      const versionChanged =
        prev != null && next != null &&
        prev.serverVersion !== next.serverVersion;

      if (!onlineChanged && !versionChanged) {
        setDisplayedDetails(next);
        setDisplayedServer(selectedServer);
        return;
      }

      if (onlineChanged) setOnlineFading(true);
      if (versionChanged) setVersionFading(true);

      const t = setTimeout(() => {
        setDisplayedDetails(next);
        setDisplayedServer(selectedServer);
      }, 150);

      if (onlineChanged) {
        if (onlineFadeTimer.current) clearTimeout(onlineFadeTimer.current);
        onlineFadeTimer.current = setTimeout(() => setOnlineFading(false), 300);
      }
      if (versionChanged) {
        if (versionFadeTimer.current) clearTimeout(versionFadeTimer.current);
        versionFadeTimer.current = setTimeout(() => setVersionFading(false), 300);
      }
      return () => clearTimeout(t);
    }
  }, [selectedServerDetails, selectedServer, selectedServerId, displayedServerId]);

  useEffect(() => {
    if (selectedServerId && selectedServerId !== displayedServerId) {
      isTransitioning.current = true;
      fadeOutDoneRef.current = false;
      pendingRevealRef.current = null;
      setServerDetails(null);

      if (!contentVisibleRef.current) {
        fadeOutDoneRef.current = true;
        return () => {
          if (revealFallbackRef.current) clearTimeout(revealFallbackRef.current);
          isTransitioning.current = false;
          fadeOutDoneRef.current = false;
        };
      }

      const el = contentRef.current;

      const onTransitionEnd = (e: TransitionEvent) => {
        if (e.propertyName !== "opacity" || e.target !== el) return;
        el?.removeEventListener("transitionend", onTransitionEnd);
        fadeOutDoneRef.current = true;

        if (pendingRevealRef.current) {
          doReveal(pendingRevealRef.current.server, pendingRevealRef.current.details);
        } else {
          revealFallbackRef.current = setTimeout(() => {
            doReveal(latestServerRef.current, latestDetailsRef.current);
          }, 8000);
        }
      };

      if (el) el.addEventListener("transitionend", onTransitionEnd);
      setContentVisible(false);

      return () => {
        el?.removeEventListener("transitionend", onTransitionEnd);
        if (revealFallbackRef.current) clearTimeout(revealFallbackRef.current);
        isTransitioning.current = false;
        fadeOutDoneRef.current = false;
      };
    }
  }, [selectedServerId]);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<{
          status: string;
          file_name: string;
          current_file: number;
          total_files: number;
          downloaded_bytes: number;
          total_bytes: number;
          speed: number;
          eta: number;
          error: string | null;
        }>("download-progress", (event) => {
          if (cancelled) return;
          const d = event.payload;
          setDownloadProgress({
            fileName: d.file_name,
            currentFile: d.current_file,
            totalFiles: d.total_files,
            downloadedBytes: d.downloaded_bytes,
            totalBytes: d.total_bytes,
            speed: d.speed,
            eta: d.eta,
            status: d.status === "completed" ? "completed"
              : d.status === "extracting" ? "extracting"
              : d.status === "verifying" ? "verifying"
              : d.status === "error" ? "error"
              : "downloading",
          });
          if (d.error) setDownloadError(d.error);
        });
        unlistenRef.current = unlisten;
      } catch {
      }
    })();
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, [setDownloadProgress]);

  useEffect(() => {
    if (settings.installationDirectory) {
      validateGameFolder(settings.installationDirectory).then((valid) => {
        setNeedsGameFiles(!valid);
      });
    }
  }, [settings.installationDirectory]);

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
        verifyUnlistenRef.current = unlisten;
      } catch {
      }
    })();
    return () => {
      cancelled = true;
      verifyUnlistenRef.current?.();
    };
  }, []);

  const prevDir = useRef(settings.installationDirectory);
  useEffect(() => {
    const dirChanged = prevDir.current !== settings.installationDirectory;
    prevDir.current = settings.installationDirectory;
    if (!dirChanged || !settings.installationDirectory || !settings.selectedCDN) return;

    validateGameFolder(settings.installationDirectory).then(async (valid) => {
      if (!valid) return;
      setAutoVerifyStatus("verifying");
      setAutoVerifyMessage("Scanning game files...");
      setVerifyPercent(0);
      setVerifyCurrentFile("");
      setVerifyShow(true);
      if (verifyDismissTimer.current) clearTimeout(verifyDismissTimer.current);
      setAutoVerifying(true);
      try {
        const corrupted = await verifyGameFiles(settings.selectedCDN, settings.installationDirectory);
        if (corrupted.length === 0) {
          setAutoVerifyStatus("done");
          setAutoVerifyMessage("All files verified — no issues found.");
          setVerifyPercent(100);
          verifyDismissTimer.current = setTimeout(() => setVerifyShow(false), 10000);
        } else {
          setAutoVerifyStatus("repairing");
          setAutoVerifyMessage(`Found ${corrupted.length} corrupted file${corrupted.length > 1 ? "s" : ""}, repairing...`);
          setVerifyPercent(0);
          await repairGameFiles(settings.selectedCDN, settings.installationDirectory, corrupted);
          setAutoVerifyStatus("done");
          setAutoVerifyMessage(`Repaired ${corrupted.length} file${corrupted.length > 1 ? "s" : ""} successfully.`);
          setVerifyPercent(100);
          verifyDismissTimer.current = setTimeout(() => setVerifyShow(false), 10000);
        }
      } catch (err) {
        setAutoVerifyStatus("error");
        setAutoVerifyMessage(err instanceof Error ? err.message : String(err));
        verifyDismissTimer.current = setTimeout(() => setVerifyShow(false), 10000);
      } finally {
        setAutoVerifying(false);
      }
    });
  }, [settings.installationDirectory]);

  useEffect(() => {
    if (!selectedServer) return;
    fetchServerDetails(selectedServer.ip)
      .then(setServerDetails)
      .catch(() => setServerDetails(null));

    const interval = setInterval(() => {
      fetchServerDetails(selectedServer.ip)
        .then(setServerDetails)
        .catch(() => {});
    }, process.env.NODE_ENV === "development" ? 5_000 : 60_000);
    return () => clearInterval(interval);
  }, [selectedServer?.id, setServerDetails]);

  async function handleDownloadGame() {
    if (!settings.selectedCDN || !settings.installationDirectory) {
      setDownloadError("Configure CDN and game folder in Settings.");
      return;
    }
    setDownloadError(null);
    setIsDownloadActive(true);
    setDownloadProgress({ status: "downloading", downloadedBytes: 0, totalBytes: 0 });
    try {
      await downloadGame(settings.selectedCDN, settings.installationDirectory);
      setNeedsGameFiles(false);
      setIsDownloadActive(false);
      setDownloadProgress({ status: "idle" });
    } catch (err) {
      setDownloadError(err instanceof Error ? err.message : String(err));
      setIsDownloadActive(false);
      setDownloadProgress({ status: "error" });
    }
  }

  const isDownloading = downloadProgress.status === "downloading" || downloadProgress.status === "extracting" || downloadProgress.status === "verifying" || isDownloadActive;

  useEffect(() => {
    if (downloadError && !isDownloading) {
      downloadErrorRef.current = downloadError;
      setShowDownloadError(true);
    } else {
      setShowDownloadError(false);
    }
  }, [downloadError, isDownloading]);

  const downloadPercent =
    downloadProgress.totalBytes > 0
      ? (downloadProgress.downloadedBytes / downloadProgress.totalBytes) * 100
      : downloadProgress.totalFiles > 0
        ? (downloadProgress.currentFile / downloadProgress.totalFiles) * 100
        : 0;

  if (!selectedServer) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center space-y-4 animate-scale-in">
          <div className="w-16 h-16 rounded-2xl bg-surface border border-border flex items-center justify-center mx-auto">
            <Gamepad2 size={28} className="text-muted/40" />
          </div>
          <div>
            <h2 className="text-sm font-semibold text-muted-foreground">
              Select a Server
            </h2>
            <p className="text-[11px] text-muted mt-1">
              Choose a server from the list on the left to get started.
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={contentRef}
      className="flex-1 flex flex-col relative"
      style={{ opacity: contentVisible ? 1 : 0, transition: "opacity 0.4s ease" }}
    >
      <div className="absolute top-0 left-0 right-0 px-6 pt-5 pb-3 flex items-end justify-between z-20 pointer-events-none">
        <div className="pointer-events-auto">
          <h1 className="text-xl font-bold">
            {displayedServer?.name}
          </h1>
          {displayedDetails && (
            <div className="flex items-center gap-4 mt-1.5 text-xs text-muted">
              <span className={`flex items-center gap-1.5 transition-opacity duration-150 ${onlineFading ? "opacity-0" : "opacity-100"}`}>
                <div className="w-1.5 h-1.5 rounded-full bg-success" />
                <Users size={11} />
                {displayedDetails.onlineNumber}{displayedDetails.maxUsersAllowed ? ` / ${displayedDetails.maxUsersAllowed}` : ""}
              </span>
              {displayedDetails.serverVersion && displayedDetails.serverVersion !== "unspecified" && (
                <span className={`flex items-center gap-1 transition-opacity duration-150 ${versionFading ? "opacity-0" : "opacity-100"}`}>
                  <Server size={11} />
                  v{displayedDetails.serverVersion}
                </span>
              )}
            </div>
          )}
        </div>
        <div className="flex gap-1.5 pointer-events-auto">
          {displayedDetails?.homePageUrl && (
            <Tooltip label="Website">
              <a
                href={displayedDetails.homePageUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="text-muted hover:text-foreground transition-smooth p-2 rounded-lg hover:bg-surface-hover"
              >
                <ExternalLink size={14} />
              </a>
            </Tooltip>
          )}
          {displayedDetails?.discordUrl && (
            <Tooltip label="Discord">
              <a
                href={displayedDetails.discordUrl}
                target="_blank"
                rel="noopener noreferrer"
                className="text-muted hover:text-foreground transition-smooth p-2 rounded-lg hover:bg-surface-hover"
              >
                <Globe size={14} />
              </a>
            </Tooltip>
          )}
        </div>
      </div>
      <div className="flex-1 flex items-center justify-center px-6">
        <div className="space-y-4">
        <div className="flex gap-2 items-stretch">
        <div className="w-[24rem] shrink-0">
          <div className="bg-surface border border-border rounded-xl p-5 relative">
              <h3 className="text-[10px] font-semibold mb-4 uppercase tracking-widest text-muted">
                {isLoggedIn ? "Launch" : needsGameFiles ? "Download" : "SIGN IN"}
              </h3>
              <LoginForm
                needsGameFiles={needsGameFiles}
                isDownloading={isDownloading}
                onDownloadGame={handleDownloadGame}
                canDownload={!!settings.selectedCDN}
                displayedServerId={displayedServer?.id ?? null}
              />
              <div className="absolute left-0 right-0 top-full mt-2 z-10 space-y-2">
                <div
                  style={{
                    opacity: isDownloading ? 1 : 0,
                    transform: isDownloading ? 'translateY(0px)' : 'translateY(8px)',
                    transition: 'opacity 0.4s ease, transform 0.4s ease',
                    pointerEvents: isDownloading ? 'auto' : 'none',
                  }}
                >
                  <div className="bg-surface border border-border rounded-xl p-4 space-y-2 shadow-lg">
                    <div className="flex items-center justify-between">
                      <h3 className="text-[10px] font-medium flex items-center gap-2">
                        <div className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
                        {downloadProgress.status === "extracting"
                          ? "Extracting"
                          : downloadProgress.status === "verifying"
                          ? "Verifying"
                          : "Downloading"}
                      </h3>
                      <span className="text-[9px] text-muted font-mono bg-surface-hover px-1.5 py-0.5 rounded">
                        {downloadProgress.currentFile} / {downloadProgress.totalFiles}
                      </span>
                    </div>

                    <div className="flex justify-between items-center mb-1">
                      <span className="text-sm font-mono text-muted">{downloadPercent.toFixed(1)}%</span>
                      <span className="text-[9px] text-muted font-mono">{formatSpeed(downloadProgress.speed)}</span>
                    </div>

                    <ProgressBar
                      value={downloadPercent}
                      variant="primary"
                      size="sm"
                      showPercent={false}
                    />

                    <div className="flex justify-between text-[9px] text-muted font-mono">
                      <span>
                        {downloadProgress.totalBytes > 0
                          ? `${formatBytes(downloadProgress.downloadedBytes)} / ${formatBytes(downloadProgress.totalBytes)}`
                          : formatBytes(downloadProgress.downloadedBytes)}
                      </span>
                      <span>ETA {formatETA(downloadProgress.eta || 0)}</span>
                    </div>

                    <p className="text-[9px] text-muted truncate">
                      {downloadProgress.fileName}
                    </p>
                  </div>
                </div>
              </div>
              <div
                className="absolute left-0 right-0 top-full z-9 mt-2"
                style={{
                  opacity: verifyShow ? 1 : 0,
                  transform: verifyShow ? 'translateY(0px)' : 'translateY(8px)',
                  transition: 'opacity 0.4s ease, transform 0.4s ease',
                  pointerEvents: verifyShow ? 'auto' : 'none',
                }}
              >
                <div
                  className="rounded-xl p-4 space-y-2 shadow-lg bg-surface border border-border"
                  style={{
                    borderColor: autoVerifyStatus === "done" ? 'rgba(16, 185, 129, 0.3)' :
                                 autoVerifyStatus === "error" ? 'rgba(239, 68, 68, 0.3)' : undefined,
                    transition: 'border-color 0.3s ease',
                  }}
                >
                  {(autoVerifyStatus === "verifying" || autoVerifyStatus === "repairing") && (
                    <>
                      <div className="flex items-center justify-between">
                        <h3 className="text-[10px] font-medium flex items-center gap-2">
                          <div className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
                          <span className={autoVerifyStatus === "repairing" ? "text-accent" : ""}>
                            {autoVerifyStatus === "verifying" ? "Verifying" : "Repairing"}
                          </span>
                        </h3>
                        <span className="text-[9px] text-muted font-mono bg-surface-hover px-1.5 py-0.5 rounded">
                          {autoVerifyMessage}
                        </span>
                      </div>
                      <ProgressBar
                        value={verifyPercent}
                        variant={autoVerifyStatus === "repairing" ? "accent" : "primary"}
                        size="sm"
                        showPercent
                      />
                      {verifyCurrentFile && (
                        <p className="text-[9px] text-muted truncate">{verifyCurrentFile}</p>
                      )}
                    </>
                  )}
                  {autoVerifyStatus === "done" && (
                    <div className="flex items-center gap-2">
                      <CheckCircle size={11} className="text-success shrink-0" />
                      <p className="text-[10px] text-success">{autoVerifyMessage}</p>
                    </div>
                  )}
                  {autoVerifyStatus === "error" && (
                    <div className="flex items-center gap-2">
                      <AlertTriangle size={11} className="text-danger shrink-0" />
                      <p className="text-[10px] text-danger">{autoVerifyMessage}</p>
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        {displayedDetails && (
          <div className="w-36 shrink-0 flex flex-col">
            <div className="bg-surface border border-border/50 rounded-xl divide-y divide-border/50 flex-1 flex flex-col">
              <div className="px-3 py-2 flex-1 flex flex-col justify-center">
                <p className="text-[10px] text-muted uppercase tracking-widest leading-tight">Players</p>
                <p className="text-sm font-bold mt-0.5 font-mono leading-tight">
                  {displayedDetails.onlineNumber} / {displayedDetails.numberOfRegistered}
                </p>
              </div>
              <div
                className={`px-3 py-2 flex-1 flex flex-col justify-center ${displayedDetails.discordUrl ? 'cursor-pointer hover:bg-surface-hover transition-smooth' : ''}`}
                onClick={() => displayedDetails.discordUrl && open(displayedDetails.discordUrl).catch(() => {})}
              >
                <p className="text-[10px] text-muted uppercase tracking-widest leading-tight">Discord</p>
                <p className={`text-sm font-bold mt-0.5 font-mono leading-tight ${displayedDetails.discordUrl ? 'text-foreground' : 'text-muted'}`}>
                  {displayedDetails.discordUrl ? "Join" : "N/A"}
                </p>
              </div>
              <div className="px-3 py-2 flex-1 flex flex-col justify-center">
                <p className="text-[10px] text-muted uppercase tracking-widest leading-tight">Cash Multiplier</p>
                <p className={`text-sm font-bold mt-0.5 font-mono leading-tight ${displayedDetails.cashRewardMultiplier && displayedDetails.cashRewardMultiplier > 1 ? "text-accent" : "text-muted"}`}>
                  {displayedDetails.cashRewardMultiplier && displayedDetails.cashRewardMultiplier > 1 ? `X${displayedDetails.cashRewardMultiplier}` : "Inactive"}
                </p>
              </div>
              <div className="px-3 py-2 flex-1 flex flex-col justify-center">
                <p className="text-[10px] text-muted uppercase tracking-widest leading-tight">Rep Multiplier</p>
                <p className={`text-sm font-bold mt-0.5 font-mono leading-tight ${displayedDetails.repRewardMultiplier && displayedDetails.repRewardMultiplier > 1 ? "text-accent" : "text-muted"}`}>
                  {displayedDetails.repRewardMultiplier && displayedDetails.repRewardMultiplier > 1 ? `X${displayedDetails.repRewardMultiplier}` : "Inactive"}
                </p>
              </div>
              <div className="px-3 py-2 flex-1 flex flex-col justify-center">
                <p className="text-[10px] text-muted uppercase tracking-widest leading-tight">Playtime</p>
                <p className="text-sm font-bold mt-0.5 font-mono leading-tight text-foreground">
                  {displayedServer ? formatPlaytime(getSeconds(displayedServer.id)) : "0m"}
                </p>
              </div>
            </div>
          </div>
        )}
        </div>
          <div
            style={{
              opacity: showDownloadError ? 1 : 0,
              transform: showDownloadError ? 'translateY(0px)' : 'translateY(8px)',
              transition: 'opacity 0.4s ease, transform 0.4s ease',
              pointerEvents: showDownloadError ? 'auto' : 'none',
              maxHeight: showDownloadError ? '200px' : '0px',
              overflow: 'hidden',
            }}
          >
            <div className="bg-danger/5 border border-danger/20 rounded-xl p-4 flex items-start gap-3">
              <AlertTriangle size={14} className="text-danger mt-0.5 shrink-0" />
              <div className="flex-1">
                <p className="text-[11px] text-danger/90">{downloadErrorRef.current}</p>
                <button
                  className="text-[10px] text-muted hover:text-foreground mt-1.5 underline underline-offset-2 cursor-pointer"
                  onClick={() => setDownloadError(null)}
                >
                  Dismiss
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

