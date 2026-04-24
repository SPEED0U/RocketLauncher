"use client";

import { cn } from "@/lib/utils";
import { useLauncherStore } from "@/stores/launcherStore";
import {
  Settings,
  Shield,
  CheckCircle,
  Bug,
  Rocket,
  Minus,
  X,
} from "lucide-react";
import type { LauncherPage } from "@/lib/types";
import { useEffect, useState } from "react";
import { UpdateNotification } from "@/components/ui/UpdateNotification";
import { Tooltip } from "@/components/ui/Tooltip";

interface NavAction {
  page: LauncherPage;
  icon: React.ReactNode;
  tooltip: string;
}

const allNavActions: NavAction[] = [
  { page: "settings", icon: <Settings size={16} />, tooltip: "Settings" },
  { page: "security", icon: <Shield size={16} />, tooltip: "Security" },
  { page: "verify", icon: <CheckCircle size={16} />, tooltip: "Verify Files" },
  { page: "debug", icon: <Bug size={16} />, tooltip: "Diagnostics" },
];

const baseNavActions = allNavActions.filter((a) => a.page !== "debug");

async function minimizeWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().minimize();
  } catch {}
}

async function closeWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  } catch {}
}

async function startDrag(e: React.MouseEvent) {
  if ((e.target as HTMLElement).closest("button, a")) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().startDragging();
  } catch {}
}

export function TopBar() {
  const { currentPage, setPage, isAutoVerifying, isGameRunning } =
    useLauncherStore();

  const navLocked = isAutoVerifying || isGameRunning;

  const [appVersion, setAppVersion] = useState("...");
  const [showDebug, setShowDebug] = useState(
    process.env.NODE_ENV === "development"
  );
  const [navActions, setNavActions] = useState(baseNavActions);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.ctrlKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        setShowDebug((v) => !v);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  useEffect(() => {
    import("@tauri-apps/api/app")
      .then(({ getVersion }) => getVersion())
      .then(setAppVersion)
      .catch(() => setAppVersion("Loading..."));
  }, []);

  useEffect(() => {
    import("@/lib/tauri-api")
      .then(({ getSystemInfo }) => getSystemInfo())
      .then((sysInfo) => {
        const isWindows = sysInfo.os_name.toLowerCase().includes("windows");
        // Filter out security for non-Windows users
        setNavActions(isWindows ? baseNavActions : baseNavActions.filter((a) => a.page !== "security"));
      })
      .catch(() => {
        setNavActions(baseNavActions);
      });
  }, []);

  return (
    <header
      onMouseDown={startDrag}
      className="h-12 shrink-0 bg-surface/60 border-b border-border/50 flex items-center justify-between px-4 backdrop-blur-sm z-10 select-none"
    >
      <div className="flex items-center gap-3">
        <button
          onClick={() => !navLocked && currentPage !== "main" && setPage("main")}
          disabled={navLocked || currentPage === "main"}
          className={cn(
            "flex items-center gap-2 group",
            navLocked ? "opacity-50 cursor-not-allowed" : currentPage === "main" ? "cursor-default" : "cursor-pointer"
          )}
        >
          <div className="w-7 h-7 rounded-lg bg-primary/10 flex items-center justify-center group-hover:bg-primary/20 transition-smooth">
            <Rocket size={14} className="text-primary" />
          </div>
          <div className="flex items-baseline gap-1.5">
            <span className="font-bold text-sm tracking-wide text-gradient">
              ROCKET
            </span>
            <span className="text-[9px] text-muted font-mono">{appVersion}</span>
          </div>
        </button>
        <div className="w-px h-5 bg-border/50 mx-1" />
        <nav className="flex items-center gap-0.5">
          {navActions.map((item) => (
            <Tooltip key={item.page} label={item.tooltip}>
              <button
                onClick={() =>
                  !navLocked && currentPage !== item.page && setPage(item.page)
                }
                disabled={navLocked || currentPage === item.page}
                className={cn(
                  "p-2 rounded-lg transition-smooth relative",
                  navLocked || currentPage === item.page
                    ? "cursor-default"
                    : "cursor-pointer",
                  navLocked
                    ? "opacity-50"
                    : "",
                  currentPage === item.page
                    ? "bg-primary/15 text-primary"
                    : "text-muted hover:text-foreground hover:bg-surface-hover"
                )}
              >
                {item.icon}
              </button>
            </Tooltip>
          ))}
          <div
            className="overflow-hidden transition-all duration-300 ease-in-out"
            style={{
              width: showDebug ? "2rem" : "0px",
              opacity: showDebug ? 1 : 0,
              transform: showDebug ? "scale(1)" : "scale(0.6)",
              pointerEvents: showDebug ? "auto" : "none",
            }}
          >
            <Tooltip label="Diagnostics">
              <button
                onClick={() =>
                  !navLocked && currentPage !== "debug" && setPage("debug")
                }
                disabled={navLocked || currentPage === "debug"}
                className={cn(
                  "p-2 rounded-lg transition-smooth relative",
                  navLocked || currentPage === "debug"
                    ? "cursor-default"
                    : "cursor-pointer",
                  navLocked
                    ? "opacity-50"
                    : "",
                  currentPage === "debug"
                    ? "bg-primary/15 text-primary"
                    : "text-muted hover:text-foreground hover:bg-surface-hover"
                )}
              >
                <Bug size={16} />
              </button>
            </Tooltip>
          </div>
        </nav>
      </div>
      <div className="flex items-center gap-3">
        <UpdateNotification />
        <div className="w-px h-5 bg-border/50" />

        <div className="flex items-center -mr-1">
          <Tooltip label="Minimize">
            <button
              onClick={minimizeWindow}
              className="p-2 text-muted hover:text-foreground hover:bg-surface-hover rounded-lg transition-smooth cursor-pointer"
            >
              <Minus size={14} />
            </button>
          </Tooltip>
          <Tooltip label="Close">
            <button
              onClick={closeWindow}
              className="p-2 text-muted hover:text-danger hover:bg-danger/10 rounded-lg transition-smooth cursor-pointer"
            >
              <X size={14} />
            </button>
          </Tooltip>
        </div>
      </div>
    </header>
  );
}
