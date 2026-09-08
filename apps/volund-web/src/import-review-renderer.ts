import { formatBytes } from "./format";
import { importErrorMessage } from "./import-errors";
import { reviewActionLabel, reviewTotalFiles, uploadPercentage } from "./import-view-helpers";
import type { ImportReview, ImportReviewItem } from "./types";

export function renderImportReview(host: HTMLElement, review: ImportReview): void {
  const totalFiles = reviewTotalFiles(review);
  const section = host.querySelector<HTMLElement>("#import-review")!;
  section.dataset.draftId = review.draftId;
  host.querySelector<HTMLElement>("#review-title")!.textContent = review.modelName;
  const state = host.querySelector<HTMLElement>("#review-state")!;
  state.textContent = review.conflicts === 0 ? "✓ KONFLIKTFREI" : `${review.conflicts} KONFLIKTE`;
  state.classList.toggle("conflict", review.conflicts > 0);
  host.querySelector<HTMLElement>("#review-summary")!.innerHTML = [
    [totalFiles, "Dateien gesamt"], [review.createFiles, "Davon neu"], [review.relocateFiles, "Stabil verschieben"],
    [review.reuseFiles, "Wiederverwenden"], [review.skipFiles, "Übersprungen"],
    [formatBytes(review.savedBytes), "Doppelte Bytes gespart"],
  ].map(([value, label]) => `<article><strong>${value}</strong><span>${label}</span></article>`).join("");
  host.querySelector<HTMLElement>("#review-destination")!.textContent = `${review.rootName} / ${review.baseDirectory}`;
  host.querySelector<HTMLElement>("#review-model-action")!.textContent = review.modelAction === "extend"
    ? "Nur neue Dateien werden ergänzt; bestehende Modellangaben bleiben unverändert."
    : review.modelAction === "update" ? "Das bestehende Modell und seine Metadaten werden aktualisiert."
      : "Ein neues logisches Modell wird angelegt.";
  const items = host.querySelector<HTMLElement>("#review-items")!;
  items.replaceChildren();
  let shown = 0;
  const more = document.createElement("button");
  more.type = "button";
  more.className = "secondary-action";
  more.dataset.reviewMore = "true";
  const showMore = (): void => {
    more.remove();
    items.append(...review.items.slice(shown, shown + 200).map(reviewItemRow));
    shown = Math.min(shown + 200, review.items.length);
    if (shown < review.items.length) {
      more.textContent = `Weitere Dateien anzeigen (${review.items.length - shown} verbleibend)`;
      items.append(more);
    }
  };
  more.addEventListener("click", showMore);
  showMore();
  host.querySelector<HTMLDetailsElement>("#import-review .file-details")!.open = false;
  host.querySelector<HTMLElement>("#review-details-label")!.textContent = `Details anzeigen · ${totalFiles} Dateien`;
  const button = host.querySelector<HTMLButtonElement>("#commit-import")!;
  button.disabled = review.conflicts > 0;
  button.textContent = "Importieren";
  host.querySelector<HTMLElement>("#review-error")!.hidden = true;
  host.querySelector<HTMLElement>("#import-review")!.scrollIntoView({ behavior: "smooth", block: "start" });
}

export function updateUploadProgress(host: HTMLElement, completed: number, total: number): void {
  host.querySelector<HTMLElement>("#upload-progress-count")!.textContent = `${completed} / ${total}`;
  host.querySelector<HTMLElement>("#upload-progress-bar")!.style.width = `${uploadPercentage(completed, total)}%`;
}

export function showUploadError(host: HTMLElement, message: string): void {
  const progress = host.querySelector<HTMLElement>("#upload-progress")!;
  progress.hidden = false;
  progress.classList.add("error");
  host.querySelector<HTMLElement>("#upload-progress-label")!.textContent = "Übertragung unterbrochen";
  host.querySelector<HTMLElement>("#upload-current-file")!.textContent = importErrorMessage(message);
}

function reviewItemRow(item: ImportReviewItem): HTMLElement {
  const row = document.createElement("article");
  row.className = `review-${item.action}${item.isPrimary ? " primary-candidate" : ""}`;
  const details = document.createElement("span");
  const name = document.createElement("strong");
  const path = document.createElement("small");
  const action = document.createElement("b");
  name.textContent = item.originalPath;
  path.textContent = item.action === "reuse" ? item.existingPath || item.targetPath : item.targetPath;
  action.textContent = reviewActionLabel(item.action);
  details.append(name, path);
  row.append(action, details, document.createTextNode(formatBytes(item.byteSize)));
  if (item.action === "conflict") {
    const rename = draftButton("Ziel ändern", "resolution", item.id);
    rename.dataset.resolution = "create";
    rename.dataset.itemId = item.id;
    rename.dataset.targetPath = item.targetPath;
    const skip = draftButton("Überspringen", "resolution", item.id);
    skip.dataset.resolution = "skip";
    skip.dataset.itemId = item.id;
    row.append(rename);
    if (!item.isPrimary) row.append(skip);
  }
  return row;
}

function draftButton(label: string, action: string, draftId: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "secondary-action";
  button.textContent = label;
  button.dataset.draftAction = action;
  button.dataset.draftId = draftId;
  return button;
}
