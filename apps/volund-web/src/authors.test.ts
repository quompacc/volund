import { describe, expect, it } from "vitest";
import { authorMarkup } from "./authors";
import type { AuthorSummary } from "./types";

describe("author lifecycle presentation", () => {
  it("escapes metadata and exposes revisioned administration only to administrators", () => {
    const author: AuthorSummary = {
      id: "author-1", name: "Maker <Lab>", website: "https://example.test/?a=1&b=2",
      provenanceSource: "website", provenanceNote: "Profile <verified>", active: true,
      mergedIntoId: null, modelCount: 3, revision: 4, updatedAtUnixMs: 0,
    };
    const managed = authorMarkup([author, { ...author, id: "author-2", name: "Target" }], true);
    expect(managed).toContain("Maker &lt;Lab&gt;");
    expect(managed).toContain("Profile &lt;verified&gt;");
    expect(managed).toContain("Revision 4");
    expect(managed).toContain('data-edit-author="author-1"');
    expect(managed).toContain('data-merge-author="author-1"');
    const readOnly = authorMarkup([author], false);
    expect(readOnly).not.toContain("data-edit-author");
    expect(readOnly).not.toContain("data-merge-author");
  });

  it("has an honest empty state", () => {
    expect(authorMarkup([], true)).toContain("Noch keine Autoren");
  });
});
