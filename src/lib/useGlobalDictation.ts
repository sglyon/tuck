import { useCallback, useEffect, useRef, useState } from "react";
import { call, native } from "./bridge";
import type { GlobalDictationStatus } from "./types";

export function useGlobalDictation() {
  const [status, setStatus] = useState<GlobalDictationStatus | null>(null);
  const [pending, setPending] = useState("");
  const [error, setError] = useState("");
  const [dismissed, setDismissed] = useState(false);
  const mounted = useRef(true);
  const revision = useRef(0);
  const flight = useRef<Promise<void> | null>(null);
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
  const run = useCallback((method: string): Promise<void> => {
    if (!native) return Promise.resolve();
    if (flight.current) return flight.current;
    const stamp = revision.current;
    setPending(method);
    setError("");
    const task = (async () => {
      try {
        const next = await call<GlobalDictationStatus>(method);
        if (!next || typeof next.state !== "string") throw new Error("Global dictation is unavailable in this desktop build.");
        if (mounted.current && revision.current === stamp) receive(next);
      } catch (e) {
        if (mounted.current && revision.current === stamp) setError(e instanceof Error ? e.message : String(e));
      } finally {
        if (mounted.current) setPending("");
      }
    })();
    flight.current = task;
    void task.then(() => { if (flight.current === task) flight.current = null; });
    return task;
  }, [receive]);
  const refresh = useCallback(() => run("global_dictation_status"), [run]);
  const toggle = useCallback(() => run("global_dictation_toggle"), [run]);
  const cancel = useCallback(() => run("global_dictation_cancel"), [run]);
  return { status, pending, error, dismissed, receive, refresh, toggle, cancel, dismiss: () => setDismissed(true) };
}
export type GlobalDictationController = ReturnType<typeof useGlobalDictation>;
