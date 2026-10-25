// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { thumbnailPanelMarkup } from "./model-presentation";
import { mountThumbnailActions } from "./thumbnail-actions";
import type { LibraryRoot, ModelFile, ModelSummary } from "./types";

const model = {
  id: "model-1", name: "Trident", kind: "assembly", revision: 4,
  thumbnail: { kind: "default", candidateId: null, url: null, status: "default" },
} as ModelSummary;
const roots: LibraryRoot[] = [{ id: "root-1", key: "cad", name: "CAD", readOnly: false, fileCount: 1, missingFileCount: 0, latestScanStatus: null }];
const files = [{ rootKey: "cad", primary: true } as ModelFile];

function setup(overrides: Record<string, unknown> = {}) {
  const host = document.createElement("div");
  host.innerHTML = thumbnailPanelMarkup(model, [], true);
  const api = {
    roots: vi.fn().mockResolvedValue(roots),
    updateThumbnail: vi.fn().mockResolvedValue(model),
    previewImport: vi.fn().mockResolvedValue({ id: "draft-1", items: [{ id: "item-1", category: "image" }] }),
    configureImport: vi.fn().mockResolvedValue({}),
    uploadImportItem: vi.fn().mockResolvedValue({}),
    reviewImport: vi.fn().mockResolvedValue({ items: [{ action: "create" }] }),
    commitImport: vi.fn().mockResolvedValue({ modelId: "model-1" }),
    cancelImport: vi.fn().mockResolvedValue({}),
    ...overrides,
  };
  const reload = vi.fn();
  mountThumbnailActions(host, model, files, reload, api as never);
  return { host, api, reload };
}

async function pick(host: HTMLElement, file: File): Promise<void> {
  const input = host.querySelector<HTMLInputElement>("#thumbnail-upload")!;
  Object.defineProperty(input, "files", { value: [file], configurable: true });
  input.dispatchEvent(new Event("change"));
  await vi.waitFor(() => expect(input.disabled).toBe(false));
}

describe("thumbnail panel actions", () => {
  it("offers a dedicated image upload only to editors", () => {
    expect(thumbnailPanelMarkup(model, [], true)).toContain('id="thumbnail-upload" type="file"');
    expect(thumbnailPanelMarkup(model, [], false)).not.toContain("thumbnail-upload");
  });

  it("uploads a picked image into the model's library and reloads", async () => {
    const { host, api, reload } = setup();
    await pick(host, new File([new Uint8Array([1])], "cover.webp"));
    expect(api.configureImport).toHaveBeenCalledWith("draft-1", expect.objectContaining({ libraryRootId: "root-1", thumbnailItemId: "item-1" }));
    expect(api.commitImport).toHaveBeenCalledWith("draft-1");
    expect(reload).toHaveBeenCalledOnce();
    expect(host.querySelector("#thumbnail-upload-status")!.textContent).toBe("Bild hinzugefügt und als Vorschaubild gesetzt.");
  });

  it("explains failures without reloading", async () => {
    const { host, reload } = setup({ roots: vi.fn().mockResolvedValue([{ ...roots[0], readOnly: true }]) });
    await pick(host, new File([new Uint8Array([1])], "cover.png"));
    const status = host.querySelector("#thumbnail-upload-status")!;
    expect(status.classList.contains("error")).toBe(true);
    expect(status.textContent).toContain("keine beschreibbare Bibliothek");
    expect(reload).not.toHaveBeenCalled();
  });

  it("still saves a selection of an existing image", async () => {
    const { host, api, reload } = setup();
    host.querySelector<HTMLFormElement>("#thumbnail-form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() => expect(reload).toHaveBeenCalledOnce());
    expect(api.updateThumbnail).toHaveBeenCalledWith(model, "default", null);
  });
});
