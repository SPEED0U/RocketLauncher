import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ServerInfo, ServerDetails } from "@/lib/types";

interface ServerState {
  servers: ServerInfo[];
  customServers: ServerInfo[];
  serverOrder: string[];
  selectedServer: ServerInfo | null;
  selectedServerDetails: ServerDetails | null;
  isLoading: boolean;
  error: string | null;

  setServers: (servers: ServerInfo[]) => void;
  updateServerPing: (id: string, ping: number) => void;
  updateServerPings: (updates: Array<{ id: string; ping: number }>) => void;
  addCustomServer: (server: ServerInfo) => void;
  removeCustomServer: (id: string) => void;
  setServerOrder: (orderedIds: string[]) => void;
  selectServer: (server: ServerInfo | null) => void;
  setServerDetails: (details: ServerDetails | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useServerStore = create<ServerState>()(
  persist(
    (set, get) => ({
      servers: [],
      customServers: [],
      serverOrder: [],
      selectedServer: null,
      selectedServerDetails: null,
      isLoading: false,
      error: null,

      setServers: (servers) =>
        set((state) => {
          const allIds = [...servers, ...state.customServers].map((s) => s.id);
          const existing = state.serverOrder.filter((id) => allIds.includes(id));
          const missing = allIds.filter((id) => !existing.includes(id));
          return {
            servers,
            serverOrder: [...existing, ...missing],
          };
        }),
      updateServerPings: (updates) =>
        set((state) => {
          if (updates.length === 0) return {};

          const pingById = new Map(updates.map((u) => [u.id, u.ping]));
          let changed = false;

          const updatedServers = state.servers.map((s) => {
            const nextPing = pingById.get(s.id);
            if (nextPing === undefined || s.ping === nextPing) return s;
            changed = true;
            return { ...s, ping: nextPing };
          });
          const updatedCustomServers = state.customServers.map((s) => {
            const nextPing = pingById.get(s.id);
            if (nextPing === undefined || s.ping === nextPing) return s;
            changed = true;
            return { ...s, ping: nextPing };
          });

          let updatedSelected = state.selectedServer;
          if (state.selectedServer) {
            const selectedPing = pingById.get(state.selectedServer.id);
            if (selectedPing !== undefined && state.selectedServer.ping !== selectedPing) {
              updatedSelected = { ...state.selectedServer, ping: selectedPing };
            }
          }

          const selectedPing = updatedSelected ? pingById.get(updatedSelected.id) : undefined;
          if (updatedSelected && selectedPing === -1) {
            const fallback = [...updatedServers, ...updatedCustomServers]
              .find((s) => s.ping !== undefined && s.ping >= 0);
            return {
              servers: updatedServers,
              customServers: updatedCustomServers,
              selectedServer: fallback ?? updatedSelected,
              selectedServerDetails: fallback ? null : state.selectedServerDetails,
            };
          }

          if (!changed && updatedSelected === state.selectedServer) return {};
          return {
            servers: updatedServers,
            customServers: updatedCustomServers,
            selectedServer: updatedSelected,
          };
        }),
      updateServerPing: (id, ping) => get().updateServerPings([{ id, ping }]),
      addCustomServer: (server) =>
        set((state) => ({
          customServers: [...state.customServers, server],
          serverOrder: state.serverOrder.includes(server.id)
            ? state.serverOrder
            : [...state.serverOrder, server.id],
        })),
      removeCustomServer: (id) =>
        set((state) => ({
          customServers: state.customServers.filter((s) => s.id !== id),
          serverOrder: state.serverOrder.filter((serverId) => serverId !== id),
          selectedServer: state.selectedServer?.id === id ? null : state.selectedServer,
          selectedServerDetails: state.selectedServer?.id === id
            ? null
            : state.selectedServerDetails,
        })),
      setServerOrder: (orderedIds) =>
        set((state) => {
          const allIds = [...state.servers, ...state.customServers].map((s) => s.id);
          const existing = orderedIds.filter((id) => allIds.includes(id));
          const missing = allIds.filter((id) => !existing.includes(id));
          return {
            serverOrder: [...existing, ...missing],
          };
        }),
      selectServer: (server) => set((state) => ({
        selectedServer: server,
        selectedServerDetails: state.selectedServer?.id === server?.id ? state.selectedServerDetails : null,
      })),
      setServerDetails: (details) =>
        set({ selectedServerDetails: details }),
      setLoading: (loading) => set({ isLoading: loading }),
      setError: (error) => set({ error }),
    }),
    {
      name: "launcher-servers",
      partials: (state: ServerState) => ({
        customServers: state.customServers,
        serverOrder: state.serverOrder,
        selectedServer: state.selectedServer,
      }),
    } as never
  )
);
