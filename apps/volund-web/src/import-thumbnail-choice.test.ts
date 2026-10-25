// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import {
  UPLOADED_THUMBNAIL_VALUE, draftThumbnailItemId, importThumbnailPickerMarkup, mountImportThumbnailChoice,
} from "./import-thumbnail-choice";
import type { ImportCommit } from "./types";

const commit = { modelId: "model-9", draftId: "draft-main" } as ImportCommit;

function setup(overrides: Record<string, unknown> = {}) {
  const host = document.createElement("div");
  host.innerHTML = `<select id="metadata-library"><option value="root-1" selected>CAD</option></select>
    <select id="metadata-thumbnail"><option value="">Automatisch</option><option value="item-7">cover.png</option></select>
    ${importThumbnailPickerMarkup()}<p id="review-confirm-note"></p>`;
  const api = {
    model: vi.fn().mockResolvedValue({ id: "model-9", name: "Neu", kind: "part", revision: 1 }),
    previewImport: vi.fn().mockResolvedValue({ id: "draft-2", items: [{ id: "item-1", category: "image" }] }),
    configureImport: vi.fn().mockResolvedValue({}),
    uploadImportItem: vi.fn().mockResolvedValue({}),
    reviewImport: vi.fn().mockResolvedValue({ items: [{ action: "create" }] }),
    commitImport: vi.fn().mockResolvedValue({ modelId: "model-9" }),
    cancelImport: vi.fn().mockResolvedValue({}),
    ...overrides,
  };
  const choice = mountImportThumbnailChoice(host, api as never);
  const select = host.querySelector<HTMLSelectElement>("#metadata-thumbnail")!;
  const pick = (file: File): void => {
    const input = host.querySelector<HTMLInputElement>("#metadata-thumbnail-upload")!;
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    input.dispatchEvent(new Event("change"));
  };
  return { host, api, choice, select, pick };
}

describe("import thumbnail choice", () => {
  it("never sends the upload placeholder as draft item", () => {
    expect(draftThumbnailItemId(UPLOADED_THUMBNAIL_VALUE)).toBeNull();
    expect(draftThumbnailItemId("")).toBeNull();
    expect(draftThumbnailItemId("item-7")).toBe("item-7");
  });

  it("adds a picked image as selected thumbnail option", () => {
    const { select, pick } = setup();
    pick(new File([new Uint8Array([1])], "cover.jpg"));
    expect(select.value).toBe(UPLOADED_THUMBNAIL_VALUE);
    expect(select.selectedOptions[0]!.textContent).toBe("Eigenes Bild · cover.jpg");
  });

  it("rejects non-raster files without changing the selection", () => {
    const { host, select, pick } = setup();
    pick(new File(["<svg/>"], "logo.svg"));
    expect(select.value).toBe("");
    expect(host.querySelector("#metadata-thumbnail-upload-note")!.classList.contains("error")).toBe(true);
  });

  it("uploads the picked image to the committed model", async () => {
    const { host, api, choice, pick } = setup();
    pick(new File([new Uint8Array([1])], "cover.png"));
    await choice.applyAfterCommit(commit);
    expect(api.model).toHaveBeenCalledWith("model-9");
    expect(api.configureImport).toHaveBeenCalledWith("draft-2", expect.objectContaining({
      targetAction: "extend", targetModelId: "model-9", libraryRootId: "root-1", thumbnailItemId: "item-1",
    }));
    expect(host.querySelector("#review-confirm-note")!.textContent).toContain("als Vorschaubild gesetzt");
  });

  it("does nothing when another thumbnail was chosen or the draft was replaced", async () => {
    const { api, choice, select, pick } = setup();
    pick(new File([new Uint8Array([1])], "cover.png"));
    select.value = "item-7";
    await choice.applyAfterCommit(commit);
    pick(new File([new Uint8Array([1])], "cover.png"));
    choice.reset();
    expect(select.querySelector(`option[value="${UPLOADED_THUMBNAIL_VALUE}"]`)).toBeNull();
    await choice.applyAfterCommit(commit);
    expect(api.model).not.toHaveBeenCalled();
  });

  it("reports a failed upload while keeping the committed model", async () => {
    const { host, choice, pick } = setup({ uploadImportItem: vi.fn().mockRejectedValue(new Error("network down")) });
    pick(new File([new Uint8Array([1])], "cover.png"));
    await choice.applyAfterCommit(commit);
    const note = host.querySelector("#review-confirm-note")!;
    expect(note.textContent).toContain("Modell gespeichert, aber das Vorschaubild fehlt");
    expect(note.classList.contains("error")).toBe(true);
  });
});
