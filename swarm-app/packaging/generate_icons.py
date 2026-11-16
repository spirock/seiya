"""Regenerate checked-in icons on macOS using sips; packaging needs no converter."""
import hashlib
import json
from pathlib import Path
import struct
import subprocess

ROOT = Path(__file__).resolve().parents[2]
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)


def generate():
    source = ROOT / 'docs/assets/seiya-icon-proposal.png'
    output = Path(__file__).resolve().parent / 'icons'
    output.mkdir(exist_ok=True)
    images = {}
    for size in SIZES:
        dest = output / f'seiya-{size}.png'
        subprocess.run(['/usr/bin/sips', '-s', 'format', 'png', '-z', str(size), str(size),
                        str(source), '--out', str(dest)], check=True, capture_output=True)
        images[size] = dest.read_bytes()
        if images[size][:8] != b'\x89PNG\r\n\x1a\n' or struct.unpack('>II', images[size][16:24]) != (size, size):
            raise SystemExit('Unexpected resized PNG dimensions.')
    # PNG-compressed icon entries supported by Windows Vista+ and macOS 10.7+.
    sizes = tuple(s for s in SIZES if s <= 256)
    offset = 6 + 16 * len(sizes)
    directory = bytearray(struct.pack('<HHH', 0, 1, len(sizes)))
    for size in sizes:
        image = images[size]
        directory.extend(struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(image), offset))
        offset += len(image)
    (output / 'seiya.ico').write_bytes(directory + b''.join(images[s] for s in sizes))
    chunks = []
    for kind, size in [(b'icp4', 16), (b'icp5', 32), (b'icp6', 64), (b'ic07', 128),
                       (b'ic08', 256), (b'ic09', 512), (b'ic10', 1024)]:
        chunks.append(kind + struct.pack('>I', 8 + len(images[size])) + images[size])
    body = b''.join(chunks)
    (output / 'seiya.icns').write_bytes(b'icns' + struct.pack('>I', 8 + len(body)) + body)
    names = [f'seiya-{s}.png' for s in SIZES] + ['seiya.ico', 'seiya.icns']
    manifest = {'source': 'docs/assets/seiya-icon-proposal.png',
                'source_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
                'files': {name: hashlib.sha256((output / name).read_bytes()).hexdigest() for name in names}}
    (output / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    generate()
