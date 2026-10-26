/** Latest scan run of a library, with live progress (ADR 0035). */
export interface LatestScan {
  id: string;
  status: "queued" | "running" | "completed" | "failed" | "cancelled";
  full: boolean;
  discoveredFiles: number;
  hashedFiles: number;
  attempt: number;
  cancellationRequested: boolean;
  errorMessage: string | null;
}

/** `POST /api/v1/libraries/dry-run`: what a first scan of a path would do. */
export interface DryRunReport {
  canonicalPath: string;
  files: number;
  bytes: number;
  directories: number;
  cadFiles: number;
  formats: Record<string, number>;
  otherFiles: number;
  symlinksSkipped: number;
  blockers: Array<{ code: string; path: string }>;
  blockerCount: number;
  largestFiles: Array<{ path: string; bytes: number }>;
  overlaps: Array<{ key: string; path: string; relation: "same" | "inside" | "contains" }>;
  truncated: boolean;
  elapsedMs: number;
}

const BLOCKER_LABEL: Record<string, string> = {
  directory_unreadable: "Verzeichnis nicht lesbar",
  entry_unreadable: "Eintrag nicht lesbar",
  name_not_utf8: "Dateiname ist kein gültiges UTF-8",
  path_invalid: "Pfad wird vom Katalog nicht akzeptiert",
};
const OVERLAP_LABEL = { same: "ist bereits registriert als", inside: "liegt innerhalb von", contains: "enthält die Bibliothek" } as const;

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GiB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${bytes} B`;
}

export function dryRunMarkup(report: DryRunReport): string {
  const formats = Object.entries(report.formats).map(([format, count]) => `${escapeMarkup(format.toUpperCase())} ${count}`).join(" · ");
  const summary = `<p><strong>Probelauf für ${escapeMarkup(report.canonicalPath)}:</strong> ${report.files} Dateien · ${formatBytes(report.bytes)} · ${report.directories} Ordner · ${report.cadFiles} CAD-Dateien${formats ? ` (${formats})` : ""} · ${report.otherFiles} weitere Dateien${report.symlinksSkipped ? ` · ${report.symlinksSkipped} Symlinks werden übersprungen` : ""}.</p>`;
  const truncated = report.truncated ? '<p class="form-message">Der Probelauf wurde an seiner Grenze beendet; die Zahlen sind Mindestwerte.</p>' : "";
  const blockers = report.blockerCount === 0
    ? '<p class="dry-run-ok">Keine Blocker: Ein erster Scan kann den Ordner vollständig indexieren.</p>'
    : `<div class="dry-run-blockers"><p><strong>${report.blockerCount} Blocker</strong> – ein Scan würde daran scheitern:</p><ul>${report.blockers.map((item) =>
      `<li>${escapeMarkup(BLOCKER_LABEL[item.code] ?? item.code)}: <code>${escapeMarkup(item.path)}</code></li>`).join("")}</ul>${report.blockerCount > report.blockers.length ? `<p>… und ${report.blockerCount - report.blockers.length} weitere.</p>` : ""}</div>`;
  const overlaps = report.overlaps.length === 0 ? "" : `<div class="dry-run-overlaps"><p><strong>Überschneidung mit registrierten Bibliotheken:</strong></p><ul>${report.overlaps.map((item) =>
    `<li>Der Pfad ${OVERLAP_LABEL[item.relation]} <code>${escapeMarkup(item.key)}</code> (${escapeMarkup(item.path)}). Dateien würden doppelt katalogisiert.</li>`).join("")}</ul></div>`;
  const largest = report.largestFiles.length === 0 ? "" : `<details><summary>Größte Dateien</summary><ul>${report.largestFiles.map((item) =>
    `<li><code>${escapeMarkup(item.path)}</code> · ${formatBytes(item.bytes)}</li>`).join("")}</ul></details>`;
  return `${summary}${truncated}${blockers}${overlaps}${largest}`;
}

export function scanActive(scan: LatestScan | null | undefined): boolean {
  return scan?.status === "queued" || scan?.status === "running";
}

/** Progress line and cancel control of a library's current scan. */
export function scanProgressMarkup(libraryKey: string, scan: LatestScan | null | undefined): string {
  if (!scan) return "";
  if (scanActive(scan)) {
    const progress = scan.status === "queued" ? "wartet auf den Scan-Worker" : `${scan.discoveredFiles} Dateien gefunden · ${scan.hashedFiles} gehasht`;
    const retry = scan.attempt > 1 ? ` · Versuch ${scan.attempt}` : "";
    const cancel = scan.cancellationRequested
      ? '<span class="scan-cancelling">Abbruch angefordert …</span>'
      : `<button type="button" class="secondary-action" data-cancel-scan="${escapeMarkup(scan.id)}" data-library="${escapeMarkup(libraryKey)}">Scan abbrechen</button>`;
    return `<div class="scan-progress" aria-live="polite"><span>${scan.full ? "Vollscan" : "Scan"} ${progress}${retry}</span>${cancel}</div>`;
  }
  if (scan.status === "failed" && scan.errorMessage) {
    return `<div class="scan-progress scan-failed"><span>Letzter Scan fehlgeschlagen: ${escapeMarkup(scan.errorMessage)}</span></div>`;
  }
  return "";
}

function escapeMarkup(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#39;"); }
