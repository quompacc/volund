# ADR 0033: Central problem view

- Status: Accepted
- Date: 2026-10-26

## Context

Problems were visible only on each model's problem page: conversion
diagnostics and invalid assembly manifests, with durable ignore and resolve
decisions. Nobody could see at a glance which models in the archive needed
attention. Other issues were not shown as problems at all: originals that
disappeared from a library, and libraries whose last scan failed.

## Decision

- `GET /api/v1/problems` returns one sorted, paged list from three sources:
  1. the per-model conversion and assembly problems, produced by the same code
     as the model problem page, so keys, severities, and ignore/resolve states
     agree;
  2. originals linked to an active model that are missing or not `available`
     (error);
  3. libraries whose latest finished scan failed (error).
- **Filters:** minimum severity (default `warning`) and status (default
  `open`).
- **Sorting:** severity first, then most recent first.
- **Counts:** the response carries per-severity counts for the status filter,
  for the navigation badge and the summary line.
- The overview inspects at most the 2,000 most recently edited models with
  candidate diagnostics. It reports `truncated` when more exist instead of
  growing without bound.
- Decisions stay where their context is: the overview only links to the
  model's problem page, where problems are ignored or resolved, or to library
  administration. Missing-file and scan problems are always `open` and
  disappear when their cause is fixed.
- The browser gets a "Probleme" view with severity and status filters and a
  navigation badge counting open errors and warnings.

## Consequences

- Operators can work through the catalog's problems from one place.
- The overview's cost grows with the number of models that have diagnostics,
  bounded by the 2,000-model limit. A persisted problem index can replace the
  on-demand aggregation if this becomes slow.
