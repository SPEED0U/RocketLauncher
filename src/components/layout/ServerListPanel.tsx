"use client";

import { useEffect, useState, useRef, useLayoutEffect, useMemo } from "react";
import { cn } from "@/lib/utils";
import { useServerStore } from "@/stores/serverStore";
import { useLauncherStore } from "@/stores/launcherStore";
import { fetchServerList, measureServerInformationLatency } from "@/lib/tauri-api";
import type { ServerInfo } from "@/lib/types";
import {
  Plus,
  X,
  Globe,
  Users,
  Shield,
  RefreshCw,
  ArrowUpDown,
  ChevronUp,
  ChevronDown,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { getServerIcon } from "@/lib/serverIcons";
import { Tooltip } from "@/components/ui/Tooltip";

export function ServerListPanel() {
  const {
    servers,
    customServers,
    selectedServer,
    serverOrder,
    setServers,
    selectServer,
    addCustomServer,
    removeCustomServer,
    setServerOrder,
    setLoading,
    isLoading,
  } = useServerStore();
  const { setPage, isLoggedIn, isAutoVerifying, downloadProgress } = useLauncherStore();

  const serverLocked = isAutoVerifying || downloadProgress.status === "downloading" || downloadProgress.status === "extracting";

  const [showAdd, setShowAdd] = useState(false);
  const [reorderMode, setReorderMode] = useState(false);
  const [newName, setNewName] = useState("");
  const [newIp, setNewIp] = useState("");

  const allServers = useMemo(
    () => [...servers, ...customServers],
    [servers, customServers]
  );
  const customServerIds = useMemo(
    () => new Set(customServers.map((s) => s.id)),
    [customServers]
  );
  const filteredServers = useMemo(() => {
    if (serverOrder.length === 0) return allServers;
    const byId = new Map(allServers.map((s) => [s.id, s]));
    const orderSet = new Set(serverOrder);
    const ordered = serverOrder
      .map((id) => byId.get(id))
      .filter((s): s is ServerInfo => s !== undefined);
    const remaining = allServers.filter((s) => !orderSet.has(s.id));
    return [...ordered, ...remaining];
  }, [allServers, serverOrder]);

  const pingIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const pingAbortRef = useRef(false);

  async function fetchPingFromServerInfo(serverIp: string): Promise<number> {
    return measureServerInformationLatency(serverIp).catch(() => -1);
  }

  async function pingAllServers(serverList: ServerInfo[], force = false, allowRetry = true) {
    pingAbortRef.current = false;
    const toPing = force
      ? serverList.filter((s) => s.ip)
      : serverList.filter((s) => s.ip && s.ping === undefined);
    const batchSize = 5;
    const results: number[] = [];

    for (let i = 0; i < toPing.length; i += batchSize) {
      if (pingAbortRef.current) break;
      const batch = toPing.slice(i, i + batchSize);
      const batchResults = await Promise.all(
        batch.map(async (s) => {
          const ping = await fetchPingFromServerInfo(s.ip);
          return { id: s.id, ping };
        })
      );
      if (!pingAbortRef.current) {
        const store = useServerStore.getState();
        store.updateServerPings(batchResults);
        results.push(...batchResults.map((r) => r.ping));
      }
    }

    // Some users get a transient startup glitch where every first ping is 1ms.
    // If that pattern is detected, run exactly one retry pass.
    const allOnes = results.length > 1 && results.every((p) => p === 1);
    if (force && allowRetry && allOnes && !pingAbortRef.current) {
      await pingAllServers(serverList, true, false);
    }
  }

  async function loadServers() {
    setLoading(true);
    try {
      const list = await fetchServerList();
      const offlineList = list.map((s) => ({ ...s, ping: -1 as number }));
      setServers(offlineList);
      const combined = [...offlineList, ...customServers];

      const previousId = selectedServer?.id;
      if (previousId) {
        const found = combined.find((s) => s.id === previousId);
        if (found) {
          selectServer({ ...found, ping: -1 });
        } else {
          selectServer(combined[0] || null);
        }
      } else if (combined.length > 0) {
        selectServer(combined[0]);
      }
      pingAllServers(combined, true);

      if (previousId) {
        setTimeout(() => {
          const store = useServerStore.getState();
          const current = store.selectedServer;
          if (current && current.id === previousId && current.ping === -1) {
            const online = [...store.servers, ...store.customServers].find((s) => s.ping !== undefined && s.ping >= 0);
            if (online) store.selectServer(online);
          }
        }, 5000);
      }
    } catch {
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (allServers.length === 0) return;

    pingAllServers(allServers);

    pingIntervalRef.current = setInterval(() => {
      if (typeof document !== "undefined" && document.visibilityState === "hidden") {
        return;
      }
      const store = useServerStore.getState();
      pingAllServers([...store.servers, ...store.customServers], true);
    }, 60_000);

    return () => {
      pingAbortRef.current = true;
      if (pingIntervalRef.current) clearInterval(pingIntervalRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [servers.length, customServers.length]);

  useEffect(() => {
    if (servers.length === 0) {
      loadServers();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [servers.length]);

  function handleSelect(server: ServerInfo) {
    if (serverLocked) return;
    if (server.id !== selectedServer?.id) {
      selectServer(server);
    }
    setPage("main");
    if (server.ip) {
      fetchPingFromServerInfo(server.ip).then((ping) => {
        useServerStore.getState().updateServerPing(server.id, ping);
      }).catch(() => {});
    }
  }

  function handleAdd() {
    if (!newName || !newIp) return;
    addCustomServer({
      id: `custom-${Date.now()}`,
      name: newName,
      ip: newIp,
      category: "Custom",
    });
    setNewName("");
    setNewIp("");
    setShowAdd(false);
  }

  return (
    <aside className="relative w-70 shrink-0 bg-surface/40 border-r border-border/50 flex flex-col h-full">
      <div className="p-3 space-y-2">
        <div className="flex items-center justify-between">
          <span className="text-[10px] text-muted uppercase tracking-widest">
            Servers
            <span className="ml-1.5 text-muted-foreground font-mono">
              {filteredServers.length}
            </span>
          </span>
          <div className="flex items-center gap-1">
            <Tooltip label={showAdd ? "Cancel" : "Add server"}>
              <button
                onClick={() => setShowAdd(!showAdd)}
                disabled={reorderMode}
                className="p-1 rounded text-muted hover:text-foreground hover:bg-surface-hover transition-smooth cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
              >
                {showAdd ? <X size={12} /> : <Plus size={12} />}
              </button>
            </Tooltip>
            <Tooltip label={reorderMode ? "Done reordering" : "Reorder servers"}>
              <button
                onClick={() => setReorderMode((v) => !v)}
                className={cn(
                  "p-1 rounded transition-smooth cursor-pointer",
                  reorderMode
                    ? "text-primary bg-primary/10"
                    : "text-muted hover:text-foreground hover:bg-surface-hover"
                )}
              >
                <ArrowUpDown size={12} />
              </button>
            </Tooltip>
            <Tooltip label={reorderMode ? "Not available while reordering" : "Refresh"}>
              <button
                onClick={loadServers}
                disabled={isLoading || reorderMode}
                className="p-1 rounded text-muted hover:text-foreground hover:bg-surface-hover transition-smooth cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
              >
                <RefreshCw
                  size={12}
                  className={isLoading ? "animate-spin" : ""}
                />
              </button>
            </Tooltip>
          </div>
        </div>
      </div>
      <div
        className={cn(
          "absolute left-0 right-0 top-[3.1rem] px-3 z-20 transition-all duration-250 ease-out",
          showAdd
            ? "opacity-100 translate-y-0"
            : "opacity-0 -translate-y-1 pointer-events-none"
        )}
      >
        <div className="min-h-0 overflow-hidden">
          <div className="bg-background border border-border/60 rounded-xl p-3 space-y-2 shadow-xl">
            <input
              type="text"
              placeholder="Server name"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              className="w-full rounded-md border border-border/50 bg-background/50 px-2.5 py-1.5 text-xs text-foreground placeholder:text-muted/50 focus:outline-none focus:ring-1 focus:ring-primary/30 transition-smooth"
            />
            <input
              type="text"
              placeholder="http://server.example.com/engine.svc"
              value={newIp}
              onChange={(e) => setNewIp(e.target.value)}
              className="w-full rounded-md border border-border/50 bg-background/50 px-2.5 py-1.5 text-xs text-foreground placeholder:text-muted/50 focus:outline-none focus:ring-1 focus:ring-primary/30 transition-smooth"
            />
            <div className="flex gap-1.5">
              <Button size="sm" onClick={handleAdd} className="flex-1 text-[11px]">
                Add
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => setShowAdd(false)}
                className="text-[11px]"
              >
                Cancel
              </Button>
            </div>
          </div>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto px-2 pb-2 space-y-1" style={{ contain: "layout" }}>
        {isLoading && filteredServers.length === 0 ? (
          <div className="text-center py-12 text-muted">
            <RefreshCw
              size={18}
              className="mx-auto mb-2 animate-spin text-primary"
            />
            <p className="text-[11px]">Loading servers...</p>
          </div>
        ) : filteredServers.length === 0 ? (
          <div className="text-center py-12 text-muted">
            <p className="text-[11px]">No servers found.</p>
          </div>
        ) : (
          <AnimatedServerList
            servers={filteredServers}
            customServerIds={customServerIds}
            selectedId={selectedServer?.id}
            serverLocked={serverLocked}
            isLoggedIn={isLoggedIn}
            reorderMode={reorderMode}
            onReorder={setServerOrder}
            onSelect={handleSelect}
            onRemoveCustomServer={removeCustomServer}
          />
        )}
      </div>
    </aside>
  );
}

function AnimatedServerList({
  servers,
  customServerIds,
  selectedId,
  serverLocked,
  isLoggedIn,
  reorderMode,
  onReorder,
  onSelect,
  onRemoveCustomServer,
}: {
  servers: ServerInfo[];
  customServerIds: Set<string>;
  selectedId?: string;
  serverLocked: boolean;
  isLoggedIn: boolean;
  reorderMode: boolean;
  onReorder: (orderedIds: string[]) => void;
  onSelect: (s: ServerInfo) => void;
  onRemoveCustomServer: (id: string) => void;
}) {
  const itemRefs = useRef<Record<string, HTMLDivElement | null>>({});
  const prevPositionsRef = useRef<Record<string, number>>({});
  // Initialized with current IDs so pre-existing servers don't animate on first render
  const prevServerIdsRef = useRef<string[]>(servers.map((s) => s.id));
  const pendingDeletesRef = useRef<Set<string>>(new Set());

  function capturePositions() {
    const snapshot: Record<string, number> = {};
    for (const id of Object.keys(itemRefs.current)) {
      const el = itemRefs.current[id];
      if (el) snapshot[id] = el.getBoundingClientRect().top;
    }
    prevPositionsRef.current = snapshot;
  }

  function moveServer(id: string, direction: "up" | "down") {
    capturePositions();
    const ids = servers.map((s) => s.id);
    const idx = ids.indexOf(id);
    if (idx === -1) return;
    const newIdx = direction === "up" ? idx - 1 : idx + 1;
    if (newIdx < 0 || newIdx >= ids.length) return;
    const next = [...ids];
    [next[idx], next[newIdx]] = [next[newIdx], next[idx]];
    onReorder(next);
  }

  function handleRemoveCustom(id: string) {
    if (pendingDeletesRef.current.has(id)) return;
    const el = itemRefs.current[id];
    if (!el) { onRemoveCustomServer(id); return; }

    pendingDeletesRef.current.add(id);
    el.style.transition = "opacity 180ms ease";
    el.style.opacity = "0";
    setTimeout(() => {
      pendingDeletesRef.current.delete(id);
      onRemoveCustomServer(id);
    }, 190);
  }

  // FLIP animation for reorder moves
  useLayoutEffect(() => {
    const previous = prevPositionsRef.current;
    if (Object.keys(previous).length === 0) return;

    for (const id of Object.keys(itemRefs.current)) {
      const el = itemRefs.current[id];
      if (!el) continue;
      const prev = previous[id];
      if (prev === undefined) continue;
      const curr = el.getBoundingClientRect().top;
      const delta = prev - curr;
      if (Math.abs(delta) < 1) continue;
      el.style.transition = "none";
      el.style.transform = `translateY(${delta}px)`;
      requestAnimationFrame(() => {
        el.style.transition = "transform 240ms cubic-bezier(0.22,1,0.36,1)";
        el.style.transform = "translateY(0)";
      });
    }
    prevPositionsRef.current = {};
  }, [servers]);

  // Enter animation for newly added custom servers
  useLayoutEffect(() => {
    const prevIds = prevServerIdsRef.current;
    const currentIds = servers.map((s) => s.id);
    const newCustomIds = currentIds.filter(
      (id) => !prevIds.includes(id) && customServerIds.has(id)
    );

    for (const id of newCustomIds) {
      const el = itemRefs.current[id];
      if (!el) continue;
      el.style.opacity = "0";
      requestAnimationFrame(() => {
        el.style.transition = "opacity 200ms ease";
        el.style.opacity = "1";
        setTimeout(() => {
          const node = itemRefs.current[id];
          if (node) {
            node.style.opacity = "";
            node.style.transition = "";
          }
        }, 210);
      });
    }

    prevServerIdsRef.current = currentIds;
  }, [servers, customServerIds]);

  return (
    <>
      {servers.map((server, index) => (
        <div
          key={server.id}
          ref={(el) => { itemRefs.current[server.id] = el; }}
          className="will-change-transform"
        >
          <ServerListItem
            server={server}
            isCustom={customServerIds.has(server.id)}
            isSelected={selectedId === server.id}
            isDisabled={serverLocked || server.ping === -1 || (isLoggedIn && selectedId !== server.id)}
            onClick={() => onSelect(server)}
            reorderMode={reorderMode}
            canMoveUp={index > 0}
            canMoveDown={index < servers.length - 1}
            onMoveUp={() => moveServer(server.id, "up")}
            onMoveDown={() => moveServer(server.id, "down")}
            onRemoveCustom={() => handleRemoveCustom(server.id)}
          />
        </div>
      ))}
    </>
  );
}

function getPingBars(ping: number): number {
  if (ping < 0) return 0;
  if (ping < 60) return 4;
  if (ping < 100) return 3;
  if (ping < 150) return 2;
  return 1;
}

function getPingTone(ping: number): string {
  if (ping < 0) return "text-danger";
  if (ping < 60) return "text-success";
  if (ping < 100) return "text-primary";
  if (ping < 150) return "text-warning";
  return "text-danger";
}

function getPingTooltip(ping: number): string {
  if (ping < 0) return "Ping: indisponible";
  return `Ping: ${ping} ms`;
}

function PingSignal({ ping }: { ping: number }) {
  const bars = getPingBars(ping);
  const tone = getPingTone(ping);

  return (
    <Tooltip label={getPingTooltip(ping)}>
      <span className={cn("inline-flex items-end gap-0.5 shrink-0", tone)}>
        {[1, 2, 3, 4].map((i) => (
          <span
            key={i}
            className={cn(
              "w-1 rounded-sm transition-opacity",
              i <= bars ? "opacity-100 bg-current" : "opacity-25 bg-current"
            )}
            style={{ height: `${4 + i * 2}px` }}
          />
        ))}
      </span>
    </Tooltip>
  );
}

function ServerListItem({
  server,
  isCustom,
  isSelected,
  isDisabled,
  onClick,
  reorderMode,
  canMoveUp,
  canMoveDown,
  onMoveUp,
  onMoveDown,
  onRemoveCustom,
}: {
  server: ServerInfo;
  isCustom: boolean;
  isSelected: boolean;
  isDisabled: boolean;
  onClick: () => void;
  reorderMode?: boolean;
  canMoveUp?: boolean;
  canMoveDown?: boolean;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  onRemoveCustom?: () => void;
}) {
  return (
    <div
      className={cn(
        "w-full text-left rounded-lg transition-all duration-200 group flex items-stretch relative",
        isDisabled && !isSelected
          ? "opacity-40"
          : isSelected
            ? "bg-primary/10"
            : "hover:bg-surface-hover"
      )}
    >
      <span
        className={cn(
          "absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-5 rounded-r-full bg-primary transition-all duration-200 origin-center",
          isSelected ? "opacity-100 scale-y-100" : "opacity-0 scale-y-0"
        )}
      />
      <button
        onClick={isDisabled ? undefined : onClick}
        disabled={isDisabled && !reorderMode}
        className={cn(
          "flex-1 text-left px-2.5 py-2",
          isDisabled && !isSelected ? "cursor-not-allowed" : "cursor-pointer"
        )}
      >
      <div className="flex items-center gap-2.5">
        <div className="w-8 h-8 rounded-md bg-surface-hover/80 flex items-center justify-center shrink-0 overflow-hidden">
          {getServerIcon(server.id) || server.iconUrl ? (
            // eslint-disable-next-line @next/next/no-img-element
            <img
              src={getServerIcon(server.id) || server.iconUrl}
              alt=""
              className="w-full h-full object-cover"
            />
          ) : (
            <Globe size={14} className="text-muted" />
          )}
        </div>
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5">
            <span
              className={cn(
                "text-xs font-medium truncate",
                isSelected ? "text-foreground" : "text-muted-foreground"
              )}
            >
              {server.name}
            </span>
            {server.isOfficial && (
              <Shield size={10} className="text-primary shrink-0" />
            )}
          </div>
          <div className="flex items-center gap-2 mt-0.5 text-[10px] text-muted">
            {server.onlineCount !== undefined && (
              <span className="flex items-center gap-0.5 text-success/80">
                <Users size={9} />
                {server.onlineCount}
              </span>
            )}
            {server.category && <span>{server.category}</span>}
          </div>
        </div>
        {isCustom && !reorderMode && (
          <Tooltip label="Delete custom server">
            <span
              role="button"
              tabIndex={0}
              onClick={(e) => {
                e.stopPropagation();
                onRemoveCustom?.();
              }}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  e.stopPropagation();
                  onRemoveCustom?.();
                }
              }}
              className={cn(
                "ml-1 p-0.5 rounded text-danger/80 hover:text-danger hover:bg-danger/10 transition-all cursor-pointer",
                "opacity-0 group-hover:opacity-100"
              )}
            >
              <Trash2 size={12} />
            </span>
          </Tooltip>
        )}
        <span className={cn("transition-opacity duration-200", server.ping !== undefined ? "opacity-100" : "opacity-0")}>
          <PingSignal ping={server.ping ?? -1} />
        </span>
      </div>
      </button>
      <span
        className={cn(
          "flex flex-col justify-center gap-0.5 shrink-0 overflow-hidden transition-all duration-200 pr-1",
          reorderMode ? "w-5 opacity-100" : "w-0 opacity-0"
        )}
      >
          <Tooltip label="Move up">
          <button
            onClick={(e) => { e.stopPropagation(); onMoveUp?.(); }}
            disabled={!canMoveUp}
            className="p-0.5 rounded text-muted hover:text-foreground hover:bg-surface-hover disabled:opacity-20 cursor-pointer disabled:cursor-not-allowed"
          >
            <ChevronUp size={12} />
          </button>
          </Tooltip>
          <Tooltip label="Move down">
          <button
            onClick={(e) => { e.stopPropagation(); onMoveDown?.(); }}
            disabled={!canMoveDown}
            className="p-0.5 rounded text-muted hover:text-foreground hover:bg-surface-hover disabled:opacity-20 cursor-pointer disabled:cursor-not-allowed"
          >
            <ChevronDown size={12} />
          </button>
          </Tooltip>
        </span>
    </div>
  );
}
