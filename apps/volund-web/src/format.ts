import type { Preview } from "./types";

export function fileName(path: string): string {
  return path.split("/").at(-1) ?? path;
}

export function parentPath(path: string): string {
  const segments = path.split("/");
  return segments.length > 1 ? segments.slice(0, -1).join("/") : "Wurzelverzeichnis";
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = units[0] ?? "KB";
  for (const next of units.slice(1)) {
    if (value < 1024) break;
    value /= 1024;
    unit = next;
  }
  return `${value.toLocaleString("de-DE", { maximumFractionDigits: 1 })} ${unit}`;
}

export function formatDate(timestamp: number): string {
  return new Intl.DateTimeFormat("de-DE", {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(timestamp));
}

export function newestReadyPreview(previews: Preview[]): Preview | undefined {
  return previews.find(
    (preview) =>
      preview.status === "ready" &&
      preview.artifacts.some((artifact) => artifact.kind === "preview-glb"),
  );
}

export function previewLabel(status: string): string {
  const labels: Record<string, string> = {
    queued: "Vorschau eingeplant",
    running: "Vorschau wird erzeugt",
    ready: "Vorschau bereit",
    failed: "Vorschau fehlgeschlagen",
  };
  return labels[status] ?? status;
}
