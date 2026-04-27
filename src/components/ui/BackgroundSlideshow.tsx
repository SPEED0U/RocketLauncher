"use client";

import { useState, useEffect, useRef } from "react";

async function isTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

const SLIDESHOW_IMAGES = [
  "/imgs/01.png",
  "/imgs/02.png",
  "/imgs/03.png",
  "/imgs/04.png",
  "/imgs/05.png",
  "/imgs/06.png",
  "/imgs/07.png",
  "/imgs/08.png",
  "/imgs/09.png",
  "/imgs/10.png",
];

const DURATION_MS = 15000;
const CROSSFADE_MS = 4000;

interface SlideState {
  imageIndex: number;
  startScale: number;
  endScale: number;
  startX: number;
  endX: number;
  startY: number;
  endY: number;
  key: number;
}

function generateSlideAnimation(imageIndex: number, key: number): SlideState {
  const startScale = 1.0;
  const endScale = 1.15 + Math.random() * 0.15;
  const startX = 0;
  const endX = (Math.random() - 0.5) * 8;
  const startY = 0;
  const endY = (Math.random() - 0.5) * 8;
  
  return {
    imageIndex,
    startScale,
    endScale,
    startX,
    endX,
    startY,
    endY,
    key,
  };
}

export function BackgroundSlideshow({ disabled }: { disabled?: boolean }) {
  const disabledRef = useRef(disabled);
  disabledRef.current = disabled;

  const [gameRunning, setGameRunning] = useState(false);
  const gameRunningRef = useRef(false);

  useEffect(() => {
    let unlistenRunning: (() => void) | undefined;
    let unlistenExited: (() => void) | undefined;

    isTauri().then((tauri) => {
      if (!tauri) return;
      import("@tauri-apps/api/event").then(({ listen }) => {
        listen("game-running", () => {
          gameRunningRef.current = true;
          setGameRunning(true);
        }).then((fn) => { unlistenRunning = fn; });
        listen("game-exited", () => {
          gameRunningRef.current = false;
          setGameRunning(false);
        }).then((fn) => { unlistenExited = fn; });
      });
    });

    return () => {
      unlistenRunning?.();
      unlistenExited?.();
    };
  }, []);

  const [slideA, setSlideA] = useState<SlideState>(() =>
    generateSlideAnimation(disabled ? Math.floor(Math.random() * SLIDESHOW_IMAGES.length) : 0, 0)
  );
  const [slideB, setSlideB] = useState<SlideState>(() => generateSlideAnimation(1, 1));
  const [showA, setShowA] = useState(true);
  const [nextImageIndex, setNextImageIndex] = useState(2);
  const [animKey, setAnimKey] = useState(2);
  const [firstLoaded, setFirstLoaded] = useState(false);

  useEffect(() => {
    const img = new window.Image();
    img.onload = () => setFirstLoaded(true);
    img.onerror = () => setFirstLoaded(true);
    img.src = SLIDESHOW_IMAGES[slideA.imageIndex];
  }, []);

  useEffect(() => {
    if (disabledRef.current) return;
    const interval = setInterval(() => {
      if (disabledRef.current || gameRunningRef.current) return;
      const nextSlide = generateSlideAnimation(nextImageIndex, animKey);
      
      if (showA) {
        setSlideB(nextSlide);
      } else {
        setSlideA(nextSlide);
      }
      
      setTimeout(() => {
        setShowA(!showA);
      }, 50);
      
      setNextImageIndex((nextImageIndex + 1) % SLIDESHOW_IMAGES.length);
      setAnimKey(prev => prev + 1);
    }, DURATION_MS);

    return () => clearInterval(interval);
  }, [showA, nextImageIndex, animKey]);

  return (
    <div className="absolute inset-0 overflow-hidden" style={{ opacity: firstLoaded ? 1 : 0, transition: "opacity 0.3s ease" }}>
      <div
        key={`slide-a-${slideA.key}`}
        className="absolute inset-0 bg-cover bg-center will-change-transform"
        style={{
          backgroundImage: `url(${SLIDESHOW_IMAGES[slideA.imageIndex]})`,
          animation: (disabled || gameRunning) ? "none" : `kenBurns-a-${slideA.key} ${DURATION_MS + CROSSFADE_MS}ms linear forwards`,
          opacity: showA ? 1 : 0,
          transition: `opacity ${CROSSFADE_MS}ms ease-in-out`,
          zIndex: showA ? 2 : 1,
        }}
      />
      <div
        key={`slide-b-${slideB.key}`}
        className="absolute inset-0 bg-cover bg-center will-change-transform"
        style={{
          backgroundImage: `url(${SLIDESHOW_IMAGES[slideB.imageIndex]})`,
          animation: (disabled || gameRunning) ? "none" : `kenBurns-b-${slideB.key} ${DURATION_MS + CROSSFADE_MS}ms linear forwards`,
          opacity: showA ? 0 : 1,
          transition: `opacity ${CROSSFADE_MS}ms ease-in-out`,
          zIndex: showA ? 1 : 2,
        }}
      />
      <style jsx>{`
        @keyframes kenBurns-a-${slideA.key} {
          from {
            transform: scale(${slideA.startScale}) translate(${slideA.startX}%, ${slideA.startY}%);
          }
          to {
            transform: scale(${slideA.endScale}) translate(${slideA.endX}%, ${slideA.endY}%);
          }
        }
        @keyframes kenBurns-b-${slideB.key} {
          from {
            transform: scale(${slideB.startScale}) translate(${slideB.startX}%, ${slideB.startY}%);
          }
          to {
            transform: scale(${slideB.endScale}) translate(${slideB.endX}%, ${slideB.endY}%);
          }
        }
      `}</style>
      <div className="absolute inset-0 bg-background/60" style={{ zIndex: 10 }} />
    </div>
  );
}
