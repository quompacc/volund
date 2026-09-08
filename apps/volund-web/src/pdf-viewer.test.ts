import { describe, expect, it } from "vitest";
import { normalizedPdfPage, pdfViewerMarkup, steppedPdfScale } from "./pdf-viewer";

describe("PDF viewer", () => {
  it("renders explicit page, zoom, and original controls", () => {
    const markup = pdfViewerMarkup("Manual.pdf", "2 MB", "/api/v1/files/id/content");
    expect(markup).toContain("data-pdf-previous");
    expect(markup).toContain("data-pdf-canvas");
    expect(markup).toContain("Original öffnen");
    expect(markup).toContain("Manual.pdf");
  });

  it("escapes stored filenames and source attributes", () => {
    const markup = pdfViewerMarkup('<Manual "final">.pdf', "2 MB", "/content?id=1&mode=pdf");
    expect(markup).toContain("&lt;Manual &quot;final&quot;&gt;.pdf");
    expect(markup).toContain("/content?id=1&amp;mode=pdf");
    expect(markup).not.toContain('<Manual "final">');
  });

  it("keeps page and zoom navigation inside supported bounds", () => {
    expect(normalizedPdfPage(-4, 8)).toBe(1);
    expect(normalizedPdfPage(12, 8)).toBe(8);
    expect(steppedPdfScale(0.4, -1)).toBe(0.4);
    expect(steppedPdfScale(3, 1)).toBe(3);
    expect(steppedPdfScale(1, 1)).toBe(1.2);
  });
});
