import type { StralarAvailability } from "./types";

/** File formats the Stralar slicer imports: meshes and CAD (STEP/IGES are tessellated there). */
const STRALAR_FORMATS = /\.(stl|3mf|obj|step|stp|iges|igs)$/i;

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

/** Whether to offer Stralar; any failure counts as "not available". */
export async function loadStralarAvailability(load: () => Promise<StralarAvailability>): Promise<boolean> {
  try {
    return (await load()).available === true;
  } catch {
    return false;
  }
}
