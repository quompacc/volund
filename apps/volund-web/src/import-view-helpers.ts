import type { ImportManifestEntry, ImportReview, ImportReviewItem, ModelSummary } from "./types";

export type ImportStage = "analyzing" | "uploading" | "reviewing" | "committing" | "complete";

interface BrowserFile {
  name: string;
  size: number;
  webkitRelativePath?: string;
}

export function importStageLabel(stage: ImportStage): string {
  return ({ analyzing: "Wird analysiert …", uploading: "Wird hochgeladen …", reviewing: "Wird geprüft …",
    committing: "Wird übernommen …", complete: "Abgeschlossen" })[stage];
}

export function extensionTarget(model: ModelSummary): { action: "extend"; modelId: string; revision: number } {
  return { action: "extend", modelId: model.id, revision: model.revision };
}

export function selectedManifest(files: ArrayLike<BrowserFile>): ImportManifestEntry[] {
  return Array.from(files, (file) => ({ path: selectedFilePath(file), byteSize: file.size }));
}

export function selectedFilePath(file: BrowserFile): string {
  return file.webkitRelativePath || file.name;
}

export function uploadPercentage(completed: number, total: number): number {
  if (total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((completed / total) * 100)));
}

export function reviewActionLabel(action: ImportReviewItem["action"]): string {
  return ({ create: "NEU", reuse: "WIEDERVERWENDEN", relocate: "VERSCHIEBEN", conflict: "KONFLIKT", skip: "ÜBERSPRINGEN" })[action];
}

export function reviewTotalFiles(review: ImportReview): number {
  return review.createFiles + review.relocateFiles + review.reuseFiles + review.conflicts + review.skipFiles;
}

export function importSourceName(entries: ImportManifestEntry[]): string {
  const first = entries[0]?.path ?? "Import";
  const root = first.split("/")[0] ?? first;
  if (entries.length > 1 && entries.every((entry) => entry.path.startsWith(`${root}/`))) return root;
  return entries.length === 1 ? root : `${entries.length} Projektdateien`;
}
