/** German display labels for administration states and scan weekday masks. */

const RUN_STATUS: Record<string, string> = {
  queued: "Eingeplant", running: "Läuft", completed: "Abgeschlossen", ready: "Bereit", partial: "Teilweise",
  failed: "Fehlgeschlagen", cancelled: "Abgebrochen", "timed-out": "Zeitlimit",
};

/** Job and scan run status (`completed` → "Abgeschlossen"); unknown values pass through. */
export function runStatusLabel(status: string): string {
  return RUN_STATUS[status] ?? status;
}

/** Outcome of the last scheduled scan trigger. */
export function scheduleOutcomeLabel(outcome: "enqueued" | "coalesced" | "blocked" | null): string {
  if (outcome === null) return "noch nicht ausgeführt";
  return { enqueued: "eingereiht", coalesced: "mit laufendem Scan zusammengefasst", blocked: "blockiert" }[outcome];
}

/** Account role as shown in lists and selects. */
export function roleLabel(role: string): string {
  return ({ owner: "Owner", administrator: "Administrator", editor: "Editor", viewer: "Viewer" } as Record<string, string>)[role] ?? role;
}

/** Account status in German. */
export function accountStatusLabel(status: string): string {
  return ({ active: "aktiv", disabled: "deaktiviert", locked: "gesperrt", invited: "eingeladen" } as Record<string, string>)[status] ?? status;
}

/** Background component keys from the operations health report. */
export function componentLabel(key: string): string {
  return ({ "scan-worker": "Scan-Worker", "preview-worker": "Vorschau-Worker", scheduler: "Zeitplaner", backup: "Sicherung" } as Record<string, string>)[key] ?? key;
}

/** Weekdays in mask bit order: bit 0 is Monday, matching the scheduler. */
export const WEEKDAYS = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"] as const;

/** Combine selected weekday bit indexes (0 = Monday) into a scheduler mask. */
export function weekdayMask(days: Iterable<number>): number {
  let mask = 0;
  for (const day of days) {
    if (Number.isInteger(day) && day >= 0 && day < 7) mask |= 1 << day;
  }
  return mask;
}

/** Human summary of a weekday mask, e.g. "täglich", "Mo–Fr" or "Mo, Mi, Fr". */
export function describeWeekdays(mask: number): string {
  const days = WEEKDAYS.filter((_, index) => (mask & (1 << index)) !== 0);
  if (days.length === 7) return "täglich";
  if (days.length === 0) return "an keinem Tag";
  if (mask === 0b0011111) return "Mo–Fr";
  if (mask === 0b1100000) return "Sa–So";
  return days.join(", ");
}
