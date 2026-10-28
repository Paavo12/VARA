import { getContext, setContext } from "svelte";
import type { Session } from "@fuber/api-client";

const KEY = Symbol("fuber-session");

/** Makes the signed-in session available to every account screen below this component. */
export function provideSession(session: Session): void {
  setContext(KEY, session);
}

export function useSession(): Session {
  const s = getContext<Session | undefined>(KEY);
  if (!s)
    throw new Error("provideSession() must be called in a parent component");
  return s;
}

/** The user-facing message from an API error body, or a generic one. */
export function messageOf(
  error: unknown,
  fallback = "Something went wrong. Please try again.",
): string {
  const e = error as { error?: { message?: unknown } } | undefined;
  return typeof e?.error?.message === "string" ? e.error.message : fallback;
}
