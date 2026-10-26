// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { jobsApi, libraryApi } from "./api";
import { bindLibraryActions, libraryPanel, libraryRow } from "./libraries";
import { dryRunMarkup, scanProgressMarkup, type DryRunReport, type LatestScan } from "./library-onboarding";
import type { ManagedLibrary } from "./types";

const report: DryRunReport = {
  canonicalPath: "/srv/cad <new>", files: 4, bytes: 3 * 1024 ** 2, directories: 3, cadFiles: 2,
  formats: { step: 1, stl: 1 }, otherFiles: 2, symlinksSkipped: 1,
  blockers: [{ code: "name_not_utf8", path: "bad�.step" }], blockerCount: 3,
  largestFiles: [{ path: "parts/frame.step", bytes: 2048 }],
  overlaps: [{ key: "inner", path: "/srv/cad/inner", relation: "contains" }], truncated: true, elapsedMs: 12,
};
const running: LatestScan = {
  id: "scan-1", status: "running", full: false, discoveredFiles: 120, hashedFiles: 45, attempt: 2,
  cancellationRequested: false, errorMessage: null,
};
const library = {
  id: "lib-1", key: "cad", name: "CAD", filesystemPath: "/srv/cad", readOnly: true, enabled: true, fileCount: 3,
  missingFileCount: 0, latestScanStatus: "running", latestScanStartedAtUnixMs: 0, latestScan: running,
  updatedAtUnixMs: 0, revision: 1,
  storage: { state: "healthy", checkedAtUnixMs: 0, reachable: true, directory: true, readable: true, writable: true,
    totalBytes: null, availableBytes: null, usedPercent: null, reasons: [] },
} as unknown as ManagedLibrary;

describe("library onboarding dry run", () => {
  afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

  it("summarizes findings, blockers, overlaps, and truncation, escaped", () => {
    const markup = dryRunMarkup(report);
    expect(markup).toContain("/srv/cad &lt;new&gt;");
    expect(markup).toContain("4 Dateien · 3.0 MiB · 3 Ordner · 2 CAD-Dateien (STEP 1 · STL 1)");
    expect(markup).toContain("3 Blocker");
    expect(markup).toContain("Dateiname ist kein gültiges UTF-8");
    expect(markup).toContain("… und 2 weitere.");
    expect(markup).toContain("enthält die Bibliothek <code>inner</code>");
    expect(markup).toContain("Mindestwerte");
    expect(dryRunMarkup({ ...report, blockers: [], blockerCount: 0, overlaps: [], truncated: false }))
      .toContain("Keine Blocker");
  });

  it("runs the dry run from the registration form without registering", async () => {
    const dryRun = vi.spyOn(libraryApi, "dryRun").mockResolvedValue(report);
    const create = vi.spyOn(libraryApi, "create");
    const host = document.createElement("div");
    host.innerHTML = `<div id="admin-message"></div>${libraryPanel([])}`;
    document.body.append(host);
    host.querySelector<HTMLInputElement>("[name=path]")!.value = "/srv/cad";
    bindLibraryActions(host, vi.fn().mockResolvedValue(undefined));
    host.querySelector<HTMLButtonElement>("[data-dry-run-library]")!.click();
    await vi.waitFor(() => expect(host.querySelector("#library-validation")!.textContent).toContain("3 Blocker"));
    expect(dryRun).toHaveBeenCalledWith("/srv/cad");
    expect(create).not.toHaveBeenCalled();
  });
});

describe("scan progress and cancellation", () => {
  afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

  it("shows progress, retries, cancellation, and failures", () => {
    expect(scanProgressMarkup("cad", running)).toContain("Scan 120 Dateien gefunden · 45 gehasht · Versuch 2");
    expect(scanProgressMarkup("cad", running)).toContain('data-cancel-scan="scan-1"');
    expect(scanProgressMarkup("cad", { ...running, status: "queued" })).toContain("wartet auf den Scan-Worker");
    expect(scanProgressMarkup("cad", { ...running, cancellationRequested: true })).toContain("Abbruch angefordert");
    expect(scanProgressMarkup("cad", { ...running, cancellationRequested: true })).not.toContain("data-cancel-scan");
    expect(scanProgressMarkup("cad", { ...running, status: "failed", errorMessage: "volume <gone>" })).toContain("volume &lt;gone&gt;");
    expect(scanProgressMarkup("cad", { ...running, status: "completed" })).toBe("");
    expect(libraryRow(library)).toContain("Scan läuft …");
  });

  it("cancels a running scan only after exact confirmation", async () => {
    const cancel = vi.spyOn(jobsApi, "cancelRun").mockResolvedValue({} as never);
    vi.spyOn(libraryApi, "list").mockResolvedValue([]);
    const host = document.createElement("div");
    host.innerHTML = `<div id="admin-message"></div>${libraryPanel([library])}`;
    document.body.append(host);
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindLibraryActions(host, refresh);
    host.querySelector<HTMLButtonElement>("[data-cancel-scan]")!.click();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    expect(cancel).not.toHaveBeenCalled();
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = "CANCEL scan-1";
    input.dispatchEvent(new Event("input"));
    document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("scan", "scan-1", "CANCEL scan-1"));
    await vi.waitFor(() => expect(host.textContent).toContain("Abbruch für Scan scan-1 angefordert."));
    expect(refresh).toHaveBeenCalled();
  });
});
