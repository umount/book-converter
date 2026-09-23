#!/usr/bin/env python3
"""Generate deterministic, original fixtures without copying sample books."""
import hashlib
import json
from pathlib import Path
import struct
import zlib
from zipfile import ZipFile, ZipInfo, ZIP_STORED

ROOT = Path(__file__).resolve().parents[1] / 'tests' / 'fixtures'
ROOT.mkdir(parents=True, exist_ok=True)


def png():
    def chunk(kind, data):
        return struct.pack('!I', len(data)) + kind + data + struct.pack('!I', zlib.crc32(kind + data))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('!2I5B', 32, 32, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress((b'\0' + b'\xff\xff\xff' * 32) * 32)) + chunk(b'IEND', b''))


def archive(name, entries):
    with ZipFile(ROOT / name, 'w') as output:
        for path, data in entries:
            entry = ZipInfo(path, (2020, 1, 1, 0, 0, 0))
            entry.compress_type = ZIP_STORED
            entry.external_attr = 0o100644 << 16
            output.writestr(entry, data)


(ROOT / 'numbered-chinese.txt').write_text('测试故事\n作者：测试作者\n\n1. 开始\n这是第一章。\n\n2. 旅程\n这是第二章。\n', encoding='utf-8')
(ROOT / 'reference.fb2').write_text('''<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><description><title-info>
<book-title>Synthetic reference</book-title><lang>en</lang></title-info></description>
<body><section><title><p>1. Beginning</p></title><p>This is chapter one.</p></section>
<section><title><p>2. Journey</p></title><p>This is chapter two.</p></section></body></FictionBook>
''', encoding='utf-8')
image = png()
pages = [('<p>Original text.</p>'), ('<img src="plate.png" alt="Illustration"/>'),
         ('<p>Before.</p><img src="plate.png"/><p>Between.</p><img src="plate.png"/><p>After.</p>')]
archive('structural.epub', [
    ('mimetype', 'application/epub+zip'),
    ('META-INF/container.xml', '<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container" version="1.0"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'),
    ('OEBPS/content.opf', '<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">synthetic-fixture</dc:identifier><dc:title>Structural fixture</dc:title><dc:language>en</dc:language><meta property="dcterms:modified">2020-01-01T00:00:00Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="img" href="plate.png" media-type="image/png"/>' + ''.join(f'<item id="p{i}" href="p{i}.xhtml" media-type="application/xhtml+xml"/>' for i in range(3)) + '</manifest><spine>' + ''.join(f'<itemref idref="p{i}"/>' for i in range(3)) + '</spine></package>'),
    ('OEBPS/nav.xhtml', '<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Contents</title></head><body><nav epub:type="toc"><ol>' + ''.join(f'<li><a href="p{i}.xhtml">Chapter {i+1}</a></li>' for i in range(3)) + '</ol></nav></body></html>'),
    ('OEBPS/plate.png', image),
] + [(f'OEBPS/p{i}.xhtml', f'<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Chapter {i+1}</title></head><body>{body}</body></html>') for i, body in enumerate(pages)])
archive('two-volumes.cbz', [(f'Volume {v}/page {p}.png', image) for v in (2, 1) for p in (10, 2, 1)])
files = ['numbered-chinese.txt', 'reference.fb2', 'structural.epub', 'two-volumes.cbz']
(ROOT / 'manifest.json').write_text(json.dumps({
    'license': 'CC0-1.0',
    'expectations': {'book_chapters': 3, 'book_block_kinds': ['text', 'image', 'mixed'], 'image_occurrences': 3, 'unique_images': 1, 'comic_volumes': 2, 'comic_pages': 6, 'natural_page_order': [1, 2, 10]},
    'sha256': {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in files},
}, indent=2) + '\n')
