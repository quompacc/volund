import type { ImportDraft, ImportManifestEntry } from "./types";

type Item = ImportDraft["items"][number];

// Uploaded ZIPs may still need expansion retried; extracted ordinary items
// already live on the server and must not be required in the browser selection.
export function importUploadItems(items: Item[]): Item[] {
  return items.filter((item) => item.uploadStatus !== "uploaded" || item.category === "archive");
}

export function matchesResumeSelection(items: Item[], entries: ImportManifestEntry[]): boolean {
  const selected = new Map(entries.map((entry) => [entry.path, entry.byteSize]));
  const expected = new Map(items.map((item) => [item.originalPath, item.byteSize]));
  return selected.size === entries.length
    && entries.every((entry) => expected.get(entry.path) === entry.byteSize)
    && importUploadItems(items).every((item) => selected.get(item.originalPath) === item.byteSize);
}
