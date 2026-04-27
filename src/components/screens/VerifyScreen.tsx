"use client";

import { useState, useEffect, useRef } from "react";
import { Button } from "@/components/ui/Button";
import { ProgressBar } from "@/components/ui/ProgressBar";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog";
import { useLauncherStore } from "@/stores/launcherStore";
import { useSettingsStore } from "@/stores/settingsStore";
import { verifyGameFiles, repairGameFiles, removeServerMods } from "@/lib/tauri-api";
import {
  CheckCircle,
  AlertTriangle,
  FileWarning,
  Play,
  RotateCcw,
  Trash2,
  Shield,
  FolderX,
} from "lucide-react";

export function VerifyScreen() {
  const { verifyProgress, setVerifyProgress, isAutoVerifying, setAutoVerifying } = useLauncherStore();
  const { settings } = useSettingsStore();
  const [corruptedList, setCorruptedList] = useState<string[]>([]);
  const [isRepairing, setIsRepairing] = useState(false);
  const [isRemovingMods, setIsRemovingMods] = useState(false);
  const [modRemovalError, setModRemovalError] = useState("");
  const [modRemovalSuccess, setModRemovalSuccess] = useState(false);
  const [showModRemovalConfirm, setShowModRemovalConfirm] = useState(false);
  const unlistenRef = useRef<(() => void) | null>(null);

  const isRunning = verifyProgress.status === "scanning" || verifyProgress.status === "repairing";

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
          setVerifyProgress({
            status: d.status === "completed" ? "completed" : "scanning",
            currentFile: d.current_file,
            currentIndex: d.current_index,
            totalFiles: d.total_files,
            corruptedFiles: verifyProgress.corruptedFiles,
          });
        });
        unlistenRef.current = unlisten;
      } catch {
      }
    })();
    return () => {
      cancelled = true;
      unlistenRef.current?.();
    };
  }, [setVerifyProgress]);

  async function startVerification() {
    if (!settings.installationDirectory || !settings.selectedCDN) return;

    setCorruptedList([]);
    setVerifyProgress({
      status: "scanning",
      currentFile: "",
      currentIndex: 0,
      totalFiles: 0,
      corruptedFiles: [],
    });
    setAutoVerifying(true);

    try {
      const corrupted = await verifyGameFiles(
        settings.selectedCDN,
        settings.installationDirectory
      );
      setCorruptedList(corrupted);
      setVerifyProgress({
        status: "completed",
        corruptedFiles: corrupted,
      });
    } catch (err) {
      setVerifyProgress({
        status: "error",
        currentFile: err instanceof Error ? err.message : String(err),
      });
    } finally {
      setAutoVerifying(false);
    }
  }

  async function handleRepair() {
    if (!settings.installationDirectory || !settings.selectedCDN || corruptedList.length === 0) return;

    setIsRepairing(true);
    setVerifyProgress({ status: "repairing", currentIndex: 0, totalFiles: corruptedList.length, currentFile: "" });
    setAutoVerifying(true);

    let unlistenRepair: (() => void) | null = null;
    try {
      const { listen } = await import("@tauri-apps/api/event");
      unlistenRepair = await listen<{
        status: string;
        current_file: string;
        current_index: number;
        total_files: number;
        corrupted_count: number;
      }>("verify-progress", (event) => {
        const d = event.payload;
        if (d.status === "repairing") {
          setVerifyProgress({
            status: "repairing",
            currentIndex: d.current_index,
            totalFiles: d.total_files,
            currentFile: d.current_file,
          });
        }
      });
    } catch {}

    try {
      await repairGameFiles(
        settings.selectedCDN,
        settings.installationDirectory,
        corruptedList
      );
      setCorruptedList([]);
      setVerifyProgress({
        status: "completed",
        corruptedFiles: [],
      });
    } catch (err) {
      setVerifyProgress({
        status: "error",
        currentFile: err instanceof Error ? err.message : String(err),
      });
    } finally {
      unlistenRepair?.();
      setIsRepairing(false);
      setAutoVerifying(false);
    }
  }

  async function handleRemoveServerMods() {
    if (!settings.installationDirectory) return;

    setIsRemovingMods(true);
    setModRemovalError("");
    setModRemovalSuccess(false);

    try {
      await removeServerMods(settings.installationDirectory);
      setModRemovalSuccess(true);
      setTimeout(() => setModRemovalSuccess(false), 3000);
    } catch (err) {
      setModRemovalError(err instanceof Error ? err.message : String(err));
    } finally {
      setIsRemovingMods(false);
    }
  }

  const percent =
    verifyProgress.totalFiles > 0
      ? (verifyProgress.currentIndex / verifyProgress.totalFiles) * 100
      : 0;

  return (
    <div className="flex-1 relative flex items-center justify-center p-4 overflow-hidden">
      <div className="absolute top-4 left-4 right-4 flex items-start justify-between">
        <div>
          <h1 className="text-base font-bold">File Verification</h1>
          <p className="text-[11px] text-muted mt-0.5 font-mono truncate max-w-xs">
            {settings.installationDirectory || "No game directory configured"}
          </p>
        </div>
        {!settings.selectedCDN && (
          <span className="text-[10px] text-danger border border-danger/30 rounded px-2 py-0.5">
            No CDN selected
          </span>
        )}
      </div>
      <div className="w-full grid grid-cols-[3fr_2fr] grid-rows-[190px] gap-3">
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <Shield size={15} className="text-primary shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">File Integrity</h2>
              <p className="text-[10px] text-muted">Verify and repair corrupted game files</p>
            </div>
            <div className="ml-auto flex gap-2">
              <Button
                size="sm"
                onClick={startVerification}
                disabled={!settings.installationDirectory || !settings.selectedCDN || isRunning}
                isLoading={verifyProgress.status === "scanning"}
                className="h-7 px-3 text-[11px]"
              >
                <Play size={11} className="mr-1" />
                {verifyProgress.status === "scanning" ? "Scanning..." : "Verify"}
              </Button>
              {verifyProgress.status === "completed" && corruptedList.length > 0 && (
                <Button
                  variant="accent"
                  size="sm"
                  onClick={handleRepair}
                  isLoading={isRepairing}
                  className="h-7 px-3 text-[11px]"
                >
                  <RotateCcw size={11} className="mr-1" />
                  Repair ({corruptedList.length})
                </Button>
              )}
            </div>
          </div>

          <div className="px-4 py-3 space-y-3 flex-1 overflow-hidden">
            {(verifyProgress.status === "scanning" || verifyProgress.status === "repairing") && (
              <div className="space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-[11px] font-medium">
                    {verifyProgress.status === "repairing" ? "Repairing..." : "Scanning..."}
                  </span>
                  <span className="text-[10px] text-muted font-mono">
                    {verifyProgress.currentIndex} / {verifyProgress.totalFiles}
                  </span>
                </div>
                <ProgressBar value={percent} variant="primary" showPercent />
                {verifyProgress.currentFile && (
                  <p className="text-[10px] text-muted truncate font-mono">{verifyProgress.currentFile}</p>
                )}
              </div>
            )}
            {verifyProgress.status === "completed" && corruptedList.length === 0 && (
              <div className="flex items-center gap-2.5 border border-success/30 bg-success/5 rounded-lg px-3 py-2.5">
                <CheckCircle size={15} className="text-success shrink-0" />
                <div>
                  <p className="text-[12px] font-medium text-success">All files intact</p>
                  <p className="text-[10px] text-muted">{verifyProgress.totalFiles} files verified</p>
                </div>
              </div>
            )}
            {verifyProgress.status === "completed" && corruptedList.length > 0 && (
              <div className="border border-danger/30 bg-danger/5 rounded-lg px-3 py-2.5">
                <div className="flex items-center gap-2 mb-2">
                  <AlertTriangle size={13} className="text-danger shrink-0" />
                  <p className="text-[12px] font-medium text-danger">
                    {corruptedList.length} corrupted file{corruptedList.length > 1 ? "s" : ""}
                  </p>
                </div>
                <div className="space-y-0.5 max-h-14 overflow-y-auto">
                  {corruptedList.map((file, i) => (
                    <div key={i} className="flex items-center gap-1.5 text-[10px] font-mono text-danger/80">
                      <FileWarning size={10} />
                      {file}
                    </div>
                  ))}
                </div>
              </div>
            )}
            {verifyProgress.status === "error" && (
              <div className="flex items-center gap-2.5 border border-danger/30 bg-danger/5 rounded-lg px-3 py-2.5">
                <AlertTriangle size={13} className="text-danger shrink-0" />
                <p className="text-[11px] text-danger">{verifyProgress.currentFile || "Verification failed"}</p>
              </div>
            )}
            {verifyProgress.status === "idle" && (
              <p className="text-[11px] text-muted">Click Verify to scan game files integrity.</p>
            )}
          </div>
        </section>
        <section className="border border-border rounded-xl bg-surface overflow-hidden flex flex-col">
          <div className="flex items-center gap-2.5 px-4 py-3 border-b border-border/50 shrink-0">
            <FolderX size={15} className="text-warning shrink-0" />
            <div>
              <h2 className="text-xs font-bold tracking-wide uppercase">Server Mods</h2>
              <p className="text-[10px] text-muted">Remove MODS and .data folders</p>
            </div>
          </div>

          <div className="px-4 py-3 space-y-3 flex-1 overflow-hidden">
            <p className="text-[11px] text-muted leading-relaxed">
              Removes server-side modifications downloaded during gameplay. Use this if you encounter mod conflicts.
            </p>

            <Button
              variant="danger"
              size="sm"
              onClick={() => setShowModRemovalConfirm(true)}
              disabled={!settings.installationDirectory || isRemovingMods}
              isLoading={isRemovingMods}
              className="w-full h-7 text-[11px]"
            >
              <Trash2 size={11} className="mr-1.5" />
              Remove Server Mods
            </Button>

            {modRemovalSuccess && (
              <div className="flex items-center gap-2 border border-success/30 bg-success/5 rounded-lg px-3 py-2">
                <CheckCircle size={13} className="text-success" />
                <p className="text-[11px] text-success">Removed successfully</p>
              </div>
            )}

            {modRemovalError && (
              <div className="flex items-center gap-2 border border-danger/30 bg-danger/5 rounded-lg px-3 py-2">
                <AlertTriangle size={13} className="text-danger" />
                <p className="text-[11px] text-danger">{modRemovalError}</p>
              </div>
            )}
          </div>
        </section>

      </div>

      <ConfirmDialog
        isOpen={showModRemovalConfirm}
        onClose={() => setShowModRemovalConfirm(false)}
        onConfirm={handleRemoveServerMods}
        title="Remove Server Mods"
        message="This will delete the MODS and .data folders. This action cannot be undone. Continue?"
        confirmText="Remove"
        cancelText="Cancel"
        variant="danger"
      />
    </div>
  );
}
