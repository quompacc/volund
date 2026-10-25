import { catalogApi } from "./api";
import { importErrorMessage } from "./import-errors";
import { THUMBNAIL_UPLOAD_ACCEPT, isThumbnailImage, thumbnailUploadStageLabel, uploadModelThumbnail, type ThumbnailUploadApi } from "./thumbnail-upload";
import type { ImportCommit } from "./types";

/** Select value standing for an image chosen outside the import package. */
export const UPLOADED_THUMBNAIL_VALUE = "upload";

export interface ImportThumbnailChoice {
  /** Forgets a picked image, e.g. when a new draft replaces the old one. */
  reset(): void;
  /** Adds the picked image to the committed model and selects it as thumbnail. */
  applyAfterCommit(result: ImportCommit): Promise<void>;
}

export function importThumbnailPickerMarkup(): string {
  return `<div class="thumbnail-upload-pick"><label class="secondary-action"><span class="icon icon-image" aria-hidden="true"></span>Eigenes Bild hochladen<input id="metadata-thumbnail-upload" type="file" accept="${THUMBNAIL_UPLOAD_ACCEPT}"></label><small id="metadata-thumbnail-upload-note" class="form-message" aria-live="polite">Für Pakete ohne passendes Bild: wird nach dem Import ergänzt und als Vorschaubild gesetzt.</small></div>`;
}

/** The draft item to send as thumbnailItemId; an uploaded image is applied after the commit instead. */
export function draftThumbnailItemId(value: string): string | null {
  return value && value !== UPLOADED_THUMBNAIL_VALUE ? value : null;
}

export function mountImportThumbnailChoice(
  host: HTMLElement,
  api: ThumbnailUploadApi & Pick<typeof catalogApi, "model"> = catalogApi,
): ImportThumbnailChoice {
  const input = host.querySelector<HTMLInputElement>("#metadata-thumbnail-upload")!;
  const select = host.querySelector<HTMLSelectElement>("#metadata-thumbnail")!;
  const note = host.querySelector<HTMLElement>("#metadata-thumbnail-upload-note")!;
  const defaultNote = note.textContent ?? "";
  let pending: File | null = null;
  const removeOption = (): void => select.querySelector(`option[value="${UPLOADED_THUMBNAIL_VALUE}"]`)?.remove();
  input.addEventListener("change", () => {
    const file = input.files?.[0];
    input.value = "";
    if (!file) return;
    if (!isThumbnailImage(file)) {
      note.classList.add("error");
      note.textContent = "Bitte ein PNG-, JPEG-, GIF- oder WebP-Bild wählen.";
      return;
    }
    pending = file;
    removeOption();
    const option = document.createElement("option");
    option.value = UPLOADED_THUMBNAIL_VALUE;
    option.textContent = `Eigenes Bild · ${file.name}`;
    select.append(option);
    select.value = UPLOADED_THUMBNAIL_VALUE;
    note.classList.remove("error");
    note.textContent = `„${file.name}“ wird nach dem Import dem Modell hinzugefügt und als Vorschaubild gesetzt.`;
  });
  return {
    reset: () => {
      pending = null;
      removeOption();
      note.classList.remove("error");
      note.textContent = defaultNote;
    },
    applyAfterCommit: async (result) => {
      if (!pending || select.value !== UPLOADED_THUMBNAIL_VALUE) return;
      const status = host.querySelector<HTMLElement>("#review-confirm-note")!;
      const libraryRootId = host.querySelector<HTMLSelectElement>("#metadata-library")!.value;
      try {
        const model = await api.model(result.modelId);
        await uploadModelThumbnail(model, pending, libraryRootId, (stage) => { status.textContent = thumbnailUploadStageLabel(stage); }, api);
        pending = null;
        status.textContent = "Modell gespeichert. Das eigene Bild ist ergänzt und als Vorschaubild gesetzt.";
      } catch (error) {
        status.classList.add("error");
        status.textContent = `Modell gespeichert, aber das Vorschaubild fehlt: ${importErrorMessage(error instanceof Error ? error.message : "Upload fehlgeschlagen.")} Du kannst es auf der Modellseite erneut hochladen.`;
      }
    },
  };
}
