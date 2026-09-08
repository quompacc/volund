import { describe, expect, it } from "vitest";
import { problemCenterMarkup } from "./problem-center";
import type { ModelProblem } from "./problem-types";

const problem: ModelProblem = {
  key: "preview:0", severity: "error", status: "open", code: "conversion.failed",
  message: "Fehler <script>", sourceId: "source", sourceName: "sehr/lang/<teil>.step",
  sourceUrl: "/source", diagnosticsUrl: "/diagnostic", previewId: "preview", profile: "fine",
  occurredAtUnixMs: 1, remediation: "Profil prüfen",
};

describe("model problem center", () => {
  it("filters by severity and renders safe source/derived remediation controls", () => {
    const markup = problemCenterMarkup([problem, { ...problem, key: "info", severity: "info" }], "warning", true);
    expect(markup).toContain("1 von 2");
    expect(markup).toContain("Fehler &lt;script&gt;");
    expect(markup).not.toContain("<script>");
    expect(markup).toContain("Original herunterladen");
    expect(markup).toContain("Abgeleitete Diagnose öffnen");
    expect(markup).toContain("Auswahl ignorieren");
  });
});
