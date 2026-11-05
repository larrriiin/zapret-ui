# Landing page assets

- `app-screenshot.png`: a copy of the current generated home screenshot (2200 × 1960). Run `npm run screenshots` to refresh it together with `screenshots/` and `docs/screenshots/`. The generator captures the current UI using demonstration data, without reading personal settings or changing system connections.
- `icon.png`: copied from `src-tauri/icons/128x128.png`.
- `inter-400.ttf` and `inter-700.ttf`: copied from `src/assets/fonts/` to keep the website typography consistent with the application and avoid external font requests.

The `docs/` site is static and self-contained; publish the directory as-is without the Tauri/Vite build.
