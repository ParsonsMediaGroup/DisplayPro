"""Reproducible high-contrast app/tray icon with hand-aligned small sizes."""
from pathlib import Path
from PIL import Image, ImageDraw

root = Path(__file__).resolve().parents[1]
output = root / 'src-tauri' / 'icons'
output.mkdir(parents=True, exist_ok=True)

def icon(size):
    scale = size / 32
    def box(values):
        return tuple(round(v * scale) for v in values)
    image = Image.new('RGBA', (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle(box((0, 0, 31, 31)), radius=max(2, round(6*scale)), fill='#080C12')
    draw.rounded_rectangle(box((3, 5, 28, 23)), radius=max(1, round(2*scale)), fill='#FFFFFF')
    draw.rectangle(box((5, 7, 26, 20)), fill='#080C12')
    draw.rectangle(box((14, 23, 17, 26)), fill='#FFFFFF')
    draw.rounded_rectangle(box((10, 26, 21, 28)), radius=max(1, round(scale)), fill='#FFFFFF')
    draw.rectangle(box((8, 10, 11, 17)), fill='#83FAD0')
    draw.rectangle(box((14, 10, 23, 12)), fill='#83FAD0')
    draw.rectangle(box((14, 15, 20, 17)), fill='#83FAD0')
    return image

sizes = [16, 20, 24, 32, 48, 64, 128, 256]
images = [icon(size) for size in sizes]
images[-1].save(output / 'icon.ico', format='ICO', sizes=[(s, s) for s in sizes], append_images=images[:-1])
images[-1].save(output / 'icon.png')
images[0].save(output / 'tray-16.png')
sheet = Image.new('RGB', (640, 200), '#111827')
draw = ImageDraw.Draw(sheet)
draw.rectangle((320, 0, 639, 199), fill='#F4F6F8')
for offset in [0, 320]:
    x = offset+20
    for size in [16, 24, 32, 48, 64]:
        im = icon(size)
        sheet.paste(im, (x, 70-size//2), im)
        x += size+16
(root/'verification').mkdir(exist_ok=True)
sheet.save(root/'verification'/'icon-contrast.png')
print(output / 'icon.ico')
