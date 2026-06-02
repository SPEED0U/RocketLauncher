"use client";

import { useEffect, useRef, useState } from "react";
import { useLauncherStore } from "@/stores/launcherStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { TopBar } from "@/components/layout/TopBar";
import { ServerListPanel } from "@/components/layout/ServerListPanel";
import { SplashScreen } from "@/components/screens/SplashScreen";
import { WelcomeScreen } from "@/components/screens/WelcomeScreen";
import { MainScreen } from "@/components/screens/MainScreen";
import { SettingsScreen } from "@/components/screens/SettingsScreen";
import { DebugScreen } from "@/components/screens/DebugScreen";
import { RegisterScreen } from "@/components/screens/RegisterScreen";
import { UpdatePopup } from "@/components/screens/UpdatePopup";
import { BackgroundSlideshow } from "@/components/ui/BackgroundSlideshow";
import { RocketLaunchOverlay } from "@/components/ui/RocketLaunchOverlay";
import { useDiscordRPC } from "@/lib/useDiscordRPC";
import { checkProcessRunning, cleanMods, hasPendingModCleanup } from "@/lib/tauri-api";

function ContentPanel() {
  const { currentPage, setPage } = useLauncherStore();
  const [displayedPage, setDisplayedPage] = useState(currentPage);
  const [transitionClass, setTransitionClass] = useState("animate-fade-in");
  const [isWindows, setIsWindows] = useState(true);
  const [sysInfoLoaded, setSysInfoLoaded] = useState(false);

  useEffect(() => {
    import("@/lib/tauri-api")
      .then(({ getSystemInfo }) => getSystemInfo())
      .then((sysInfo) => {
        const isWin = sysInfo.os_name.toLowerCase().includes("windows");
        setIsWindows(isWin);
        // Redirect to main if trying to access security on non-Windows
        if (!isWin && currentPage === "security") {
          setPage("main");
        }
      })
      .catch(() => setIsWindows(true))
      .finally(() => setSysInfoLoaded(true));

    if (currentPage !== displayedPage) {
      setTransitionClass("animate-fade-out");
      const timer = setTimeout(() => {
        setDisplayedPage(currentPage);
        setTransitionClass("animate-fade-in");
      }, 200);
      return () => clearTimeout(timer);
    }
  }, [currentPage, displayedPage]);

  const content = (() => {
    switch (displayedPage) {
      case "settings":
        return <SettingsScreen />;
      case "debug":
        return <DebugScreen />;
      case "register":
        return <RegisterScreen />;
      default:
        return <MainScreen />;
    }
  })();

  if (!sysInfoLoaded) {
    return <div className="flex-1 flex flex-col min-h-0" />;
  }

  return (
    <div key={displayedPage} className={`flex-1 flex flex-col min-h-0 ${transitionClass}`}>
      {content}
    </div>
  );
}

export default function Home() {
  const { currentPage } = useLauncherStore();
  const { settings } = useSettingsStore();
  useDiscordRPC();

  const cleanedRef = useRef(false);
  useEffect(() => {
    if (!settings.installationDirectory || cleanedRef.current) return;

    let cancelled = false;
    const installDir = settings.installationDirectory;

    const tryCleanup = async () => {
      if (cancelled || cleanedRef.current) return;

      const isRunning = await checkProcessRunning("nfsw.exe").catch(() => false);
      if (cancelled || cleanedRef.current) return;

      // Avoid touching active mod links while the game process is still alive.
      if (isRunning) {
        setTimeout(() => {
          void tryCleanup();
        }, 2000);
        return;
      }

      const needsCleanup = await hasPendingModCleanup(installDir).catch(() => true);
      if (cancelled || cleanedRef.current) return;

      if (!needsCleanup) {
        cleanedRef.current = true;
        return;
      }

      cleanedRef.current = true;
      await cleanMods(installDir).catch(() => {});
    };

    void tryCleanup();

    return () => {
      cancelled = true;
    };
  }, [settings.installationDirectory]);
  useEffect(() => {
    if (process.env.NODE_ENV !== "production") return;
    const handler = (e: KeyboardEvent) => {
      if (
        e.key === "F5" ||
        e.key === "F12" ||
        (e.ctrlKey && e.shiftKey && ["I", "J", "C"].includes(e.key)) ||
        (e.ctrlKey && e.key === "u")
      ) {
        e.preventDefault();
      }
    };
    const ctxHandler = (e: MouseEvent) => e.preventDefault();
    window.addEventListener("keydown", handler);
    window.addEventListener("contextmenu", ctxHandler);
    return () => {
      window.removeEventListener("keydown", handler);
      window.removeEventListener("contextmenu", ctxHandler);
    };
  }, []);

  if (currentPage === "splash") {
    return <SplashScreen />;
  }

  if (currentPage === "welcome") {
    return <WelcomeScreen />;
  }

  return (
    <div className="flex flex-col w-full h-full">
      <TopBar />
      <div className="flex flex-1 min-h-0">
        <ServerListPanel />
        <main className="flex-1 flex flex-col overflow-hidden min-w-0 relative">
          <BackgroundSlideshow disabled={settings.disableSlideshow} />
          <div className="relative z-10 flex-1 flex flex-col min-h-0">
            <ContentPanel />
          </div>
        </main>
      </div>
      <UpdatePopup latestVersion="" />
      <RocketLaunchOverlay />
    </div>
  );
}
