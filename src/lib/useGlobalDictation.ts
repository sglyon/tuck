import { useCallback, useEffect, useRef, useState } from "react";
import { call, native } from "./bridge";
import type { GlobalDictationConfig, GlobalDictationStatus } from "./types";

export function useGlobalDictation() {
  const [status, setStatus] = useState<GlobalDictationStatus | null>(null);
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [dismissed, setDismissed] = useState(false);
  const mounted = useRef(true);
  const revision = useRef(0);
  const flight = useRef<Promise<boolean> | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const receive = useCallback((value: GlobalDictationStatus) => {
    if (!value || typeof value.state !== "string") return;
    revision.current++;
    if (mounted.current) {
      setStatus(value);
      setError("");
      setDismissed(false);
    }
  }, []);
  const run = useCallback((method: string, params?: GlobalDictationConfig): Promise<boolean> => {
    if (!native) return Promise.resolve(false);
    if (flight.current) return flight.current;
    const stamp = revision.current;
    setPending(method);
    setError("");
    const task = (async () => {
      try {
        const next = await call<GlobalDictationStatus>(method, params ? {...params} : undefined);
        if (!next || typeof next.state !== "string") throw new Error("Global dictation is unavailable in this desktop build.");
        if (mounted.current && revision.current === stamp) receive(next);
        return true;
      } catch (e) {
        if (mounted.current && (method === "global_dictation_configure" || revision.current === stamp)) setError(e instanceof Error ? e.message : String(e));
        return false;
      } finally {
        if (mounted.current) setPending("");
      }
    })();
    flight.current = task;
    void task.then(() => { if (flight.current === task) flight.current = null; });
    return task;
  }, [receive]);
  const refresh = useCallback(async () => { await run("global_dictation_status"); }, [run]);
  const toggle = useCallback(async () => { await run("global_dictation_toggle"); }, [run]);
  const cancel = useCallback(async () => { await run("global_dictation_cancel"); }, [run]);
  const configure = useCallback((config: GlobalDictationConfig) => run("global_dictation_configure", config), [run]);
  return { status, pending, error, dismissed, receive, refresh, toggle, cancel, configure, dismiss: () => setDismissed(true) };
}
export type GlobalDictationController = ReturnType<typeof useGlobalDictation>;
