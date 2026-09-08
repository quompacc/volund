// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { catalogApi } from "./api";
import { mountMockView } from "./mockup";

vi.mock("./pdf-viewer", () => ({ PdfViewer: class {}, pdfViewerMarkup: vi.fn() }));
afterEach(() => vi.restoreAllMocks());

it.each(["dashboard", "models"] as const)("explains an empty %s without inventing a tag filter", async (view) => {
  vi.spyOn(catalogApi, "models").mockResolvedValue([]);
  vi.spyOn(catalogApi, "tags").mockResolvedValue({ items: [], limit: 100, offset: 0, total: 0 });
  vi.spyOn(catalogApi, "roots").mockResolvedValue([]);
  const host = document.createElement("div");
  const dispose = mountMockView(host, view, vi.fn(), null, true, true);
  await vi.waitFor(() => expect(host.textContent).toContain("Noch keine Modelle"));
  expect(host.textContent).not.toContain("anderen Tag");
  expect(host.textContent).toContain("Importiere ein Modell");
  dispose();
});
