"use client";

import { useEffect, useState } from "react";
import Lottie from "lottie-react";
import { useLauncherStore } from "@/stores/launcherStore";
import { useServerStore } from "@/stores/serverStore";
import rocketAnimation from "../../../public/animations/rocket.json";

type Phase = "idle" | "visible" | "leaving";

export function RocketLaunchOverlay() {
  const { gameStatus } = useLauncherStore();
  const { selectedServer } = useServerStore();
  const [show, setShow] = useState(false);
  const [phase, setPhase] = useState<Phase>("idle");

  useEffect(() => {
    if (gameStatus === "launching") {
      setShow(true);
      requestAnimationFrame(() => requestAnimationFrame(() => setPhase("visible")));
    } else {
      setPhase("leaving");
      const t = setTimeout(() => { setShow(false); setPhase("idle"); }, 700);
      return () => clearTimeout(t);
    }
  }, [gameStatus]);

  if (!show) return null;

  return (
    <div
      className="fixed inset-0 z-9999 flex items-center justify-center"
      style={{
        background: "radial-gradient(ellipse at 50% 80%, #130a2e 0%, #080718 50%, #020209 100%)",
        opacity: phase === "visible" ? 1 : 0,
        transition: "opacity 0.5s ease",
      }}
    >
      <div className="flex flex-col items-center gap-6">
        <Lottie
          animationData={rocketAnimation}
          loop
          autoplay
          style={{ width: 320, height: 320 }}
        />
        <p className="text-white/50 text-[10px] font-mono tracking-[0.5em] uppercase animate-soft-pulse">
          Launching Game
        </p>
        {selectedServer && (
          <p className="text-white/30 text-[9px] font-mono tracking-[0.35em] uppercase -mt-4">
            Destination&nbsp;<span className="text-white/60">{selectedServer.name}</span>
          </p>
        )}
      </div>
    </div>
  );
}

