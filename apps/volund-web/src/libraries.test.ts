// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, libraryApi } from "./api";
import { bindLibraryActions, libraryPanel, libraryRow } from "./libraries";
import type { ManagedLibrary } from "./types";

function library(overrides: Partial<ManagedLibrary> = {}): ManagedLibrary {
  return {
    id: "library-1", revision: 3,
    key: "cad_main",
    name: "CAD & Main",
    filesystemPath: "/srv/cad/<main>",
    readOnly: true,
    enabled: true,
    fileCount: 42,
    missingFileCount: 2,
    latestScanStatus: "completed",
    latestScanStartedAtUnixMs: null,
    updatedAtUnixMs: 0,
    storage: {
      state: "healthy",
      checkedAtUnixMs: 1_700_000_000_000,
      reachable: true,
      directory: true,
      readable: true,
      writable: true,
      totalBytes: 100 * 1024 ** 3,
      availableBytes: 50 * 1024 ** 3,
      usedPercent: 50,
      filesystemType: "ext4",
      reasons: [],
    },
    ...overrides,
  };
}

describe("library administration", () => {
  afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });
  it.each(["rename", "toggle"])("captures the %s revision before confirmation and reports conflicts", async (action) => {
    const host = document.createElement("div");
    host.innerHTML = `<div id="admin-message"></div>${libraryPanel([library()])}`;
    document.body.append(host);
    const update = vi.spyOn(libraryApi, "update").mockRejectedValue(new ApiError(409, "library changed; reload before retrying"));
    const refresh = vi.fn().mockResolvedValue(undefined);
    bindLibraryActions(host, refresh);
    const control = host.querySelector<HTMLElement>(action === "rename" ? "[data-library-edit]" : "[data-library-toggle]")!;
    if (action === "rename") control.dispatchEvent(new Event("submit", { cancelable: true }));
    else control.click();
    control.dataset.revision = "9";
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = "UPDATE LIBRARY cad_main"; input.dispatchEvent(new Event("input"));
    document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
    await vi.waitFor(() => expect(update).toHaveBeenCalledWith("cad_main", expect.objectContaining({ expectedRevision: 3 })));
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("zwischenzeitlich geändert. Bitte neu laden"));
    expect(refresh).not.toHaveBeenCalled();
  });
  it("renders a path validation/create form and a useful empty state", () => {
    const markup = libraryPanel([]);
    expect(markup).toContain("Pfad prüfen");
    expect(markup).toContain("Bibliothek anlegen");
    expect(markup).toContain("Noch keine Bibliothek registriert");
    expect(markup).toContain('aria-live="polite"');
  });

  it("renders escaped identity, counts, edit and scan controls", () => {
    const markup = libraryRow(library());
    expect(markup).toContain("CAD &amp; Main");
    expect(markup).toContain("/srv/cad/&lt;main&gt;");
    expect(markup).toContain("42 Dateien · 2 fehlend");
    expect(markup).toContain("Bibliothek umbenennen");
    expect(markup).toContain("Umbenennen");
    expect(markup).toContain("Vollscan");
    expect(markup).toContain("50.0 GiB von 100.0 GiB frei");
    expect(markup).toContain("GESUND");
  });

  it("explains blocked storage and disables scans", () => {
    const base = library();
    const markup = libraryRow(library({
      storage: { ...base.storage, state: "blocked", readable: false, reasons: [{ code: "storage_unreadable", state: "blocked" }] },
    }));
    expect(markup).toContain("BLOCKIERT");
    expect(markup).toContain("nicht lesbar");
    expect(markup).toContain('data-full="true" disabled');
  });

  it("disables scan buttons while a library is disabled", () => {
    const markup = libraryRow(library({ enabled: false }));
    expect(markup).toContain("DEAKTIVIERT");
    expect(markup).toContain('data-full="false" disabled');
    expect(markup).toContain("Aktivieren");
  });

  it("disables duplicate scan requests while durable work is active", () => {
    const markup = libraryRow(library({ latestScanStatus: "queued" }));
    expect(markup).toContain("Scan läuft …");
    expect(markup).toContain('data-full="false" disabled');
    expect(markup).toContain('data-full="true" disabled');
  });

  it("requires the exact full-scan target confirmation before submission", async () => {
    const host = document.createElement("div");
    host.innerHTML = `<div id="admin-message"></div>${libraryPanel([library()])}`;
    document.body.append(host);
    const scan = vi.spyOn(libraryApi, "scan").mockResolvedValue({ id: "scan", status: "queued", full: true });
    vi.spyOn(libraryApi, "list").mockResolvedValue([]);
    bindLibraryActions(host, async () => undefined);
    const full = host.querySelector<HTMLButtonElement>("[data-library-scan][data-full=true]")!;
    full.click();
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = "wrong"; input.dispatchEvent(new Event("input"));
    expect(document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("abgebrochen"));
    expect(scan).not.toHaveBeenCalled();
    expect(full.disabled).toBe(false);
    expect(full.textContent).toBe("Vollscan");
    full.click();
    const retryInput = document.querySelector<HTMLInputElement>("dialog input")!;
    retryInput.value = "FULL SCAN cad_main"; retryInput.dispatchEvent(new Event("input"));
    document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
    await vi.waitFor(() => expect(scan).toHaveBeenCalledWith("cad_main", true, "FULL SCAN cad_main"));
    expect(host.querySelector("#admin-message")?.textContent).toContain("dauerhaft eingereiht");
  });

  it("restores the scan button after a failed API request", async () => {
    const host = document.createElement("div");
    host.innerHTML = `<div id="admin-message"></div>${libraryPanel([library()])}`;
    document.body.append(host);
    vi.spyOn(libraryApi, "scan").mockRejectedValue(new Error("Scan konnte nicht starten"));
    bindLibraryActions(host, async () => undefined);
    const button = host.querySelector<HTMLButtonElement>("[data-library-scan][data-full=false]")!;
    button.click();
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("konnte nicht starten"));
    expect(button.disabled).toBe(false);
    expect(button.textContent).toBe("Scan");
  });
});
