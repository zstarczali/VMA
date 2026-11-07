"""Builds vma.ico from the two SVG icons.

Needs Pillow and Microsoft Edge (used to render the SVGs). Run it after changing an SVG:
    python assets/make_icon.py
"""

import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parent
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
RENDER_SIZE = 1024
SMALL_SIZES = [16, 24, 32]
LARGE_SIZES = [48, 64, 128, 256]


def render(svg: Path, tmp: Path) -> Image.Image:
    """Renders an SVG with a transparent background at RENDER_SIZE."""
    page = tmp / f"{svg.stem}.html"
    page.write_text(
        f"<!doctype html><html style='overflow:hidden'><body style='margin:0;background:transparent'>"
        f"<img src='{svg.as_uri()}' width='{RENDER_SIZE}' height='{RENDER_SIZE}' style='display:block'>",
        encoding="utf-8",
    )
    png = tmp / f"{svg.stem}.png"
    subprocess.run(
        [
            EDGE,
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--allow-file-access-from-files",
            "--default-background-color=00000000",
            f"--user-data-dir={tmp / 'profile'}",
            f"--window-size={RENDER_SIZE},{RENDER_SIZE}",
            f"--screenshot={png}",
            page.as_uri(),
        ],
        check=True,
        timeout=60,
    )
    return Image.open(png).convert("RGBA").crop((0, 0, RENDER_SIZE, RENDER_SIZE))


def main() -> None:
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        small = render(ASSETS / "vma-icon-small.svg", tmp)
        large = render(ASSETS / "vma-icon.svg", tmp)
        # BOX keeps the 16 unit pixel grid of the small icon sharp
        images = [small.resize((s, s), Image.BOX) for s in SMALL_SIZES]
        images += [large.resize((s, s), Image.LANCZOS) for s in LARGE_SIZES]
        images[-1].save(
            ASSETS / "vma.ico",
            format="ICO",
            sizes=[im.size for im in images],
            append_images=images[:-1],
        )


if __name__ == "__main__":
    main()
