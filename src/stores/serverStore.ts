import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { ServerInfo, ServerDetails } from "@/lib/types";

interface ServerState {
  servers: ServerInfo[];
  customServers: ServerInfo[];
  selectedServer: ServerInfo | null;
  selectedServerDetails: ServerDetails | null;
  isLoading: boolean;
  error: string | null;

  setServers: (servers: ServerInfo[]) => void;
  updateServerPing: (id: string, ping: number) => void;
  addCustomServer: (server: ServerInfo) => void;
  removeCustomServer: (id: string) => void;
  selectServer: (server: ServerInfo | null) => void;
  setServerDetails: (details: ServerDetails | null) => void;
  setLoading: (loading: boolean) => void;
  setError: (error: string | null) => void;
}

export const useServerStore = create<ServerState>()(
  persist(
    (set) => ({
      servers: [],
      customServers: [],
      selectedServer: null,
      selectedServerDetails: null,
      isLoading: false,
      error: null,

      setServers: (servers) => set({ servers }),
      updateServerPing: (id, ping) =>
        set((state) => {
          const updatedServers = state.servers.map((s) =>
            s.id === id ? { ...s, ping } : s
          );
          const updatedSelected =
            state.selectedServer?.id === id
              ? { ...state.selectedServer, ping }
              : state.selectedServer;

          // Si le serveur sélectionné passe offline, chercher un autre serveur online
          if (state.selectedServer?.id === id && ping === -1) {
            const fallback = updatedServers.find((s) => s.ping !== undefined && s.ping >= 0);
            return {
              servers: updatedServers,
              selectedServer: fallback ?? updatedSelected,
              selectedServerDetails: fallback ? null : state.selectedServerDetails,
            };
          }

          return {
            servers: updatedServers,
            selectedServer: updatedSelected,
          };
        }),
      addCustomServer: (server) =>
        set((state) => ({
          customServers: [...state.customServers, server],
        })),
      removeCustomServer: (id) =>
        set((state) => ({
          customServers: state.customServers.filter((s) => s.id !== id),
        })),
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
        selectedServer: state.selectedServer,
      }),
    } as never
  )
);
