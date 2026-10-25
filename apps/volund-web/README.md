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
Set `VOLUND_API_PROXY` to use another daemon, for example through an SSH tunnel
(`VOLUND_API_PROXY=http://127.0.0.1:18080 npm run dev`). If that instance uses
secure cookies, also set `VOLUND_API_SECURE_COOKIES=true` so mutations pass its
same-origin check.
Production assets are installed at `/usr/share/volund/web` and remain read-only
to the API systemd service.

The import view first sends only selected file paths and sizes to the preview
API, which creates a persisted draft. After the metadata step it streams each
file into the isolated staging area (ADR 0014); the daemon expands uploaded ZIP
archives there. The library changes only when the reviewed draft is committed.

The interface supports System, Light, and Dark themes. All colours are tokens in
`src/theme.css`; component styles must not hard-code colours.
