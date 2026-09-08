import { describe, expect, it } from "vitest";
import { historyMarkup } from "./catalog-history";

describe("catalog history", () => {
  it("renders a semantic sanitized list without raw JSON", () => {
    const markup = historyMarkup([{ id: "event-1", actorId: "actor-1", actorName: "Owner <script>",
      timestampUnixMs: 0, action: "tag.remove", targetType: "tag", targetId: "tag-1",
      targetName: "CoreXY", outcome: "denied", revision: 2,
      summary: { fields: ["name", "description"], code: "wrong_confirmation" } }]);
    expect(markup).toContain("Geänderte Felder");
    expect(markup).toContain("name, description");
    expect(markup).toContain("Ziel: CoreXY · tag · Revision 2");
    expect(markup).not.toContain("<pre>");
    expect(markup).not.toContain("<script>");
  });
});
