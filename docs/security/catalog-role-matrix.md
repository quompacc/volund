# Phase 3 catalog role matrix

All routes additionally require an active authenticated session, completed
password change, same-origin mutation request, and a valid double-submit CSRF
token. Server capabilities are authoritative; hidden browser controls are only a
usability aid.

| Catalog action | Viewer | Editor | Administrator | Owner |
| --- | --- | --- | --- | --- |
| Read models, files, thumbnails, authors, tags, collections, history | yes | yes | yes | yes |
| Edit model metadata, primary file, thumbnail, components | no | yes | yes | yes |
| Import and change collection membership | no | yes | yes | yes |
| Create a collection in model/import context | no | yes | yes | yes |
| Rename/remove collections; administer/merge authors or tags | no | no | yes | yes |
| Read global rejected security attempts | no | no | yes | yes |

Model history is model-scoped business evidence. It never returns global denied
requests, session events, client addresses, request IDs, absolute paths, headers,
environment values, or submitted confirmation strings.
