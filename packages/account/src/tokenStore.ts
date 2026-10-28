import { memoryTokenStore, type TokenStore } from "@fuber/api-client";
import { Capacitor } from "@capacitor/core";

/**
 * Where the refresh token is kept (TRD §7.4): the iOS Keychain / Android Keystore on phones.
 * In a desktop browser (development) it is held in memory only, so a reload signs you out:
 * web storage is readable by any script on the page and is never used for tokens.
 */
export function deviceTokenStore(app: "rider" | "driver"): TokenStore {
  if (!Capacitor.isNativePlatform()) return memoryTokenStore();
  const key = `fuber.${app}.refresh`;
  const plugin = import("@aparajita/capacitor-secure-storage").then(
    (m) => m.SecureStorage,
  );
  return {
    async get() {
      const s = await plugin;
      const v = await s.get(key).catch(() => null);
      return typeof v === "string" ? v : null;
    },
    async set(value) {
      const s = await plugin;
      if (value === null) await s.remove(key).catch(() => undefined);
      else await s.set(key, value);
    },
  };
}
