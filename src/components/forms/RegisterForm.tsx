"use client";

import { useState, useEffect } from "react";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { registerOnServer } from "@/lib/tauri-api";
import { useLauncherStore } from "@/stores/launcherStore";
import { useServerStore } from "@/stores/serverStore";
import { validateEmail } from "@/lib/utils";
import { LogIn } from "lucide-react";

export function RegisterForm() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [repeatPassword, setRepeatPassword] = useState("");
  const [ticket, setTicket] = useState("");
  const [error, setError] = useState("");
  const [errorVisible, setErrorVisible] = useState(false);
  const [displayedError, setDisplayedError] = useState("");
  const [success, setSuccess] = useState(false);
  const [isLoading, setIsLoading] = useState(false);

  const { setPage } = useLauncherStore();
  const { selectedServer, selectedServerDetails } = useServerStore();

  useEffect(() => {
    if (error) {
      setDisplayedError(error);
      setErrorVisible(false);
      requestAnimationFrame(() => {
        requestAnimationFrame(() => setErrorVisible(true));
      });
      const t = setTimeout(() => setErrorVisible(false), 15000);
      return () => clearTimeout(t);
    } else {
      setErrorVisible(false);
    }
  }, [error]);

  useEffect(() => {
    if (!errorVisible && displayedError) {
      const t = setTimeout(() => { setDisplayedError(""); setError(""); }, 300);
      return () => clearTimeout(t);
    }
  }, [errorVisible, displayedError]);

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

    if (password.length < 4) {
      setError("Password must be at least 4 characters.");
      return;
    }

    if (password !== repeatPassword) {
      setError("Passwords do not match.");
      return;
    }

    setIsLoading(true);
    try {
      const details = selectedServerDetails || (selectedServer ? await import("@/lib/tauri-api").then(m => m.fetchServerDetails(selectedServer.ip)).catch(() => null) : null);
      const result = await registerOnServer(
        selectedServer.ip,
        email,
        password,
        ticket || undefined,
        details?.modernAuthSupport,
        details?.authHash
      );
      if (result.success) {
        setSuccess(true);
      } else {
        setError(result.error || "Registration failed.");
      }
    } catch {
      setError("Connection error with server.");
    } finally {
      setIsLoading(false);
    }
  }

  if (success) {
    return (
      <div className="flex flex-col items-center text-center animate-scale-in h-full">
        <div className="flex-1 flex flex-col items-center justify-center gap-3">
          <div className="text-success text-3xl">✓</div>
          <h2 className="text-lg font-bold">Registration Successful!</h2>
        </div>
        <Button className="w-full mt-8" onClick={() => setPage("main")}>
          <LogIn size={14} className="mr-2" />
          Back to SIGN IN
        </Button>
      </div>
    );
  }

  return (
    <>
      {displayedError && (
        <div className="absolute bottom-full left-0 right-0 mb-2 z-10">
          <p className={`text-xs text-danger bg-[#1a0a0a] border border-danger/30 rounded-lg px-3 py-2 shadow-lg transition-all duration-300 ease-out ${
            errorVisible ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2"
          }`}>
            {displayedError}
          </p>
        </div>
      )}

      <form onSubmit={handleSubmit} className="space-y-3 animate-fade-in-up">
      <Input
        label="Email"
        type="email"
        placeholder="your@email.com"
        value={email}
        onChange={(e) => setEmail(e.target.value)}
        autoComplete="email"
      />
      <Input
        label="Password"
        type="password"
        placeholder="••••••••"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        autoComplete="new-password"
      />
      <Input
        label="Confirm Password"
        type="password"
        placeholder="••••••••"
        value={repeatPassword}
        onChange={(e) => setRepeatPassword(e.target.value)}
        autoComplete="new-password"
      />

      {selectedServerDetails?.requireTicket && (
        <Input
          label="Invitation Ticket"
          type="text"
          placeholder="Ticket required by this server"
          value={ticket}
          onChange={(e) => setTicket(e.target.value)}
        />
      )}

      <div className="flex items-center gap-2 pt-1">
        <Button
          type="submit"
          isLoading={isLoading}
          disabled={!selectedServer}
          className="flex-1"
        >
          Register
        </Button>
        <Button
          type="button"
          variant="ghost"
          onClick={() => setPage("main")}
        >
          Back
        </Button>
      </div>
    </form>
    </>
  );
}
