# Synthetic refactoring fixtures

These fixtures are original generated test data, dedicated to the public domain
under CC0-1.0. No source from `samples/` is included.

Run `python3 scripts/generate-fixtures.py` from any directory to regenerate them.
ZIP timestamps and member ordering are fixed. `manifest.json` records hashes and
expected structure. Comic pages intentionally reuse identical image bytes while
retaining six separate page occurrences. They test ordering and identity, not OCR.
The EPUB contains text-only, image-only and mixed chapters with repeated images.
The TXT checks Chinese text with numbered-dot headings; FB2 supplies reference text.
