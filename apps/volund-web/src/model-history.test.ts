import { describe, expect, it } from "vitest";
import { historyMarkup } from "./model-history";

describe("model business history", () => {
  it("renders actor and allowlisted change data without trusting markup", () => {
    const markup = historyMarkup([{
      id: "event-1", actorDisplayName: "Maker <admin>", action: "model.update",
      outcome: "success", occurredAtUnixMs: 0, change: { fields: ["name", "license"], revision: 3 },
    }]);
    expect(markup).toContain("Modelldaten geändert");
    expect(markup).toContain("Maker &lt;admin&gt;");
    expect(markup).toContain("Felder: name, license · Revision 3");
  });

  it("has a truthful empty state", () => {
    expect(historyMarkup([])).toContain("Noch keine Änderungen");
  });
});
