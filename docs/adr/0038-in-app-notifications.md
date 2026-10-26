# ADR 0038: In-app notification center

- Status: Accepted
- Date: 2026-10-26

## Context

The product contract lists notifications for job failures, capacity, backup,
and security events. The information already existed in the operations view,
the problem view, and the security audit, but nothing told a user that it was
worth looking. The operator chose in-app notifications only: no email, no
webhook, nothing sent outside VÖLUND.

## Decision

- `GET /api/v1/notifications` derives notifications on request; nothing is
  queued or persisted except one read marker per account. There are two kinds:
  - **Conditions** from operations health stay unread until their cause is
    fixed:
    - backup degraded or blocked;
    - a background worker degraded or blocked;
    - library storage degraded or blocked.
  - **Events** from the last 7 days (at most 20 of each type) are unread until
    the account opens the center:
    - failed or timed-out conversions, linked to the affected model's problem
      page when the file belongs to an active model;
    - failed scans;
    - in the last 24 hours, at least 5 denied logins (warning), and any other
      denied action (info).
- **Audience.** Administrators see conditions, security events, and failed
  scans. Catalog editors see failed conversions. Viewers see nothing, because
  they cannot act on any of it.
- **Read marker.** `POST /api/v1/notifications/seen` stores the moment the
  account opened the center. The response's `attention` value (active
  conditions plus unread events) drives the badge.
- **Browser.** A bell in the top bar shows the badge, refreshes every minute,
  and opens a keyboard-closable panel. Every entry links to the view that
  resolves it.

## Consequences

- Problems are pushed to the people who can act on them, without new
  infrastructure, secrets, or outbound traffic.
- Users only see notifications in an open browser tab. An external channel
  (webhook or email) can be added on top of the same derivation later.
- Each request runs the operations health check. Its cost is the storage
  probes of the registered libraries, a few milliseconds each.
