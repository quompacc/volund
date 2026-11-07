import type { ModelFile, StralarAvailability } from "./types";

/** File formats the Stralar slicer imports: meshes and CAD (STEP/IGES are tessellated there). */
const STRALAR_FORMATS = /\.(stl|3mf|obj|step|stp|iges|igs)$/i;
const MESH_FORMATS = /\.(3mf|stl|obj)$/i;

/** Base path of the Stralar studio behind VÖLUND (ADR 0042). */
export const STRALAR_PATH = "/stralar/";

/** True for files Stralar can open, judged by the file name. */
export function isStralarImportable(path: string): boolean {
  return STRALAR_FORMATS.test(path);
}

/**
 * Link that opens a file in Stralar. The studio runs under VÖLUND's origin, so it fetches the
 * same-origin download itself with the current session — no bearer link is created.
 */
export function stralarOpenUrl(fileId: string, path: string): string {
  const name = path.split(/[\\/]/).at(-1) ?? "";
  const file = `/api/v1/files/${encodeURIComponent(fileId)}/content`;
  return `${STRALAR_PATH}#/open?file=${encodeURIComponent(file)}&name=${encodeURIComponent(name)}`;
}

/**
 * The file of a model to open in Stralar from the model page: a printable mesh first (what one
 * slices), then any mesh, then the primary STEP, then any other CAD file. Missing files never.
 */
export function pickStralarFile(files: readonly ModelFile[], primaryFileId: string | null): ModelFile | null {
  const rank = (file: ModelFile): number => {
    if (MESH_FORMATS.test(file.path)) return file.printable ? 0 : 1;
    return file.primary || file.id === primaryFileId ? 2 : 3;
  };
  const usable = files.filter((file) => !file.missing && isStralarImportable(file.path));
  return [...usable].sort((a, b) => rank(a) - rank(b))[0] ?? null;
}

/** "In Stralar öffnen" link for the model page header (new tab, same session). */
export function stralarActionMarkup(file: ModelFile | null): string {
  if (!file) return "";
  const href = stralarOpenUrl(file.id, file.path).replaceAll("&", "&amp;");
  return `<a class="secondary-action" href="${href}" target="_blank" rel="noopener" data-stralar-open title="Dieses Modell im Slicer Stralar öffnen"><span class="icon icon-link" aria-hidden="true"></span>In Stralar öffnen</a>`;
}

/** Whether to offer Stralar; any failure counts as "not available". */
export async function loadStralarAvailability(load: () => Promise<StralarAvailability>): Promise<boolean> {
  try {
    return (await load()).available === true;
  } catch {
    return false;
  }
}
