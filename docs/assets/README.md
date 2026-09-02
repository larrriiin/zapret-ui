# Landing page assets

- `app-screenshot.png`: the published application screenshot from the project README, saved locally from https://github.com/user-attachments/assets/b7bcd009-2083-4c84-935c-8fc92640a2b3 (1102 × 979). When replacing it, update the image dimensions and alternative text in `docs/index.html`.
- `icon.png`: copied from `src-tauri/icons/128x128.png`.
- `inter-400.ttf` and `inter-700.ttf`: copied from `src/assets/fonts/` to keep the website typography consistent with the application and avoid external font requests.

The `docs/` site is static and self-contained; publish the directory as-is without the Tauri/Vite build.
