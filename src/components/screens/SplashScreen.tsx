"use client";

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useLauncherStore } from "@/stores/launcherStore";
import { Rocket } from "lucide-react";

export function SplashScreen() {
  const { setPage, setInitialized } = useLauncherStore();
  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState("Initializing...");

  useEffect(() => {
    invoke("show_window").catch(() => {});
    const steps = [
      { msg: "Loading configuration...", duration: 400 },
      { msg: "Checking for updates...", duration: 600 },
      { msg: "Fetching server list...", duration: 500 },
      { msg: "Loading CDN list...", duration: 300 },
      { msg: "Preparing launcher...", duration: 400 },
    ];

    let current = 0;
    const perStep = 100 / steps.length;

    function runStep() {
      if (current >= steps.length) {
        setInitialized(true);
        setPage("main");
        return;
      }
      setStatus(steps[current].msg);
      setProgress((current + 1) * perStep);
      current++;
      setTimeout(runStep, steps[current - 1].duration);
    }

    const timeout = setTimeout(runStep, 500);
    return () => clearTimeout(timeout);
  }, [setPage, setInitialized]);

  return (
    <div className="fixed inset-0 bg-background flex flex-col items-center justify-center z-50">
      <div className="flex flex-col items-center gap-5">
        <div className="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center">
          <Rocket size={32} className="text-primary animate-pulse" />
        </div>
        <h1 className="text-2xl font-bold text-gradient tracking-tight">Rocket Launcher</h1>
        <p className="text-muted text-xs">{status}</p>

        <div className="w-56 h-1 bg-border/50 rounded-full overflow-hidden">
          <div
            className="h-full bg-primary rounded-full transition-all duration-500"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
    </div>
  );
}
