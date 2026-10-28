/**
 * Normalises what someone types as a Namibian mobile number to E.164 (`+264…`), mirroring the
 * server's rules so mistakes show before a code is requested. The server re-checks.
 */
export function normalisePhone(raw: string): string | null {
  const compact = raw.replace(/[\s()-]/g, "");
  let national: string;
  if (compact.startsWith("+264")) national = compact.slice(4);
  else if (compact.startsWith("00264")) national = compact.slice(5);
  else if (compact.startsWith("0")) national = compact.slice(1);
  else if (/^[1-9]\d{7,8}$/.test(compact)) national = compact;
  else return null;
  return /^[1-9]\d{7,8}$/.test(national) ? `+264${national}` : null;
}

/** `YYYY-MM-DD` of the latest birth date that is 18 or older today (for the date picker `max`). */
export function adultMaxDate(today = new Date()): string {
  const d = new Date(
    Date.UTC(today.getFullYear() - 18, today.getMonth(), today.getDate()),
  );
  return d.toISOString().slice(0, 10);
}
