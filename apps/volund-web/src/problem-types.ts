export interface ModelProblem {
  key: string;
  severity: "info" | "warning" | "error";
  status: "open" | "ignored" | "resolved";
  code: string;
  message: string;
  sourceId: string;
  sourceName: string;
  sourceUrl: string;
  diagnosticsUrl: string | null;
  previewId: string;
  profile: string;
  occurredAtUnixMs: number;
  remediation: string;
}
