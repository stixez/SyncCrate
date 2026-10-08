"""Render the Windows installer artwork into synccrate/src-tauri/windows/.

NSIS (.exe): sidebar.bmp (welcome/finish pages, 164x314) and header.bmp
(other pages, 150x57). WiX (.msi): wix-dialog.bmp (welcome/finish, 493x312)
and wix-banner.bmp (other pages, 493x58). Both installers draw their own
black text over the white parts, so the art stays dark only where no text
goes: the NSIS sidebar and the left strip of the WiX dialog.

Usage: python scripts/installer_art.py   (needs Edge or Chrome, and Pillow)
"""
import pathlib
import subprocess
import sys
import tempfile

from PIL import Image

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "synccrate" / "src-tauri" / "windows"
ICON = (ROOT / "synccrate" / "src-tauri" / "icons" / "icon.svg").as_uri()

BROWSERS = [
    r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    r"C:\Program Files\Google\Chrome\Application\chrome.exe",
]

FONTS = '<link href="https://fonts.googleapis.com/css2?family=Chakra+Petch:wght@600;700&family=Inter:wght@500&family=JetBrains+Mono:wght@500&display=swap" rel="stylesheet">'
BASE_CSS = """
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body { width: 100%; height: 100%; overflow: hidden; }
.word { font: 700 22px 'Chakra Petch', sans-serif; letter-spacing: .02em; }
.word b { color: #4cf0b2; font-weight: 700; }
.mono { font: 500 9px 'JetBrains Mono', monospace; letter-spacing: .14em; text-transform: uppercase; }
"""

# The dark HUD panel used on the sidebar and the WiX dialog's left strip.
PANEL_CSS = """
.panel { position: absolute; inset: 0; background: #07090b; color: #e8eeeb; overflow: hidden; }
.panel::before { content: ""; position: absolute; inset: 0;
  background-image: linear-gradient(rgba(76,240,178,.06) 1px, transparent 1px), linear-gradient(90deg, rgba(76,240,178,.06) 1px, transparent 1px);
  background-size: 18px 18px; }
.panel::after { content: ""; position: absolute; left: -40px; right: -40px; bottom: -60px; height: 180px;
  background: radial-gradient(closest-side, rgba(76,240,178,.22), transparent); }
.br { position: absolute; width: 14px; height: 14px; border-color: #4cf0b2; border-style: solid; }
.br.tl { top: 10px; left: 10px; border-width: 2px 0 0 2px; }
.br.bR { bottom: 10px; right: 10px; border-width: 0 2px 2px 0; }
.inner { position: relative; z-index: 1; height: 100%; display: flex; flex-direction: column; align-items: center; text-align: center; }
.tag { font: 500 11.5px/1.45 Inter, sans-serif; color: #9aa8a1; }
.tag em { font-style: normal; color: #4cf0b2; }
.foot { margin-top: auto; color: #5d6b64; }
"""


def panel(icon_px: int, pad_top: int) -> str:
    return f"""<div class="panel"><span class="br tl"></span><span class="br bR"></span>
<div class="inner" style="padding: {pad_top}px 14px 22px">
  <img src="{ICON}" width="{icon_px}" height="{icon_px}" alt="">
  <div class="word" style="margin-top: 16px">Sync<b>Crate</b></div>
  <div class="tag" style="margin-top: 10px">Same mods.<br><em>Every PC.</em></div>
  <div class="mono foot">LAN &middot; Join codes</div>
</div></div>"""


ART = {
    "sidebar.bmp": (164, 314, panel(76, 54)),
    "header.bmp": (150, 57, f"""<div style="position:absolute;inset:0;background:#fff;display:flex;align-items:center;justify-content:flex-end;gap:8px;padding-right:8px">
  <span class="word" style="font-size:17px;color:#07090b">Sync<b style="color:#12a874">Crate</b></span>
  <img src="{ICON}" width="34" height="34" alt=""></div>"""),
    "wix-dialog.bmp": (493, 312, f"""<div style="position:absolute;inset:0;background:#fff"></div>
<div style="position:absolute;left:0;top:0;width:164px;height:312px">{panel(72, 52)}</div>"""),
    "wix-banner.bmp": (493, 58, f"""<div style="position:absolute;inset:0;background:#fff;display:flex;align-items:center;justify-content:flex-end;padding-right:12px">
  <img src="{ICON}" width="38" height="38" alt=""></div>
<div style="position:absolute;left:0;right:0;bottom:0;height:2px;background:#4cf0b2"></div>"""),
}


def browser() -> str:
    for b in BROWSERS:
        if pathlib.Path(b).exists():
            return b
    sys.exit("Edge or Chrome not found")


def main() -> None:
    exe = browser()
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        for name, (w, h, body) in ART.items():
            src = tmp / f"{name}.html"
            src.write_text(f"<!doctype html><html><head><meta charset='utf-8'>{FONTS}<style>{BASE_CSS}{PANEL_CSS}</style></head><body>{body}</body></html>", encoding="utf-8")
            png = tmp / f"{name}.png"
            subprocess.run(
                [exe, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--allow-file-access-from-files",
                 "--force-device-scale-factor=1", f"--window-size={w},{h}", "--virtual-time-budget=8000", f"--screenshot={png}", src.as_uri()],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120,
            )
            img = Image.open(png).convert("RGB")
            if img.size != (w, h):
                img = img.crop((0, 0, w, h))
            # Installers want plain 24-bit BMPs.
            img.save(OUT / name, format="BMP")
            print(f"{name}: {w}x{h}")


if __name__ == "__main__":
    main()
