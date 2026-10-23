import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "./bridge";
import { cursorStale, emptyLibrary, libraryScope } from "./library";
import type {
  LibraryBootstrap,
  LibraryOverview,
  MeetingHeader,
  MeetingPage,
} from "./types";
const sortHeaders = (items: MeetingHeader[]) =>
  items.sort(
    (a, b) =>
      b.createdAt.localeCompare(a.createdAt) ||
      (a.id === b.id ? 0 : a.id < b.id ? 1 : -1),
  );
export function useLibraryIndex(
  query: string,
  starred: boolean,
  scope: { folder: string; tag: string; person: string },
) {
  const [headers, setHeaders] = useState<MeetingHeader[]>([]);
  const [overview, setOverview] = useState<LibraryOverview>(emptyLibrary);
  const [total, setTotal] = useState(0);
  const [cursor, setCursor] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [ready, setReady] = useState(false);
  const [revision, setRevision] = useState(0);
  const [installEpoch, setInstallEpoch] = useState(0);
  const [notice, setNotice] = useState("");
  const request = useRef(0);
  const installed = useRef("");
  const overviewRequest = useRef(0);
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const overviewTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const params = {
    query: query.trim(),
    starred: starred ? true : undefined,
    scope: libraryScope(scope),
    limit: 50,
  };
  const key = JSON.stringify(params);
  const latest = useRef(params);
  latest.current = params;
  const pageCursor = useRef(cursor);
  pageCursor.current = cursor;
  const pageLoading = useRef(false);
  const readyRef = useRef(false);
  const load = useCallback(async (more = false) => {
    if (more && (pageLoading.current || !pageCursor.current)) return;
    const token = ++request.current;
    const requested = latest.current;
    const requestedKey = JSON.stringify(requested);
    pageLoading.current = true;
    setLoading(true);
    setError("");
    setNotice("");
    try {
      const page = await call<MeetingPage>("list_meetings", {
        ...requested,
        ...(more ? { cursor: pageCursor.current } : {}),
      });
      if (
        token !== request.current ||
        requestedKey !== JSON.stringify(latest.current)
      )
        return;
      installed.current = requestedKey;
      setHeaders((prev) =>
        more
          ? [
              ...prev,
              ...page.meetings.filter((m) => !prev.some((p) => p.id === m.id)),
            ]
          : page.meetings,
      );
      setCursor(page.nextCursor);
      setTotal(page.total);
    } catch (e) {
      if (
        token !== request.current ||
        requestedKey !== JSON.stringify(latest.current)
      )
        return;
      if (more && cursorStale(e)) {
        pageCursor.current = null;
        setCursor(null);
        pageLoading.current = false;
        await load(false);
        setNotice(
          "Your library changed. Showing the latest conversations; your open meeting stays in place.",
        );
        return;
      }
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      if (token === request.current) {
        pageLoading.current = false;
        setLoading(false);
      }
    }
  }, []);
  const refreshOverview = useCallback(async () => {
    const token = ++overviewRequest.current;
    try {
      const value = await call<LibraryOverview>("library_overview");
      if (token === overviewRequest.current) {
        setOverview(value);
        const p = latest.current;
        if (
          !p.query &&
          !p.scope.folder &&
          !p.scope.tags?.length &&
          !p.scope.people?.length
        )
          setTotal(p.starred ? value.starredCount : value.meetingCount);
      }
    } catch {
      /* Existing counts remain; page and meeting errors are actionable separately. */
    }
  }, []);
  const changed = useCallback(() => {
    setRevision((v) => v + 1);
    if (overviewTimer.current) clearTimeout(overviewTimer.current);
    overviewTimer.current = setTimeout(() => void refreshOverview(), 250);
  }, [refreshOverview]);
  const install = useCallback((data: LibraryBootstrap) => {
    request.current++;
    overviewRequest.current++;
    pageLoading.current = false;
    setLoading(false);
    setHeaders(data.meetingPage.meetings);
    setCursor(data.meetingPage.nextCursor);
    setTotal(data.meetingPage.total);
    setOverview(data.library);
    setError("");
    installed.current = JSON.stringify({ query: "", scope: {}, limit: 50 });
    readyRef.current = true;
    setReady(true);
    setRevision((v) => v + 1);
    setInstallEpoch((v) => v + 1);
    setNotice("");
  }, []);
  const refresh = useCallback(
    (contentChanged = true) => {
      if (!readyRef.current) return;
      if (contentChanged) changed();
      // Background speech must not replace pages the user has already opened.
      if (refreshTimer.current) return;
      refreshTimer.current = setTimeout(
        async () => {
          refreshTimer.current = null;
          const p = latest.current;
          const requestedKey = JSON.stringify(p);
          if (!contentChanged) await refreshOverview();
          if (requestedKey !== JSON.stringify(latest.current)) return;
          if (p.query) {
            void load();
            return;
          }
          if (
            p.scope.folder ||
            p.scope.tags?.length ||
            p.scope.people?.length
          ) {
            try {
              const page = await call<MeetingPage>("list_meetings", {
                ...p,
                limit: 1,
              });
              if (requestedKey === JSON.stringify(latest.current))
                setTotal(page.total);
            } catch {
              /* Keep the current page available. */
            }
          }
        },
        contentChanged ? 250 : 5000,
      );
    },
    [load, changed, refreshOverview],
  );
  const patch = useCallback((id: string, changes: Partial<MeetingHeader>) => {
    setHeaders((prev) =>
      prev.map((m) => (m.id === id ? { ...m, ...changes } : m)),
    );
  }, []);
  const upsert = useCallback(
    (header: MeetingHeader, mutated = true) => {
      if (!readyRef.current) return;
      const p = latest.current;
      const matches =
        (!p.starred || header.starred) &&
        (!p.scope.folder || header.folder === p.scope.folder) &&
        (!p.scope.tags?.length ||
          p.scope.tags.every((t) => header.tags?.includes(t))) &&
        (!p.scope.people?.length ||
          p.scope.people.every((t) => header.people?.includes(t)));
      if (p.query) {
        if (mutated) refresh();
        return;
      }
      setHeaders((prev) => {
        const exists = prev.some((m) => m.id === header.id);
        if (!matches) return prev.filter((m) => m.id !== header.id);
        if (!mutated && !exists) return prev;
        return sortHeaders([...prev.filter((m) => m.id !== header.id), header]);
      });
      if (mutated) refresh();
    },
    [changed, refresh],
  );
  const remove = useCallback(
    (id: string) => {
      setHeaders((prev) => prev.filter((m) => m.id !== id));
      setTotal((v) => Math.max(0, v - 1));
      changed();
    },
    [changed],
  );
  useEffect(() => {
    if (!ready || installed.current === key) return;
    installed.current = key;
    request.current++;
    setHeaders([]);
    setCursor(null);
    pageCursor.current = null;
    setTotal(0);
    setLoading(true);
    setError("");
    setNotice("");
    const timer = setTimeout(() => void load(), query.trim() ? 250 : 0);
    return () => clearTimeout(timer);
  }, [key, ready, load, installEpoch]);
  useEffect(
    () => () => {
      request.current++;
      if (refreshTimer.current) clearTimeout(refreshTimer.current);
      if (overviewTimer.current) clearTimeout(overviewTimer.current);
    },
    [],
  );
  return {
    headers,
    overview,
    total,
    cursor,
    loading,
    error,
    notice,
    revision,
    install,
    load,
    refresh,
    upsert,
    patch,
    remove,
    changed,
    refreshOverview,
  };
}
