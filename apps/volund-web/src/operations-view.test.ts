// @vitest-environment happy-dom

import { describe, expect, it, vi } from "vitest";
import { operationsApi } from "./api";
import { bindOperationsActions, operationsPanel } from "./operations-view";
import type { OperationsHealth } from "./types";

function health(): OperationsHealth {
  return {
    state: "degraded", checkedAtUnixMs: Date.UTC(2026, 7, 29, 12), version: "0.32.0",
    database: { state: "healthy", serverVersion: 170011, schemaTables: 27, expectedSchemaTables: 27, appliedMigrations: 16, expectedMigrations: 16 },
    workers: [{ key: "preview-worker", state: "degraded", lastOutcome: "success", lastSucceededAtUnixMs: Date.UTC(2026, 7, 29, 11), lastFailedAtUnixMs: null, expectedIntervalSeconds: 10, reasons: ["component_heartbeat_stale"] }],
    backup: { key: "backup", state: "healthy", lastOutcome: "success", lastSucceededAtUnixMs: Date.UTC(2026, 7, 29), lastFailedAtUnixMs: null, expectedIntervalSeconds: 86400, reasons: [] },
    libraries: [],
    recentScans: [{ id: "scan", libraryKey: "cad<&", status: "failed", full: true, requestedAtUnixMs: Date.UTC(2026, 7, 29, 10), finishedAtUnixMs: Date.UTC(2026, 7, 29, 10, 1), hasError: true }],
  };
}

describe("operations panel", () => {
  it("renders aggregate, database, heartbeat and scan state without raw errors", () => {
    const markup = operationsPanel(health(), "de-DE", "Europe/Berlin");
    expect(markup).toContain("EINGESCHRÄNKT");
    expect(markup).toContain("Migrationen 16/16");
    expect(markup).toContain("Statussignal verspätet");
    expect(markup).toContain("cad&lt;&amp;");
    expect(markup).toContain("FEHLER");
    expect(markup).toContain("Geschwärztes Supportpaket herunterladen");
  });

  it("labels a missing successful heartbeat explicitly", () => {
    const value = health();
    value.backup = { ...value.backup, state: "blocked", lastOutcome: null, lastSucceededAtUnixMs: null, reasons: ["component_never_succeeded"] };
    const markup = operationsPanel(value, "de-DE", "Europe/Berlin");
    expect(markup).toContain("noch nie erfolgreich");
    expect(markup).toContain("kein erfolgreicher Lauf erfasst");
  });

  it("shows bounded generation progress and downloads the returned archive", async () => {
    const host = document.createElement("div");
    host.innerHTML = operationsPanel(health(), "de-DE", "Europe/Berlin");
    let complete!: (value: { blob: Blob; filename: string; sha256: string }) => void;
    vi.spyOn(operationsApi, "supportBundle").mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:support");
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    bindOperationsActions(host);
    const button = host.querySelector<HTMLButtonElement>("#download-support-bundle")!;
    button.click();
    expect(button.disabled).toBe(true);
    expect(button.textContent).toContain("sicher erstellt");
    complete({ blob: new Blob(["tar"]), filename: "volund-support.tar", sha256: "a".repeat(64) });
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(URL.createObjectURL).toHaveBeenCalled();
    vi.restoreAllMocks();
  });
});
