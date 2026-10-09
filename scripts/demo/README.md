# Demo video and README screenshots

Made from the real Syscura interface, fed by invented demo data (`mock.js`), so no real PC is shown.

```powershell
cd ui; npm run build; npx vite preview --port 4173    # keep this running
cd scripts\demo; npm install; npx playwright install chromium
node record.mjs     # 4K stills, captions, title cards -> frames\, README shots -> readme\
python build.py     # needs ffmpeg -> syscura-demo.mp4 (1920x1080, ~25 s)
```

Copy `readme\*.png` to `docs\screenshots\` to refresh the README.
