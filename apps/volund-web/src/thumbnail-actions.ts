import { catalogApi } from "./api";
import { importErrorMessage } from "./import-errors";
import { thumbnailLibraryRootId, thumbnailUploadStageLabel, uploadModelThumbnail, type ThumbnailUploadApi } from "./thumbnail-upload";
import type { ModelFile, ModelSummary } from "./types";

type ThumbnailActionsApi = ThumbnailUploadApi & Pick<typeof catalogApi, "updateThumbnail" | "roots">;

/** Wires the thumbnail panel: choosing an existing image and uploading a new one. */
export function mountThumbnailActions(
  host: HTMLElement,
  model: ModelSummary,
  files: ModelFile[],
  reload: () => void = () => window.location.reload(),
  api: ThumbnailActionsApi = catalogApi,
): void {
  host.querySelector<HTMLFormElement>("#thumbnail-form")?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const status = form.querySelector<HTMLElement>("#thumbnail-status")!;
    const button = form.querySelector<HTMLButtonElement>("button")!;
    const value = String(new FormData(form).get("thumbnail") || "");
    const [kind, candidateId] = value ? value.split("|", 2) : ["default", null];
    button.disabled = true; status.classList.remove("error"); status.textContent = "Auswahl wird gespeichert …";
    try {
      await api.updateThumbnail(model, kind as ModelSummary["thumbnail"]["kind"], candidateId ?? null);
      status.textContent = "Vorschaubild gespeichert.";
      reload();
    } catch (error) {
      status.classList.add("error"); status.textContent = error instanceof Error ? error.message : "Vorschaubild konnte nicht gespeichert werden."; status.focus();
    } finally { button.disabled = false; }
  });
  const input = host.querySelector<HTMLInputElement>("#thumbnail-upload");
  input?.addEventListener("change", async () => {
    const file = input.files?.[0];
    if (!file) return;
    const status = host.querySelector<HTMLElement>("#thumbnail-upload-status")!;
    input.disabled = true;
    status.classList.remove("error");
    try {
      const libraryRootId = thumbnailLibraryRootId(files, await api.roots());
      if (!libraryRootId) throw new Error("Es gibt keine beschreibbare Bibliothek für das Bild.");
      await uploadModelThumbnail(model, file, libraryRootId, (stage) => { status.textContent = thumbnailUploadStageLabel(stage); }, api);
      status.textContent = "Bild hinzugefügt und als Vorschaubild gesetzt.";
      reload();
    } catch (error) {
      status.classList.add("error");
      status.textContent = importErrorMessage(error instanceof Error ? error.message : "Bild konnte nicht hochgeladen werden.");
      status.focus();
    } finally {
      input.disabled = false;
      input.value = "";
    }
  });
}
