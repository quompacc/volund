import { catalogApi } from "./api";
import { buildMetadataRequest } from "./import-metadata";
import { importSourceName, selectedManifest } from "./import-view-helpers";
import type { ImportCommit, ImportReview, LibraryRoot, ModelFile, ModelSummary } from "./types";

/** File picker filter for images that ADR 0023 accepts as thumbnail sources. */
export const THUMBNAIL_UPLOAD_ACCEPT = "image/png,image/jpeg,image/gif,image/webp,.png,.jpg,.jpeg,.gif,.webp";
const RASTER_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "webp"]);

export type ThumbnailUploadStage = "analyzing" | "uploading" | "reviewing" | "committing";
export type ThumbnailUploadApi = Pick<typeof catalogApi,
  "previewImport" | "configureImport" | "uploadImportItem" | "reviewImport" | "commitImport" | "cancelImport">;
type ThumbnailTarget = Pick<ModelSummary, "id" | "name" | "kind" | "revision">;

export function isThumbnailImage(file: { name: string }): boolean {
  const extension = file.name.split(".").pop()?.toLowerCase() ?? "";
  return file.name.includes(".") && RASTER_EXTENSIONS.has(extension);
}

export function thumbnailUploadStageLabel(stage: ThumbnailUploadStage): string {
  return ({ analyzing: "Bild wird geprüft …", uploading: "Bild wird hochgeladen …",
    reviewing: "Ablage wird geprüft …", committing: "Vorschaubild wird übernommen …" })[stage];
}

/** Writable library for the new image: the primary file's library, then any linked file's, then the first writable one. */
export function thumbnailLibraryRootId(files: ModelFile[], roots: LibraryRoot[]): string | null {
  const writable = roots.filter((root) => !root.readOnly);
  const keys = [...files].sort((left, right) => Number(right.primary) - Number(left.primary)).map((file) => file.rootKey);
  for (const key of keys) {
    const root = writable.find((candidate) => candidate.key === key);
    if (root) return root.id;
  }
  return writable[0]?.id ?? null;
}

/** Only a plain new or reused file is safe; anything else would move or replace library content. */
export function thumbnailReviewProblem(review: ImportReview): string | null {
  const item = review.items[0];
  if (!item) return "Das Bild wurde nicht in den Importplan übernommen.";
  if (item.action === "create" || item.action === "reuse") return null;
  if (item.action === "relocate") {
    return `Dieses Bild liegt bereits unter „${item.existingPath ?? item.targetPath}“ in der Bibliothek. Wähle es dort aus oder lade eine andere Datei hoch.`;
  }
  if (item.action === "conflict") {
    return `Unter „${item.targetPath}“ liegt bereits eine andere Datei. Benenne das Bild um und versuche es erneut.`;
  }
  return "Das Bild würde übersprungen und kann deshalb nicht als Vorschaubild gesetzt werden.";
}

/**
 * Adds one image to an existing model through the regular extend import
 * (staging, SHA-256, review, atomic publication) and selects it as thumbnail
 * in the same commit. A failed attempt cancels its draft.
 */
export async function uploadModelThumbnail(
  model: ThumbnailTarget,
  file: File,
  libraryRootId: string,
  onStage: (stage: ThumbnailUploadStage) => void = () => undefined,
  api: ThumbnailUploadApi = catalogApi,
): Promise<ImportCommit> {
  if (!isThumbnailImage(file)) throw new Error("Bitte ein PNG-, JPEG-, GIF- oder WebP-Bild wählen.");
  const entries = selectedManifest([file]);
  onStage("analyzing");
  const draft = await api.previewImport(importSourceName(entries), entries);
  try {
    const item = draft.items[0];
    if (draft.items.length !== 1 || item?.category !== "image") throw new Error("Die Datei wurde nicht als Bild erkannt.");
    await api.configureImport(draft.id, buildMetadataRequest({
      modelName: model.name, kind: model.kind, libraryRootId, description: "", authorName: "",
      tags: [], collectionIds: [], targetAction: "extend", targetModel: model, thumbnailItemId: item.id,
    }));
    onStage("uploading");
    await api.uploadImportItem(draft.id, item.id, file);
    onStage("reviewing");
    const problem = thumbnailReviewProblem(await api.reviewImport(draft.id));
    if (problem) throw new Error(problem);
    onStage("committing");
    return await api.commitImport(draft.id);
  } catch (error) {
    await api.cancelImport(draft.id).catch(() => undefined);
    throw error;
  }
}
