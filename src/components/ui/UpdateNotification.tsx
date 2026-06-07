"use client";

import { useEffect, useState, useRef } from "react";
import { createPortal } from "react-dom";
import { Download, X, RefreshCw } from "lucide-react";
import { useUpdateStore } from "@/stores/updateStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { invoke } from "@tauri-apps/api/core";
import { cn } from "@/lib/utils";
import { Tooltip } from "@/components/ui/Tooltip";
import { formatVersionForDisplay } from "@/lib/config";
import { Button } from "@/components/ui/Button";
import { ProgressBar } from "@/components/ui/ProgressBar";

interface UpdateInfo {
  version: string;
  exe: string;
  publishDate: string;
  productName: string;
}

export function UpdateNotification() {
  const {
    updateAvailable,
    updateInfo,
    checking,
    downloading,
    downloadProgress,
    setUpdateAvailable,
    setChecking,
    setDownloading,
    setDownloadProgress,
  } = useUpdateStore();
  const { settings } = useSettingsStore();

  const [showModal, setShowModal] = useState(false);
  const [visible, setVisible] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [actionPhase, setActionPhase] = useState<"buttons" | "switching" | "progress">("buttons");
  const [isSpinning, setIsSpinning] = useState(false);
  const [iconAngle, setIconAngle] = useState(0);

  const displayVersion = updateInfo?.version
    ? formatVersionForDisplay(updateInfo.version)
    : undefined;

  const MAX_SPEED = 0.5;
  const ACCEL_MS  = 500;
  const DECEL_MS  = 600;
  const rafRef         = useRef<number | null>(null);
  const phaseRef       = useRef<"idle"|"accel"|"steady"|"decel">("idle");
  const angleRef       = useRef(0);
  const phaseStartRef  = useRef(0);
  const decelStartRef  = useRef(0);
  const decelTargetRef = useRef(0);
  const pendingStop    = useRef(false);
  const actionTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  function startSpinLoop() {
    let last = performance.now();
    function tick(now: number) {
      const dt      = Math.min(now - last, 50);
      last          = now;
      const elapsed = now - phaseStartRef.current;

      switch (phaseRef.current) {
        case "accel": {
          const t = Math.min(elapsed / ACCEL_MS, 1);
          angleRef.current += MAX_SPEED * (t * t) * dt;
          if (t >= 1) {
            phaseRef.current    = "steady";
            phaseStartRef.current = now;
          }
          break;
        }
        case "steady": {
          angleRef.current += MAX_SPEED * dt;
          if (pendingStop.current) {
            pendingStop.current    = false;
            const decelDist        = MAX_SPEED * DECEL_MS / 2;
            decelStartRef.current  = angleRef.current;
            decelTargetRef.current = Math.ceil((angleRef.current + decelDist) / 360) * 360;
            phaseRef.current       = "decel";
            phaseStartRef.current  = now;
          }
          break;
        }
        case "decel": {
          const t        = Math.min(elapsed / DECEL_MS, 1);
          const progress = 1 - (1 - t) * (1 - t);
          angleRef.current =
            decelStartRef.current +
            (decelTargetRef.current - decelStartRef.current) * progress;
          if (t >= 1) {
            phaseRef.current = "idle";
            setIconAngle(0);
            setIsSpinning(false);
            return;
          }
          break;
        }
        default:
          return;
      }
      setIconAngle(angleRef.current % 360);
      rafRef.current = requestAnimationFrame(tick);
    }
    rafRef.current = requestAnimationFrame(tick);
  }

  function triggerSpin() {
    if (phaseRef.current !== "idle") return;
    phaseRef.current      = "accel";
    phaseStartRef.current = performance.now();
    pendingStop.current   = false;
    setIsSpinning(true);
    startSpinLoop();
  }

  function triggerStop() {
    pendingStop.current = true;
  }

  useEffect(() => () => {
    if (rafRef.current) cancelAnimationFrame(rafRef.current);
    if (actionTimerRef.current) clearTimeout(actionTimerRef.current);
  }, []);

  useEffect(() => {
    if (checking) {
      triggerSpin();
    } else {
      triggerStop();
    }
  }, [checking]);

  function openPopup() {
    setShowModal(true);
    requestAnimationFrame(() => {
      requestAnimationFrame(() => setVisible(true));
    });
  }

  function closePopup() {
    setVisible(false);
    setTimeout(() => setShowModal(false), 300);
  }

  useEffect(() => {
    if (process.env.NODE_ENV === "development") return;

    checkForUpdates();

    const interval = setInterval(checkForUpdates, 5 * 60 * 1000);
    return () => clearInterval(interval);
  }, [settings.insider]);

  useEffect(() => {
    if (process.env.NODE_ENV !== "development") return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (!event.altKey || event.key.toLowerCase() !== "u") return;
      event.preventDefault();
      setError(null);
      setDownloading(false);
      setDownloadProgress(0);
      setUpdateAvailable(true, {
        version: "0.0.0-dev",
        exe: "rocket-launcher-dev-installer.exe",
        publishDate: new Date().toISOString(),
        productName: "Rocket Launcher (Dev)",
      });
      openPopup();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setDownloadProgress, setDownloading, setUpdateAvailable]);

  async function checkForUpdates() {
    if (checking || downloading) return;

    try {
      setChecking(true);
      setError(null);

      const command = settings.insider ? "check_for_beta_updates" : "check_for_updates";
      const result = await invoke<UpdateInfo | null>(command);

      if (result) {
        setUpdateAvailable(true, result);
        setTimeout(() => openPopup(), 4000);
      } else {
        setUpdateAvailable(false, null);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setChecking(false);
    }
  }

  async function handleDownloadAndInstall() {
    if (!updateInfo || downloading) return;

    try {
      setError(null);
      setDownloadProgress(0);
      setActionPhase("switching");
      if (actionTimerRef.current) clearTimeout(actionTimerRef.current);
      actionTimerRef.current = setTimeout(() => {
        setActionPhase("progress");
      }, 180);
      setDownloading(true);

      const installerPath = await invoke<string>("download_update", {
        exeName: updateInfo.exe,
      });

      setDownloadProgress(100);

      await new Promise((resolve) => setTimeout(resolve, 500));

      await invoke("install_update", { installerPath });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setDownloading(false);
      if (actionTimerRef.current) {
        clearTimeout(actionTimerRef.current);
        actionTimerRef.current = null;
      }
      setActionPhase("buttons");
    }
  }

  if (!updateAvailable && !checking) {
  }

  return (
    <>
      <Tooltip label={checking ? "Checking for updates..." : updateAvailable ? `Update available: ${displayVersion}` : "Check for updates"}>
        <button
          onClick={() => {
            if (updateAvailable) {
              showModal ? closePopup() : openPopup();
            } else if (!checking) {
              triggerSpin();
              setTimeout(() => checkForUpdates(), 2000);
            }
          }}
          className={cn(
            "relative p-2 rounded-md transition-all duration-200 ease-out",
            "hover:bg-white/10 hover:scale-105 active:scale-95",
            isSpinning && "cursor-not-allowed",
            updateAvailable && !isSpinning && "animate-pulse-green"
          )}
          disabled={isSpinning}
        >
          <div className="relative w-4 h-4">
            <RefreshCw
              size={16}
              className="absolute inset-0 text-muted transition-opacity duration-300"
              style={{
                transform: `rotate(${iconAngle}deg)`,
                opacity: updateAvailable && !isSpinning ? 0 : 1,
              }}
            />
            <Download
              size={16}
              className="absolute inset-0 text-success transition-[opacity,transform] duration-300"
              style={{
                transform: `scale(${updateAvailable && !isSpinning ? 1 : 0.5})`,
                opacity: updateAvailable && !isSpinning ? 1 : 0,
              }}
            />
          </div>
        </button>
      </Tooltip>
      {showModal && updateInfo && createPortal(
        <div
          className="fixed top-16 right-4 w-84 z-[9999] transition-all duration-250 ease-out"
          style={{
            opacity: visible ? 1 : 0,
            transform: visible ? "translateY(0)" : "translateY(-8px)",
            pointerEvents: visible ? "auto" : "none",
          }}
        >
          <div className="rounded-xl border border-border/50 bg-surface shadow-2xl">
            <div className="p-4 space-y-3">
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-center gap-2.5 min-w-0">
                  <div className="w-8 h-8 rounded-lg bg-surface-hover/80 border border-border/60 flex items-center justify-center shrink-0">
                    <Download size={14} className="text-primary" />
                  </div>
                  <div className="min-w-0">
                    <p className="text-[10px] uppercase tracking-widest text-muted">Launcher Update</p>
                    <h3 className="text-sm font-semibold text-foreground truncate">
                      Version {displayVersion ?? "unknown"} available
                    </h3>
                  </div>
                </div>
                <button
                  onClick={closePopup}
                  className="p-1 rounded-md text-muted hover:text-foreground hover:bg-surface-hover transition-smooth"
                >
                  <X size={14} />
                </button>
              </div>

              <div className="rounded-lg border border-border/50 bg-background/40 px-3 py-2">
                <p className="text-[11px] text-muted leading-relaxed">
                  A new version of Rocket Launcher is available. Install it to get the latest fixes and improvements.
                </p>
              </div>

              <div className="relative min-h-[2.25rem]">
                <div
                  className={cn(
                    "absolute inset-0 space-y-2 transition-all duration-250 ease-out",
                    actionPhase === "progress"
                      ? "opacity-100"
                      : "opacity-0 pointer-events-none"
                  )}
                >
                  <div className="flex items-center justify-between text-[11px]">
                    <span className="text-muted">Downloading...</span>
                    <span className="font-mono text-foreground">{Math.round(downloadProgress)}%</span>
                  </div>
                  <ProgressBar value={downloadProgress} showPercent={false} size="sm" variant="primary" />
                </div>

                <div
                  className={cn(
                    "absolute inset-0 transition-all duration-250 ease-out",
                    actionPhase === "progress" || actionPhase === "switching"
                      ? "opacity-0 pointer-events-none"
                      : "opacity-100"
                  )}
                >
                  <div className="flex gap-2">
                    <Button
                      onClick={handleDownloadAndInstall}
                      size="sm"
                      className="flex-1 gap-1.5 transition-all duration-150 ease-out active:scale-95"
                    >
                      <Download size={12} />
                      Install Update
                    </Button>
                    <Button
                      onClick={closePopup}
                      variant="ghost"
                      size="sm"
                      className="transition-all duration-150 ease-out active:scale-95"
                    >
                      Later
                    </Button>
                  </div>
                </div>
              </div>

            </div>
          </div>
          {error && (
            <div className="mt-2 rounded-lg border border-danger/40 bg-surface px-3 py-2 text-[11px] text-danger shadow-lg">
              {error}
            </div>
          )}
        </div>,
        document.body
      )}
    </>
  );
}

