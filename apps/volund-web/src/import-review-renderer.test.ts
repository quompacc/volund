// @vitest-environment happy-dom
import { expect, it } from "vitest";
import { renderImportReview } from "./import-review-renderer";
import type { ImportReview } from "./types";

function fixture(count: number, primary = false): { host: HTMLElement; review: ImportReview } {
  const host = document.createElement("main");
  host.innerHTML = `<section id="import-review"><details class="file-details"></details></section><div id="review-title"></div><div id="review-state"></div><div id="review-summary"></div><div id="review-destination"></div><div id="review-model-action"></div><div id="review-items"></div><div id="review-details-label"></div><button id="commit-import"></button><div id="review-error"></div>`;
  host.querySelector<HTMLElement>("#import-review")!.scrollIntoView = () => undefined;
  const review = {
    draftId: "draft", modelName: "Test", modelAction: "create", rootName: "Test", baseDirectory: "parts",
    createFiles: count - 1, reuseFiles: 0, relocateFiles: 0, skipFiles: 0, conflicts: 1, newBytes: 4 * (count - 1), savedBytes: 0,
    items: Array.from({ length: count }, (_, i) => ({ id: String(i), originalPath: `file-${i}.step`, category: "cad", byteSize: 4, sha256: "a".repeat(64), action: i === count - 1 ? "conflict" : "create", targetPath: `parts/file-${i}.step`, isPrimary: primary && i === count - 1 })),
  } as ImportReview;
  return { host, review };
}

it("makes conflicts after multiple pages reachable without duplicate rows", () => {
  const { host, review } = fixture(401);
  renderImportReview(host, review);
  expect(host.querySelectorAll("#review-items article")).toHaveLength(200);
  host.querySelector<HTMLButtonElement>("[data-review-more]")!.click();
  expect(host.querySelectorAll("#review-items article")).toHaveLength(400);
  host.querySelector<HTMLButtonElement>("[data-review-more]")!.click();
  expect(host.querySelectorAll("#review-items article")).toHaveLength(401);
  expect(host.querySelector('[data-resolution="create"][data-item-id="400"]')).not.toBeNull();
  expect(host.querySelector('[data-resolution="skip"][data-item-id="400"]')).not.toBeNull();
  expect(host.querySelector("[data-review-more]")).toBeNull();
  expect(host.querySelector<HTMLButtonElement>("#commit-import")!.disabled).toBe(true);
  renderImportReview(host, review);
  expect(host.querySelectorAll("#review-items article")).toHaveLength(200);
});

it("does not offer skipping the selected primary and handles a single page", () => {
  const { host, review } = fixture(1, true);
  renderImportReview(host, review);
  expect(host.querySelector('[data-resolution="skip"]')).toBeNull();
  expect(host.querySelector('[data-resolution="create"]')).not.toBeNull();
  expect(host.querySelector("[data-review-more]")).toBeNull();
});
