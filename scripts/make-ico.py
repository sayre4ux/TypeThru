"""Packs PNG files into a Windows .ico (PNG-compressed entries, Windows Vista and later).
Usage: python3 scripts/make-ico.py OUT.ico IN-16.png IN-32.png ... (square PNGs up to 256 px)."""
import struct
import sys

out, pngs = sys.argv[1], sys.argv[2:]
images = []
for path in pngs:
    data = open(path, 'rb').read()
    width, height = struct.unpack('>II', data[16:24])
    images.append((width, height, data))
header = struct.pack('<HHH', 0, 1, len(images))
offset = 6 + 16 * len(images)
entries, blobs = b'', b''
for width, height, data in images:
    entries += struct.pack('<BBBBHHII', width % 256, height % 256, 0, 0, 1, 32, len(data), offset)
    offset += len(data)
    blobs += data
open(out, 'wb').write(header + entries + blobs)
