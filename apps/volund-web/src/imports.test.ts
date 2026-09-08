// @vitest-environment happy-dom

import { describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi } from "./api";
import { renderImportReview } from "./import-review-renderer";
import { extensionTarget, importSourceName, importStageLabel, mountImports, reviewActionLabel, reviewTotalFiles, selectedManifest, uploadPercentage } from "./imports";

describe("browser import manifests", () => {
  it("prefers relative folder paths without reading file bytes", () => {
    expect(selectedManifest([
      { name: "main.step", size: 500, webkitRelativePath: "Voron/CAD/main.step" },
      { name: "manual.pdf", size: 100, webkitRelativePath: "Voron/docs/manual.pdf" },
    ])).toEqual([
      { path: "Voron/CAD/main.step", byteSize: 500 },
      { path: "Voron/docs/manual.pdf", byteSize: 100 },
    ]);
  });

  it("derives a stable source name from folders and loose files", () => {
    expect(importSourceName([{ path: "Voron/main.step", byteSize: 1 }, { path: "Voron/a.stl", byteSize: 1 }])).toBe("Voron");
    expect(importSourceName([{ path: "project.zip", byteSize: 1 }])).toBe("project.zip");
    expect(importSourceName([{ path: "a.step", byteSize: 1 }, { path: "b.pdf", byteSize: 1 }])).toBe("2 Projektdateien");
  });

  it("keeps staged upload progress bounded", () => {
    expect(uploadPercentage(78, 156)).toBe(50);
    expect(uploadPercentage(2, 0)).toBe(0);
    expect(uploadPercentage(200, 156)).toBe(100);
  });

  it("uses unambiguous German review actions", () => {
    expect(reviewActionLabel("create")).toBe("NEU");
    expect(reviewActionLabel("relocate")).toBe("VERSCHIEBEN");
    expect(reviewActionLabel("conflict")).toBe("KONFLIKT");
  });

  it("shows the complete reviewed file count instead of only new files", () => {
    expect(reviewTotalFiles({
      createFiles: 155, relocateFiles: 1, reuseFiles: 0, conflicts: 0, skipFiles: 0,
    } as Parameters<typeof reviewTotalFiles>[0])).toBe(156);
  });

  it("uses short, understandable labels for every visible import stage", () => {
    expect(importStageLabel("analyzing")).toBe("Wird analysiert …");
    expect(importStageLabel("uploading")).toBe("Wird hochgeladen …");
    expect(importStageLabel("reviewing")).toBe("Wird geprüft …");
    expect(importStageLabel("committing")).toBe("Wird übernommen …");
    expect(importStageLabel("complete")).toBe("Abgeschlossen");
  });

  it("pins model extension to the current optimistic revision", () => {
    expect(extensionTarget({ id: "model-1", revision: 9 } as Parameters<typeof extensionTarget>[0]))
      .toEqual({ action: "extend", modelId: "model-1", revision: 9 });
  });

  it("shows the upload library before files are selected", async () => {
    vi.spyOn(catalogApi, "collections").mockResolvedValue([]);
    vi.spyOn(catalogApi, "roots").mockResolvedValue([]);
    vi.spyOn(catalogApi, "models").mockResolvedValue([{ id: "model-1", name: "Testmodell", revision: 1 }] as Awaited<ReturnType<typeof catalogApi.models>>);
    vi.spyOn(catalogApi, "authors").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
    vi.spyOn(catalogApi, "tags").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
    vi.spyOn(identityApi, "settings").mockResolvedValue([]);
    const host = document.createElement("main");
    mountImports(host, () => undefined);
    await Promise.resolve();
    expect(host.textContent).toContain("UPLOAD-ZIEL");
    expect(host.querySelectorAll("#metadata-library")).toHaveLength(1);
    expect(host.textContent).toContain("ohne automatisch übernommenen Zwischenordner");
    expect(host.querySelector("#import-metadata")?.classList.contains("import-metadata-full")).toBe(true);
    expect(host.querySelector("#metadata-name")?.closest("label")?.classList.contains("import-field-name")).toBe(true);
    expect(host.querySelector("#metadata-author")?.closest("label")?.classList.contains("import-field-author")).toBe(true);
    expect(host.querySelector("#metadata-tags")?.closest("label")?.classList.contains("import-field-tags")).toBe(true);
    expect(host.querySelector("#metadata-description")?.closest("label")?.classList.contains("import-field-description")).toBe(true);
    expect(host.querySelector(".import-target-field")).not.toBeNull();
    expect(host.querySelector(".import-source-field")).not.toBeNull();
    expect(host.querySelector<HTMLElement>("#import-existing-model-label")?.hidden).toBe(true);
    expect(host.querySelector<HTMLSelectElement>("#import-existing-model")?.disabled).toBe(true);
    expect(host.querySelector("#metadata-tag-suggestions[role='listbox']")).not.toBeNull();
    expect(host.querySelector("#import-selection-dialog")).not.toBeNull();
    expect(host.querySelector("#import-metadata")?.nextElementSibling?.classList.contains("file-details-full")).toBe(true);
    expect(host.textContent).toContain("Ohne Auswahl bleibt das Modellbild neutral");
    expect(host.textContent).not.toContain("deterministisches Rasterbild");
  });

  it("resolves a file conflict through the in-page target dialog", async () => {
    vi.spyOn(catalogApi, "collections").mockResolvedValue([]);
    vi.spyOn(catalogApi, "roots").mockResolvedValue([]);
    vi.spyOn(catalogApi, "models").mockResolvedValue([{ id: "model-1", name: "Frame", revision: 1 }] as Awaited<ReturnType<typeof catalogApi.models>>);
    vi.spyOn(catalogApi, "authors").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
    vi.spyOn(catalogApi, "tags").mockResolvedValue({ items: [], total: 0, limit: 100, offset: 0 });
    vi.spyOn(identityApi, "settings").mockResolvedValue([]);
    const resolved = vi.spyOn(catalogApi, "resolveImportItem").mockResolvedValue(undefined);
    vi.spyOn(catalogApi, "reviewImport").mockResolvedValue({
      draftId: "draft-1", modelName: "Frame", modelAction: "update", existingModelId: "model-1",
      rootKey: "cad", rootName: "CAD", baseDirectory: "Projekte/frame", createFiles: 1,
      reuseFiles: 0, relocateFiles: 0, conflicts: 0, skipFiles: 0, newBytes: 10, savedBytes: 0,
      items: [],
    });
    const host = document.createElement("main");
    mountImports(host, () => undefined);
    await Promise.resolve();
    renderImportReview(host, {
      draftId: "draft-1", modelName: "Frame", modelAction: "update", existingModelId: "model-1",
      rootKey: "cad", rootName: "CAD", baseDirectory: "Projekte/frame", createFiles: 0,
      reuseFiles: 0, relocateFiles: 0, conflicts: 1, skipFiles: 0, newBytes: 0, savedBytes: 0,
      items: [{ id: "item-1", originalPath: "main.step", category: "cad", byteSize: 10,
        sha256: "a".repeat(64), action: "conflict", targetPath: "Projekte/frame/CAD/main.step",
        existingFileId: null, existingPath: null, isPrimary: true }],
    });
    host.querySelector<HTMLButtonElement>("[data-resolution='create']")!.click();
    const dialog = host.querySelector<HTMLDialogElement>("#import-conflict-dialog")!;
    expect(dialog.open).toBe(true);
    const input = dialog.querySelector<HTMLInputElement>("input")!;
    expect(input.value).toBe("Projekte/frame/CAD/main.step");
    input.value = "Projekte/frame/CAD/main-v2.step";
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(resolved).toHaveBeenCalledWith(
      "draft-1", "item-1", "create", "Projekte/frame/CAD/main-v2.step",
    ));
    expect(host.textContent).toContain("KONFLIKTFREI");
  });
});
