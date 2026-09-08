// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { jobsApi } from "./api";
import { bindJobActions, jobPanel, jobRow } from "./jobs-view";
import type { ManagedJob } from "./types";

function job(overrides: Partial<ManagedJob> = {}): ManagedJob {
  return {
    id: "11111111-1111-1111-1111-111111111111", kind: "scan", status: "failed",
    title: "CAD <root>", context: "cad", profile: null, attempt: 2, retryOfId: null,
    cancellationRequestedAtUnixMs: null, requestedAtUnixMs: Date.UTC(2026, 7, 29),
    startedAtUnixMs: Date.UTC(2026, 7, 29), finishedAtUnixMs: Date.UTC(2026, 7, 29, 0, 1),
    progressCurrent: 4, progressTotal: 10, diagnostic: "Scan fehlgeschlagen", canRetry: true, canCancel: false,
    ...overrides,
  };
}

afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

describe("central job administration", () => {
  it("labels unavailable conversion progress without a fabricated ratio", () => {
    const markup = jobRow(job({kind:"conversion",status:"running",progressCurrent:0,progressTotal:null}), "de-DE", "Europe/Berlin");
    expect(markup).toContain("Fortschritt nicht verfügbar");
    expect(markup).not.toContain("Fortschritt 0");
  });
  it("renders context, progress, diagnostics, state and legal actions", () => {
    const markup = jobRow(job(), "de-DE", "Europe/Berlin");
    expect(markup).toContain("CAD &lt;root&gt;");
    expect(markup).toContain("Neu gehashte Dateien: 4 · Entdeckte Dateien: 10");
    expect(markup).toContain("FEHLGESCHLAGEN");
    expect(markup).toContain("Wiederholen");
    expect(markup).not.toContain(">Abbrechen<");
  });

  it("does not present unchanged completed scans as incomplete", () => {
    const markup = jobRow(job({status:"completed",progressCurrent:0,progressTotal:1}), "de-DE", "Europe/Berlin");
    expect(markup).toContain("Neu gehashte Dateien: 0 · Entdeckte Dateien: 1");
    expect(markup).toContain("ABGESCHLOSSEN");
    expect(markup).not.toContain("Fortschritt 0/1");
    expect(jobRow(job({kind:"conversion"}), "de-DE", "Europe/Berlin")).toContain("Fortschritt 4/10");
    expect(jobRow(job({progressTotal:null}), "de-DE", "Europe/Berlin")).not.toContain("Entdeckte Dateien:");
  });

  it("submits filters and exact action confirmation", async () => {
    const value = job({ status: "running", canRetry: false, canCancel: true });
    const host = document.createElement("main");
    host.innerHTML = jobPanel({ items: [value], limit: 50, offset: 0, total: 1 }, {}, "de-DE", "Europe/Berlin");
    document.body.append(host);
    const setFilters = vi.fn();
    const refresh = vi.fn().mockResolvedValue(undefined);
    const cancel = vi.spyOn(jobsApi, "cancel").mockResolvedValue({ ...value, status: "cancelled" });
    bindJobActions(host, setFilters, refresh);
    host.querySelector<HTMLButtonElement>("[data-job-action=cancel]")!.click();
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = `CANCEL ${value.id}`; input.dispatchEvent(new Event("input"));
    document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith(value, `CANCEL ${value.id}`));
    await vi.waitFor(() => expect(refresh).toHaveBeenCalled());

    const form = host.querySelector<HTMLFormElement>("#job-filters")!;
    form.querySelector<HTMLSelectElement>("[name=kind]")!.value = "scan";
    form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    expect(setFilters).toHaveBeenCalledWith({ kind: "scan", offset: 0 });
  });

  it("does not retry a job after cancelling its confirmation", async () => {
    const host = document.createElement("main");
    host.innerHTML = jobPanel({ items: [job()], limit: 50, offset: 0, total: 1 }, {}, "de-DE", "Europe/Berlin");
    document.body.append(host);
    const retry = vi.spyOn(jobsApi, "retry");
    bindJobActions(host, vi.fn(), async () => undefined);
    const button = host.querySelector<HTMLButtonElement>("[data-job-action=retry]")!;
    button.click();
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(retry).not.toHaveBeenCalled();
    expect(host.textContent).toContain("Aktion abgebrochen");
  });

  it("keeps long job histories collapsed until requested", () => {
    const items = Array.from({ length: 10 }, (_, index) => job({ id: `${index}`.padStart(36, "0"), title: `Auftrag ${index + 1}` }));
    const host = document.createElement("main");
    host.innerHTML = jobPanel({ items, limit: 50, offset: 0, total: 10 }, {}, "de-DE", "Europe/Berlin");
    const overflow = host.querySelector<HTMLDetailsElement>(".admin-job-overflow")!;
    expect(host.querySelectorAll("#job-panel > .admin-list > article")).toHaveLength(8);
    expect(overflow.open).toBe(false);
    expect(overflow.querySelector("summary")?.textContent).toContain("Weitere 2 Aufträge");
    expect(overflow.querySelectorAll("article")).toHaveLength(2);
  });

  it("renders bounded job pages and requests adjacent offsets", () => {
    const host = document.createElement("main");
    host.innerHTML = jobPanel({ items: [job()], limit: 50, offset: 50, total: 101 }, { kind: "scan" }, "de-DE", "Europe/Berlin");
    document.body.append(host);
    const setFilters = vi.fn();
    bindJobActions(host, setFilters, vi.fn().mockResolvedValue(undefined));
    expect(host.querySelector("[data-job-page-info]")?.textContent).toBe("51–100 von 101");
    host.querySelector<HTMLButtonElement>("[data-job-page=previous]")!.click();
    expect(setFilters).toHaveBeenLastCalledWith({ kind: "scan", offset: 0 });
    host.querySelector<HTMLButtonElement>("[data-job-page=next]")!.click();
    expect(setFilters).toHaveBeenLastCalledWith({ kind: "scan", offset: 100 });
  });
});
