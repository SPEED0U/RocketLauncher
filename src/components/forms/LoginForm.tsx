"use client";

import { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { loginToServer, pickGameFolder, launchGame, fetchModInfo, downloadModNetModules, downloadMods, cleanMods, grantFolderPermissions, recoverPassword, checkFileExists, checkProcessRunning } from "@/lib/tauri-api";
import { isCloudDrivePath } from "@/lib/utils";
import { URLS } from "@/lib/urls";
import { useLauncherStore } from "@/stores/launcherStore";
import { useServerStore } from "@/stores/serverStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { useCredentialsStore } from "@/stores/credentialsStore";
import { usePlaytimeStore } from "@/stores/playtimeStore";
import { Play, LogOut, Download, UserPlus } from "lucide-react";
import { Tooltip } from "@/components/ui/Tooltip";
import { validateEmail, maskEmail } from "@/lib/utils";
import { open } from "@tauri-apps/plugin-shell";
import { listen } from "@tauri-apps/api/event";

function useAlert(value: string) {
  const [visible, setVisible] = useState(false);
  const [displayed, setDisplayed] = useState("");

  useEffect(() => {
    if (value) {
      setDisplayed(value);
      setVisible(false);
      requestAnimationFrame(() => requestAnimationFrame(() => setVisible(true)));
      const t = setTimeout(() => setVisible(false), 15000);
      return () => clearTimeout(t);
    }
    setVisible(false);
  }, [value]);

  useEffect(() => {
    if (!visible && displayed) {
      const t = setTimeout(() => setDisplayed(""), 300);
      return () => clearTimeout(t);
    }
  }, [visible, displayed]);

  return { visible, displayed };
}

interface LoginFormProps {
  needsGameFiles?: boolean;
  isDownloading?: boolean;
  onDownloadGame?: () => void;
  canDownload?: boolean;
  displayedServerId?: string | null;
}

export function LoginForm({ needsGameFiles, isDownloading, onDownloadGame, canDownload, displayedServerId }: LoginFormProps) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState("");
  const [info, setInfo] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isRecovering, setIsRecovering] = useState(false);

  const { visible: errorVisible, displayed: displayedError } = useAlert(error);
  const { visible: infoVisible, displayed: displayedInfo } = useAlert(info);

  const { isLoggedIn, isGameRunning, gameStatus, isAutoVerifying, setAuth, setPage, setGameRunning, setGameStatus, setDownloadProgress, logout } = useLauncherStore();
  const { selectedServer, selectedServerDetails } = useServerStore();
  const { settings, setSettings } = useSettingsStore();
  const { saveCredentials, getCredentials } = useCredentialsStore();
  const { addSeconds } = usePlaytimeStore();

  useEffect(() => {
    if (!displayedServerId) return;
    const saved = getCredentials(displayedServerId);
    setEmail(saved?.email ?? "");
    setPassword(saved?.password ?? "");
  }, [displayedServerId, getCredentials]);

  useEffect(() => {
    if (gameStatus !== "running") return;

    let cancelled = false;
    const checkInterval = setInterval(async () => {
      if (cancelled) return;
      const isRunning = await checkProcessRunning("nfsw.exe");
      if (!isRunning && !cancelled) {
        if (settings.installationDirectory) {
          await cleanMods(settings.installationDirectory).catch(() => {});
        }
        setGameStatus("idle");
      }
    }, 2000);

    return () => {
      cancelled = true;
      clearInterval(checkInterval);
    };
  }, [gameStatus, setGameStatus, settings.installationDirectory]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError("");

    if (!selectedServer) {
      setError("Please select a server first.");
      return;
    }

    if (!validateEmail(email)) {
      setError("Invalid email address.");
      return;
    }

    if (!password) {
      setError("Password is required.");
      return;
    }

    setIsLoading(true);
    try {
      const details = selectedServer ? await import("@/lib/tauri-api").then(m => m.fetchServerDetails(selectedServer.ip)).catch(() => null) : null;
      const result = await loginToServer(
        selectedServer.ip,
        email,
        password,
        details?.modernAuthSupport,
        details?.authHash
      );
      if (result.success && result.token) {
        setAuth(email, result.token, result.userId || "0");
        saveCredentials(selectedServer.id, email, password);
      } else if (result.banned) {
        setError(`Account banned: ${result.banned.reason} (expires: ${result.banned.expires})`);
        setPassword("");
      } else {
        setError(result.error || "Authentication failed.");
        setPassword("");
      }
    } catch {
      setError("Failed to connect to server.");
      setPassword("");
    } finally {
      setIsLoading(false);
    }
  }

  async function handlePlay() {
    if (!selectedServer || !settings.installationDirectory) return;
    try {
      setDownloadProgress({ status: "verifying", fileName: "Preparing..." });
      await cleanMods(settings.installationDirectory).catch(() => {});
      await grantFolderPermissions(settings.installationDirectory).catch(() => {});

      const modInfo = await fetchModInfo(selectedServer.ip).catch(() => null);
      if (modInfo?.base_path && modInfo?.server_id) {
        setDownloadProgress({ status: "downloading", fileName: "Installing ModNet modules..." });
        await downloadModNetModules(settings.installationDirectory, URLS.MODNET_CDN);
        setDownloadProgress({ status: "downloading", fileName: "Downloading mods..." });
        await downloadMods(modInfo.base_path, modInfo.server_id, settings.installationDirectory);
        setDownloadProgress({ status: "idle" });
      }

      const freshState = useLauncherStore.getState();
      const uid = String(freshState.userId || "0");
      const tok = String(freshState.loginToken || "");
      if (!tok || !uid || uid === "undefined") {
        logout();
        throw new Error("Invalid session — please sign in again");
      }

      setGameStatus("launching");

      const discordAppId = selectedServerDetails?.discordApplicationID || selectedServer.discordAppId;
      await launchGame(
        settings.installationDirectory,
        selectedServer.id,
        selectedServer.name,
        selectedServer.ip,
        tok,
        uid,
        discordAppId,
        settings.closeOnGameExit,
        settings.disableProxy
      );

      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          unlisten.then(fn => fn());
          reject(new Error("Game did not connect to server within 3 minutes"));
        }, 3 * 60 * 1000);

        const unlisten = listen("game-running", () => {
          clearTimeout(timeout);
          unlisten.then(fn => fn());
          resolve();
        });
      });

      setGameStatus("running");

      let sessionStart: number | null = null;
      const capturedServerId = selectedServer?.id ?? "unknown";

      let crashed = false;

      const unlistenExit = await listen("game-exited", () => {
        if (sessionStart !== null) {
          const elapsed = Math.floor((Date.now() - sessionStart) / 1000);
          if (elapsed > 0) addSeconds(capturedServerId, elapsed);
          sessionStart = null;
        }
        if (!crashed) {
          setGameStatus("idle");
          logout();
        }
        unlistenExit();
      });

      const unlistenCrash = await listen("game-crashed", () => {
        crashed = true;
        if (sessionStart !== null) {
          const elapsed = Math.floor((Date.now() - sessionStart) / 1000);
          if (elapsed > 0) addSeconds(capturedServerId, elapsed);
          sessionStart = null;
        }
        setError("The game crashed. You have been signed out — mod files have been cleaned.");
        setTimeout(() => {
          setGameStatus("idle");
          setTimeout(() => logout(), 300);
        }, 600);
        unlistenCrash();
      });

      sessionStart = Date.now();
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error("Failed to launch game:", msg);
      setError(msg);
      setGameStatus("idle");
    }
  }

  return (
    <>
      {(displayedError || displayedInfo) && (
        <div className="absolute bottom-full left-0 right-0 mb-2 space-y-1.5 z-10">
          {displayedError && (
            <p className={`text-xs text-danger bg-[#1a0a0a] border border-danger/30 rounded-lg px-3 py-2 shadow-lg transition-all duration-300 ease-out ${
              errorVisible ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2"
            }`}>
              {displayedError}
            </p>
          )}
          {displayedInfo && (
            <p className={`text-xs text-success bg-[#0a1a0a] border border-success/30 rounded-lg px-3 py-2 shadow-lg transition-all duration-300 ease-out ${
              infoVisible ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2"
            }`}>
              {displayedInfo}
            </p>
          )}
        </div>
      )}

    <form onSubmit={isLoggedIn ? (e) => { e.preventDefault(); handlePlay(); } : handleSubmit} className="space-y-3 animate-fade-in-up min-h-[13.5rem]">
      <Input
        label="Email"
        type="email"
        placeholder="you@example.com"
        value={settings.streamingSupport ? maskEmail(email) : email}
        onChange={(e) => setEmail(e.target.value)}
        autoComplete="email"
        disabled={isLoggedIn}
        readOnly={settings.streamingSupport && !isLoggedIn}
      />
      <Input
        label="Password"
        type="password"
        placeholder="••••••••"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        autoComplete="current-password"
        disabled={isLoggedIn}
      />

      <div>
        <button
          type="button"
          disabled={isRecovering || isLoggedIn}
          className={`text-[11px] transition-smooth cursor-pointer disabled:opacity-40 ${isLoggedIn ? "text-muted cursor-not-allowed pointer-events-none" : "text-primary/70 hover:text-primary"}`}
          onClick={async () => {
            const url = selectedServerDetails?.webRecoveryUrl?.trim();
            if (url) {
              open(url).catch(() => {});
              return;
            }
            if (!selectedServer) {
              setError("Please select a server first.");
              return;
            }
            if (!email || !validateEmail(email)) {
              setError("Enter a valid email to recover your password.");
              return;
            }
            setIsRecovering(true);
            setError("");
            setInfo("");
            try {
              const result = await recoverPassword(selectedServer.ip, email);
              if (result.success) {
                setInfo(result.message);
              } else {
                setError(result.message);
              }
            } catch {
              setError("Failed to send recovery request.");
            } finally {
              setIsRecovering(false);
            }
          }}
        >
          {isRecovering ? "Sending..." : "Forgot Password?"}
        </button>
      </div>

      <div className="flex items-center gap-2 pt-1 animate-fade-in-up">
        {isLoggedIn ? (
          <>
            <Button
              type="button"
              disabled={isGameRunning || isAutoVerifying || !settings.installationDirectory || needsGameFiles}
              className="flex-1 bg-amber-500 hover:bg-amber-400 text-white animate-soft-pulse font-black tracking-widest uppercase glow-accent"
              onClick={handlePlay}
            >
              <Play size={16} className="mr-2" />
              <span key={gameStatus} className="animate-fade-in">
                {gameStatus === "launching" ? "LAUNCHING..." : gameStatus === "running" ? "GAME RUNNING" : "PLAY"}
              </span>
            </Button>
          </>
        ) : !settings.installationDirectory ? (
          <Button
            type="button"
            className="flex-1 bg-yellow-500 hover:bg-yellow-400 text-white animate-soft-pulse"
            onClick={async () => {
              const folder = await pickGameFolder();
              if (folder) {
                if (isCloudDrivePath(folder)) {
                  setError("Cloud storage folders (OneDrive, Google Drive\u2026) are not supported. Please choose a local directory.");
                  return;
                }
                setSettings({ installationDirectory: folder });
              }
            }}
          >
            Select Directory
          </Button>
        ) : needsGameFiles ? (
          <Button
            type="button"
            disabled={!canDownload || isDownloading || isAutoVerifying}
            className="flex-1 bg-amber-500 hover:bg-amber-400 text-white animate-soft-pulse"
            onClick={onDownloadGame}
          >
            <Download size={16} className="mr-2" />
            <span key={isDownloading ? "dl" : "idle"} className="animate-fade-in">
              {isDownloading ? "Downloading..." : "Download Game"}
            </span>
          </Button>
        ) : (
          <Button
            type="submit"
            isLoading={isLoading}
            disabled={!selectedServer || isAutoVerifying}
            className="flex-1"
          >
            SIGN IN
          </Button>
        )}
        <Tooltip label={isLoggedIn ? "Sign out" : "Register"}>
          <Button
            type="button"
            variant="ghost"
            className={`self-stretch px-3 transition-colors duration-200 ${isLoggedIn ? "text-muted hover:text-danger hover:bg-danger/10 disabled:opacity-30 disabled:pointer-events-none" : "text-muted-foreground hover:text-foreground hover:bg-surface-hover"}`}
            disabled={isLoggedIn ? isGameRunning : false}
            onClick={isLoggedIn ? logout : () => {
              const url = selectedServerDetails?.webSignupUrl?.trim();
              if (url) {
                open(url).catch(() => {});
              } else {
                setPage("register");
              }
            }}
          >
            <span className="relative w-4 h-4 flex items-center justify-center">
              <LogOut
                size={16}
                className="absolute transition-all duration-200"
                style={{
                  opacity: isLoggedIn ? 1 : 0,
                  transform: isLoggedIn ? "scale(1) rotate(0deg)" : "scale(0.5) rotate(-45deg)",
                }}
              />
              <UserPlus
                size={16}
                className="absolute transition-all duration-200"
                style={{
                  opacity: isLoggedIn ? 0 : 1,
                  transform: isLoggedIn ? "scale(0.5) rotate(45deg)" : "scale(1) rotate(0deg)",
                }}
              />
            </span>
          </Button>
        </Tooltip>
      </div>
    </form>
    </>
  );
}
