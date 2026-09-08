// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi } from "./api";
import { importStatusLabel, loadDraftInventory } from "./import-drafts";

afterEach(() => {
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

function draft() {
  return {
    id: "draft-1", displayName: "Mixed upload", status: "uploading" as const,
    actorId: "actor-1", actorName: "Audit", totalFiles: 2, uploadedFiles: 1,
    totalBytes: 8, uploadedBytes: 4, createdAtUnixMs: 1, updatedAtUnixMs: 2,
    expiresAtUnixMs: 3, targetAction: "create" as const, targetModelId: null,
    lastErrorCode: null, resultModelId: null, canRetry: true, canCancel: true,
  };
}

function mockInventory() {
  vi.spyOn(catalogApi, "importDrafts").mockResolvedValue({ items: [draft()], total: 1, limit: 50, offset: 0 });
  vi.spyOn(catalogApi, "importStorage").mockResolvedValue({ reservedBytes: 8, uploadedBytes: 4, reclaimableBytes: 4, capacityBytes: 1024 });
}

describe("import draft lifecycle",()=>{
  it("labels resumable and terminal states without relying on color",()=>{
    expect(importStatusLabel("uploading")).toBe("Upload unterbrochen");
    expect(importStatusLabel("failed")).toBe("Fehlgeschlagen");
    expect(importStatusLabel("expired")).toBe("Abgelaufen");
    expect(importStatusLabel("committed")).toBe("Abgeschlossen");
  });

  it("cancels a draft through exact in-page confirmation", async () => {
    mockInventory();
    const cancel = vi.spyOn(catalogApi, "cancelImport").mockResolvedValue({ id: "draft-1", status: "cancelled", stagingCleaned: true });
    const section = document.createElement("section");
    document.body.append(section);
    await loadDraftInventory(section, vi.fn(), vi.fn());
    section.querySelector<HTMLButtonElement>("[data-action='cancel']")!.click();
    const dialog = document.querySelector<HTMLDialogElement>("[data-exact-confirmation]")!;
    expect(dialog.open).toBe(true);
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    input.value = "CANCEL IMPORT draft-1";
    input.dispatchEvent(new Event("input"));
    dialog.querySelector<HTMLButtonElement>("[type='submit']")!.click();
    await vi.waitFor(() => expect(cancel).toHaveBeenCalledWith("draft-1"));
  });

  it("renames a draft through an in-page text dialog", async () => {
    mockInventory();
    const rename = vi.spyOn(catalogApi, "renameImport").mockResolvedValue({ ...draft(), displayName: "Continued import" });
    const section = document.createElement("section");
    document.body.append(section);
    await loadDraftInventory(section, vi.fn(), vi.fn());
    section.querySelector<HTMLButtonElement>("[data-action='rename']")!.click();
    const dialog = document.querySelector<HTMLDialogElement>("[data-text-input-dialog]")!;
    expect(dialog.open).toBe(true);
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    expect(input.value).toBe("Mixed upload");
    input.value = "Continued import";
    dialog.querySelector<HTMLFormElement>("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() => expect(rename).toHaveBeenCalledWith("draft-1", "Continued import"));
  });

  it("restores focus after cancelling a draft dialog", async () => {
    mockInventory();
    const section = document.createElement("section");
    document.body.append(section);
    await loadDraftInventory(section, vi.fn(), vi.fn());
    const trigger = section.querySelector<HTMLButtonElement>("[data-action='rename']")!;
    trigger.focus();
    trigger.click();
    document.querySelector<HTMLDialogElement>("[data-text-input-dialog]")!
      .dispatchEvent(new Event("cancel", { cancelable: true }));
    await vi.waitFor(() => expect(document.activeElement).toBe(trigger));
  });
});
