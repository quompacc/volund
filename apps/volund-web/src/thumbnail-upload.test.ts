import { describe, expect, it, vi } from "vitest";
import {
  isThumbnailImage, thumbnailLibraryRootId, thumbnailReviewProblem, thumbnailUploadStageLabel, uploadModelThumbnail,
  type ThumbnailUploadApi,
} from "./thumbnail-upload";
import type { ImportDraft, ImportReview, ImportReviewItem, LibraryRoot, ModelFile } from "./types";

const model = { id: "model-1", name: "Trident", kind: "assembly" as const, revision: 7 };
const image = new File([new Uint8Array([1, 2, 3])], "cover.png", { type: "image/png" });

const root = (id: string, key: string, readOnly = false): LibraryRoot =>
  ({ id, key, name: key, readOnly, fileCount: 0, missingFileCount: 0, latestScanStatus: null });
const file = (rootKey: string, primary = false): ModelFile => ({ rootKey, primary } as ModelFile);
const reviewItem = (action: ImportReviewItem["action"]): ImportReviewItem => ({
  id: "item-1", originalPath: "cover.png", category: "image", byteSize: 3, sha256: "abc", action,
  targetPath: "Baugruppen/trident/Bilder/cover.png", existingFileId: null, existingPath: action === "relocate" ? "Alt/cover.png" : null, isPrimary: false,
});
const review = (action: ImportReviewItem["action"]): ImportReview => ({ items: [reviewItem(action)] } as ImportReview);
const draft = (category: ImportDraft["items"][number]["category"] = "image"): ImportDraft => ({
  id: "draft-1", sourceName: "cover.png", suggestedModelName: "cover", suggestedSlug: "cover", totalFiles: 1, totalBytes: 3,
  items: [{ id: "item-1", originalPath: "cover.png", byteSize: 3, category, suggestedRelativePath: "Bilder/cover.png", isPrimaryCandidate: false, uploadStatus: "pending" }],
});

function fakeApi(overrides: Partial<Record<keyof ThumbnailUploadApi, unknown>> = {}) {
  const api = {
    previewImport: vi.fn().mockResolvedValue(draft()),
    configureImport: vi.fn().mockResolvedValue({}),
    uploadImportItem: vi.fn().mockResolvedValue({}),
    reviewImport: vi.fn().mockResolvedValue(review("create")),
    commitImport: vi.fn().mockResolvedValue({ draftId: "draft-1", modelId: "model-1", status: "committed" }),
    cancelImport: vi.fn().mockResolvedValue({ id: "draft-1", status: "cancelled", stagingCleaned: true }),
    ...overrides,
  };
  return api as typeof api & ThumbnailUploadApi;
}

describe("thumbnail upload helpers", () => {
  it("accepts only raster images that can become thumbnails", () => {
    expect(isThumbnailImage({ name: "cover.PNG" })).toBe(true);
    expect(isThumbnailImage({ name: "photo.jpeg" })).toBe(true);
    expect(isThumbnailImage({ name: "drawing.svg" })).toBe(false);
    expect(isThumbnailImage({ name: "webp" })).toBe(false);
  });

  it("labels every upload stage", () => {
    expect(thumbnailUploadStageLabel("uploading")).toBe("Bild wird hochgeladen …");
    expect(thumbnailUploadStageLabel("committing")).toBe("Vorschaubild wird übernommen …");
  });

  it("prefers the writable library of the primary file", () => {
    const roots = [root("r-a", "archive", true), root("r-b", "cad"), root("r-c", "prints")];
    expect(thumbnailLibraryRootId([file("prints"), file("cad", true)], roots)).toBe("r-b");
    expect(thumbnailLibraryRootId([file("archive", true), file("prints")], roots)).toBe("r-c");
    expect(thumbnailLibraryRootId([], roots)).toBe("r-b");
    expect(thumbnailLibraryRootId([], [root("r-a", "archive", true)])).toBeNull();
  });

  it("only allows plain new or reused files", () => {
    expect(thumbnailReviewProblem(review("create"))).toBeNull();
    expect(thumbnailReviewProblem(review("reuse"))).toBeNull();
    expect(thumbnailReviewProblem(review("relocate"))).toContain("Alt/cover.png");
    expect(thumbnailReviewProblem(review("conflict"))).toContain("Benenne das Bild um");
    expect(thumbnailReviewProblem(review("skip"))).toContain("übersprungen");
    expect(thumbnailReviewProblem({ items: [] } as unknown as ImportReview)).toContain("nicht in den Importplan");
  });
});

describe("uploadModelThumbnail", () => {
  it("extends the model with the image and selects it as thumbnail in one commit", async () => {
    const api = fakeApi();
    const stages: string[] = [];
    await uploadModelThumbnail(model, image, "root-1", (stage) => stages.push(stage), api);
    expect(api.previewImport).toHaveBeenCalledWith("cover.png", [{ path: "cover.png", byteSize: 3 }]);
    expect(api.configureImport).toHaveBeenCalledWith("draft-1", expect.objectContaining({
      targetAction: "extend", targetModelId: "model-1", expectedModelRevision: 7, libraryRootId: "root-1",
      thumbnailItemId: "item-1", primaryItemId: null,
    }));
    expect(api.uploadImportItem).toHaveBeenCalledWith("draft-1", "item-1", image);
    expect(api.commitImport).toHaveBeenCalledWith("draft-1");
    expect(api.cancelImport).not.toHaveBeenCalled();
    expect(stages).toEqual(["analyzing", "uploading", "reviewing", "committing"]);
  });

  it("rejects non-images before creating a draft", async () => {
    const api = fakeApi();
    await expect(uploadModelThumbnail(model, new File(["x"], "notes.pdf"), "root-1", undefined, api)).rejects.toThrow("PNG");
    expect(api.previewImport).not.toHaveBeenCalled();
  });

  it("cancels the draft when the review would move existing library content", async () => {
    const api = fakeApi({ reviewImport: vi.fn().mockResolvedValue(review("relocate")) });
    await expect(uploadModelThumbnail(model, image, "root-1", undefined, api)).rejects.toThrow("liegt bereits");
    expect(api.commitImport).not.toHaveBeenCalled();
    expect(api.cancelImport).toHaveBeenCalledWith("draft-1");
  });

  it("cancels the draft when the server does not classify the file as image", async () => {
    const api = fakeApi({ previewImport: vi.fn().mockResolvedValue(draft("other")) });
    await expect(uploadModelThumbnail(model, image, "root-1", undefined, api)).rejects.toThrow("nicht als Bild");
    expect(api.configureImport).not.toHaveBeenCalled();
    expect(api.cancelImport).toHaveBeenCalledWith("draft-1");
  });

  it("keeps the original error when cancelling fails as well", async () => {
    const api = fakeApi({
      uploadImportItem: vi.fn().mockRejectedValue(new Error("network down")),
      cancelImport: vi.fn().mockRejectedValue(new Error("gone")),
    });
    await expect(uploadModelThumbnail(model, image, "root-1", undefined, api)).rejects.toThrow("network down");
  });
});
