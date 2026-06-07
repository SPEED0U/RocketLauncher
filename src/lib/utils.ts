import { clsx, type ClassValue } from "clsx";

export function cn(...inputs: ClassValue[]) {
  return clsx(inputs);
}

export function formatBytes(bytes: number, decimals = 2): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${parseFloat((bytes / Math.pow(k, i)).toFixed(dm))} ${sizes[i]}`;
}

export function formatSpeed(bytesPerSec: number): string {
  return `${formatBytes(bytesPerSec)}/s`;
}

export function formatETA(seconds: number): string {
  const safeSeconds = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0;
  const h = Math.floor(safeSeconds / 3600);
  const m = Math.floor((safeSeconds % 3600) / 60);
  const s = Math.floor(safeSeconds % 60);
  if (h > 0) {
    return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
  }
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
}

const CLOUD_DRIVE_SEGMENTS = [
  "onedrive",
  "google drive",
  "googledrive",
  "dropbox",
  "icloud drive",
  "iclouddrive",
];

export function isCloudDrivePath(path: string): boolean {
  const lower = path.replace(/\\/g, "/").toLowerCase();
  return CLOUD_DRIVE_SEGMENTS.some((seg) => lower.includes(seg));
}

export function formatPing(ms: number): string {
  if (ms < 0) return "N/A";
  if (ms < 50) return `${ms}ms`;
  if (ms < 100) return `${ms}ms`;
  return `${ms}ms`;
}

export function getPingColor(ms: number): string {
  if (ms < 0) return "text-muted";
  if (ms < 50) return "text-success";
  if (ms < 100) return "text-warning";
  return "text-danger";
}

export function validateEmail(email: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email);
}

export function maskEmail(email: string): string {
  if (!email || !email.includes("@")) return "EMAIL IS HIDDEN";
  
  try {
    const [localPart, domain] = email.split("@");
    if (localPart.length <= 2) {
      return "*".repeat(localPart.length) + "@" + domain;
    }
    
    const firstChar = localPart[0];
    const lastChar = localPart[localPart.length - 1];
    const maskedMiddle = "*".repeat(localPart.length - 2);
    
    return `${firstChar}${maskedMiddle}${lastChar}@${domain}`;
  } catch {
    return "EMAIL IS HIDDEN";
  }
}
