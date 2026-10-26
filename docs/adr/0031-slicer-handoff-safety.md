# ADR 0031: Slicer handoff safety and unavailable targets

- Status: Accepted
- Date: 2026-10-26

## Context

A slicer handoff gives a desktop slicer a five-minute HTTPS bearer URL for one
printable STL or 3MF and opens it through an allowlisted custom scheme
(`prusaslicer://`, `orcaslicer://`, `bambustudio://`). The G.4 gate asked for
safe behavior in every state between creation and download, and for a clear
answer when no target can work.

Several gaps remained:

- The download URL checked only that the token existed and had not expired.
  Until expiry, it kept working after the file stopped being printable, the
  model was removed, the file went missing, the account was disabled, or the
  operator removed the target.
- The URL could be used any number of times, so a leaked copy from slicer
  history or proxy logs stayed valuable for the whole lifetime.
- Responses could be cached, and the URL could leak through `Referer`.
- If targets were configured but `VOLUND_PUBLIC_BASE_URL` was missing,
  `GET /slicer-targets` still listed them. Every click then failed. An invalid
  target list returned 400, which the browser silently treated as "no targets".
- One account could create unbounded handoffs.
- When no program was registered for the scheme, the page simply did nothing.

## Decision

- **Re-validation on every request.** The download is one atomic `UPDATE` that
  increments `use_count`. It matches only if the token is unexpired and under
  its use limit, the creating account is `active`, the model is active, the
  file is still linked and marked printable, and the file is available and
  present. The target must also still be in the current configuration. A
  failed check returns the same 404 as an unknown token.
- **Bounded use.** A token serves at most eight requests. That covers a
  slicer's probe, retries, and range requests. `use_count` and `last_used_at`
  (migration 0033) record the use.
- **No caching or onward leakage.** Downloads carry `Cache-Control: no-store`
  and `Referrer-Policy: no-referrer`.
- **Per-account bound.** At most ten unexpired handoffs per account. Further
  requests receive 429 `slicer_handoff_limit`.
- **Explicit unavailable state.** No configured targets yields `[]`, as before.
  Configured targets that are invalid, or that lack a valid HTTPS public base,
  yield 503 `slicer_handoff_misconfigured` for both listing and creation. The
  daemon logs the reason. The browser shows that the server is not fully set
  up and points to "Original herunterladen" instead of offering buttons.
- **No-reaction hint.** After the launch, the page waits 2.5 s. If it keeps
  focus and stays visible, no local program took the link. The page then says
  so and offers the still-valid download link.
- **Cleanup.** Retention removes handoff rows one day after they expire.
- The slicer configuration is now part of the API state. It is read from the
  environment at startup and can be injected for tests.

## Consequences

- Revoking any precondition stops further downloads at once, not after up to
  five minutes.
- A slicer that needs more than eight requests for one file fails. The
  attachment download remains the fallback.
- The 503 state is a contract change for clients that treated any target-list
  failure as "none"; the browser application distinguishes it now.
- Migration 0033 makes a binary-only rollback to v0.39.0 invalid. The full
  dump rollback applies, and the J.2–J.4 boot probe exercises it.
