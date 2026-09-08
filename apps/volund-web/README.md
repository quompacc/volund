# VÖLUND browser application

The browser application is framework-free TypeScript with Three.js. Vite and
Node.js are build-time dependencies only; production serves `dist/` from the
native Rust daemon.

Catalog folders are virtual and derived from indexed relative paths. Folder
navigation, recursive search, format filtering, sorting, pagination, and the
selected file are represented in the URL, so views can be bookmarked and the
browser history remains useful.

```sh
npm ci
npm test
npm run build
```

For local development, `npm run dev` proxies `/api` to the loopback VÖLUND API.
Production assets are installed at `/usr/share/volund/web` and remain read-only
to the API systemd service.

The import view sends only selected file paths and sizes to the preview API. It
does not read or upload file contents. Selected directories therefore produce a
real persisted draft, while archive-content streaming remains a later stage.
