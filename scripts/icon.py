"""Build crates/app/assets/app.ico and app.res from build/icon.svg (40px and up)
and build/icon-small.svg (16-32px).

Each size is rasterised separately by headless Chrome (a small icon is not a
shrunk big one), Pillow packs the .ico, rc.exe compiles the resource with the
manifest. app.res is committed, so normal builds need no Windows SDK.
Run after changing the SVG or the manifest:  python scripts/icon.py
"""
import glob
import io
import struct
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates" / "app" / "assets"
SVG = ROOT / "build" / "icon.svg"
SMALL = ROOT / "build" / "icon-small.svg"
CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
SIZES = [16, 20, 24, 32, 40, 48, 64, 256]


def render() -> dict[int, Image.Image]:
    # Every size side by side on one page, one screenshot, then cropped apart.
    x, cells = 0, []
    for s in SIZES:
        svg = SMALL if s <= 32 else SVG
        cells.append(f'<img src="{svg.as_uri()}" style="position:absolute;left:{x}px;top:0;width:{s}px;height:{s}px">')
        x += s + 8
    with tempfile.TemporaryDirectory() as tmp:
        page, shot = Path(tmp) / "sheet.html", Path(tmp) / "sheet.png"
        page.write_text("<body style='margin:0'>" + "".join(cells) + "</body>", encoding="utf-8")
        subprocess.run([CHROME, "--headless=new", "--hide-scrollbars", "--force-device-scale-factor=1",
                        "--default-background-color=00000000", f"--window-size={x},{max(SIZES)}",
                        f"--screenshot={shot}", page.as_uri()], check=True, capture_output=True)
        sheet = Image.open(shot).convert("RGBA")
    out, x = {}, 0
    for s in SIZES:
        out[s] = sheet.crop((x, 0, x + s, s))
        x += s + 8
    return out


def write_ico(images: dict[int, Image.Image], path: Path):
    # PNG data for every size; every Windows since Vista reads it.
    blobs = []
    for s, im in images.items():
        b = io.BytesIO()
        im.save(b, "PNG")
        blobs.append((s, b.getvalue()))
    head = struct.pack("<HHH", 0, 1, len(blobs))
    offset, dirs = 6 + 16 * len(blobs), b""
    for s, data in blobs:
        dirs += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    path.write_bytes(head + dirs + b"".join(d for _, d in blobs))


def main():
    write_ico(render(), ASSETS / "app.ico")
    rc = ASSETS / "app.rc"
    rc.write_text('1 ICON "app.ico"\n1 24 "app.manifest"\n', encoding="ascii")
    tool = sorted(glob.glob(r"C:\Program Files (x86)\Windows Kits\10\bin\*\x64\rc.exe"))[-1]
    subprocess.run([tool, "/nologo", "/fo", str(ASSETS / "app.res"), str(rc)], check=True, cwd=ASSETS)
    rc.unlink()
    print("wrote", ASSETS / "app.ico", ASSETS / "app.res")


if __name__ == "__main__":
    main()
