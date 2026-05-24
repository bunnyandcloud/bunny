# Contributing

## Development setup

```bash
./bunny configure
./bunny run --web-ui
# optional: cd apps/web && npm run dev  (Vite HMR on :5173, proxies API)
cd apps/mobile && flutter pub get
```

## Monorepo layout

- Rust workspace at repo root
- Web and mobile apps under `apps/`
- Shared contracts under `packages/`

## Pull requests

- Keep changes focused
- Run `cargo test` and `npm run build` in apps/web before submitting
