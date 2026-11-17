"""Serve dist/WallFX.zip on the LAN via FastAPI."""

from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.responses import FileResponse, HTMLResponse

ROOT = Path(__file__).resolve().parents[1]
ZIP_PATH = ROOT / "dist" / "WallFX.zip"

app = FastAPI(title="WallFX share")


@app.get("/", response_class=HTMLResponse)
def index():
    if not ZIP_PATH.is_file():
        return HTMLResponse("<h1>WallFX.zip missing</h1><p>Build dist/WallFX.zip first.</p>", status_code=404)
    mb = ZIP_PATH.stat().st_size / (1024 * 1024)
    return f"""<!doctype html>
<html><head><meta charset="utf-8"><title>WallFX download</title></head>
<body style="font-family:system-ui;max-width:40rem;margin:3rem auto;padding:0 1rem">
  <h1>WallFX</h1>
  <p>Download the ship package ({mb:.1f} MB).</p>
  <p><a href="/WallFX.zip">Download WallFX.zip</a></p>
</body></html>"""


@app.get("/WallFX.zip")
def download_zip():
    if not ZIP_PATH.is_file():
        return HTMLResponse("missing", status_code=404)
    return FileResponse(
        ZIP_PATH,
        media_type="application/zip",
        filename="WallFX.zip",
    )


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=8765, log_level="info")
