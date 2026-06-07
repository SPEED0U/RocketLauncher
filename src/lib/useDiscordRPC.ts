"use client";

import { useEffect, useRef, useState } from "react";
import { useLauncherStore } from "@/stores/launcherStore";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import {
  discordRpcInit,
  discordRpcUpdate,
} from "@/lib/tauri-api";
import { getVersion } from "@tauri-apps/api/app";

const PAGE_LABELS: Record<string, { details: string; smallImage: string }> = {
  main: { details: "At main screen", smallImage: "official" },
  settings: { details: "In settings", smallImage: "screen_settings" },
  security: { details: "In security center", smallImage: "screen_settings" },
  verify: { details: "Verifying files", smallImage: "screen_verify" },
  register: { details: "Registering", smallImage: "screen_register" },
  debug: { details: "Debug screen", smallImage: "screen_settings" },
  servers: { details: "Browsing servers", smallImage: "official" },
};

export function useDiscordRPC() {
  const { currentPage, gameStatus, isLoggedIn } = useLauncherStore();
  const { selectedServer } = useServerStore();
  const { settings } = useSettingsStore();
  const initialized = useRef(false);
  const wasInGame = useRef(false);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const isDevServer = selectedServer?.category?.toUpperCase() === "DEV";

  useEffect(() => {
    getVersion().then(setAppVersion).catch(() => setAppVersion(null));
  }, []);

  useEffect(() => {
    if (settings.disableRPC || isDevServer) {
      return;
    }

    discordRpcInit()
      .then(() => { initialized.current = true; })
      .catch(() => {});
  }, [settings.disableRPC, isDevServer]);

  useEffect(() => {
    if (settings.disableRPC || isDevServer || !initialized.current) return;

    const update = async () => {
      if (gameStatus === "launching" || gameStatus === "running") {
        wasInGame.current = true;
        return;
      }

      if (wasInGame.current) {
        wasInGame.current = false;
        try {
          await discordRpcInit();
        } catch {}
      }

      const pageInfo = PAGE_LABELS[currentPage] || PAGE_LABELS.main;

      try {
        await discordRpcUpdate({
          details: isLoggedIn
            ? "Ready to race"
            : selectedServer
              ? `${selectedServer.name}`
              : "Selecting server",
          state: appVersion ? `Rocket Launcher ${appVersion}` : "Rocket Launcher",
          largeImage: "nfsw",
          largeText: "Launcher",
          smallImage: pageInfo.smallImage,
          smallText: pageInfo.details,
          button1Label: "Project Site",
          button1Url: "https://soapboxrace.world",
        });
      } catch {}
    };

    update();
  }, [currentPage, gameStatus, isLoggedIn, selectedServer, settings.disableRPC, isDevServer, appVersion]);
}
