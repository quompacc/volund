import { afterEach, expect, it, vi } from "vitest";
import { mutationJson, postDownload, postFile, requestJson } from "./api";
import { checkSessionResponse, onSessionExpired } from "./session-expiry";

afterEach(() => { checkSessionResponse(401); vi.unstubAllGlobals(); });

it("ignores anonymous login errors and non-authentication failures", () => {
  checkSessionResponse(401);
  const reload = vi.fn();
  onSessionExpired(reload);
  for (const status of [200, 403, 404, 500]) checkSessionResponse(status);
  expect(reload).not.toHaveBeenCalled();
  checkSessionResponse(401);
  checkSessionResponse(401);
  expect(reload).toHaveBeenCalledTimes(1);
});

it.each(["read", "mutation", "upload", "download"])("ends an authenticated session on a %s 401 even with a non-JSON body", async (kind) => {
  const reload = vi.fn();
  onSessionExpired(reload);
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("authentication is required", { status: 401 })));
  const request = kind === "read" ? requestJson("/api/v1/settings")
    : kind === "mutation" ? mutationJson("/api/v1/preferences", "PUT", {})
    : kind === "upload" ? postFile("/api/v1/imports/test/upload", new Blob())
    : postDownload("/api/v1/support-bundle");
  await expect(request).rejects.toMatchObject({ status: 401 });
  expect(reload).toHaveBeenCalledTimes(1);
});
