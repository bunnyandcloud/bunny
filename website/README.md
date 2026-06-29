# Bunny documentation site

Built with [Docusaurus](https://docusaurus.io/). Published at [docs.bunnyandcloud.com](https://docs.bunnyandcloud.com).

## Local development

```bash
cd website
npm ci
npm start
```

## Build

```bash
npm run build
npm run serve
```

## Deploy

Pushes to `main` that touch `website/` deploy via [`.github/workflows/docs.yml`](../.github/workflows/docs.yml) to GitHub Pages.

DNS: CNAME `docs.bunnyandcloud.com` → GitHub Pages (see `static/CNAME`).
