"use client";

import { Button } from "@/components/ui/Button";
import { Modal } from "@/components/ui/Modal";
import { useLauncherStore } from "@/stores/launcherStore";
import { Download, SkipForward } from "lucide-react";
import { formatVersionForDisplay } from "@/lib/config";

interface UpdatePopupProps {
  latestVersion: string;
  changelog?: string;
  downloadUrl?: string;
}

export function UpdatePopup({
  latestVersion,
  changelog,
  downloadUrl,
}: UpdatePopupProps) {
  const { showUpdatePopup, setShowUpdatePopup } = useLauncherStore();
  const displayVersion = formatVersionForDisplay(latestVersion);

  function handleUpdate() {
    if (downloadUrl) {
      window.open(downloadUrl, "_blank", "noopener,noreferrer");
    }
    setShowUpdatePopup(false);
  }

  return (
    <Modal
      isOpen={showUpdatePopup}
      onClose={() => setShowUpdatePopup(false)}
      title="Update Available"
      size="md"
    >
      <div className="space-y-4">
        <p className="text-xs text-muted">
          A new version of the launcher is available:{" "}
          <span className="text-foreground font-semibold">{displayVersion}</span>
        </p>

        {changelog && (
          <div className="bg-background/50 border border-border/50 rounded-xl p-3 max-h-48 overflow-y-auto">
            <h4 className="text-[10px] font-semibold uppercase tracking-widest text-muted mb-2">
              Release Notes
            </h4>
            <p className="text-xs text-muted-foreground whitespace-pre-wrap">
              {changelog}
            </p>
          </div>
        )}

        <div className="flex gap-2 justify-end pt-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowUpdatePopup(false)}
          >
            <SkipForward size={14} className="mr-1" />
            Skip
          </Button>
          <Button size="sm" onClick={handleUpdate}>
            <Download size={14} className="mr-1" />
            Update
          </Button>
        </div>
      </div>
    </Modal>
  );
}
