import { describe, expect, it } from "vitest";
import { MOCK_MODELS, mockFileTotal } from "./mock-models";

describe("mock model metrics", () => {
  it("totals every file represented by a model", () => {
    expect(mockFileTotal(MOCK_MODELS)).toBe(166);
    expect(mockFileTotal([])).toBe(0);
  });
});
