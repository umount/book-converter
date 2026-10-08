# Local book narration

Open a book and select **Narration**. Download the Qwen3-TTS 0.6B CustomVoice model
once (approximately 2.5 GB), then select the original or translation, a chapter,
chapter range or the whole book, a voice and a device. Select **Create MP3**.
The model download can be paused and resumed. Every file is checked against a
pinned size and SHA-256 hash before use.

To prepare the model before starting a job, select a compute device and press
**Load model**. Wait for **Model in memory → Ready**, then create MP3 jobs as needed.
One loaded model serves all chapters and subsequent jobs, including different voices
and books. **Create MP3** also loads it automatically if necessary.

The model stays in RAM or GPU memory after a job finishes and while paused. Use
**Unload model** when no narration is running to release that memory; downloaded
weights and audio are retained. Closing the application also unloads it. A different
explicit device, a worker failure or an application restart requires a new load.

The packaged application includes its speech engine. End users do not need Python,
FFmpeg, an API key or a paid speech service. Internet access is needed to download
the model; synthesis runs locally with offline mode enabled. Book text is not sent
to a speech provider. Translation remains a separate feature with its configured
provider.

## Languages, voices and output

The engine supports Russian, English, Chinese, Japanese, Korean, German, French,
Spanish, Portuguese and Italian. Narrating the original uses the project's source
language; narrating the translation uses its target language. These languages are
fixed when the project is created. If the source language was omitted or selected
incorrectly, import the book as a new project with the correct language. Books in an
unsupported source language can still be narrated after translation into a supported
target language.

Nine preset voices are available: Ryan, Aiden, Serena, Vivian, Uncle_Fu, Dylan, Eric,
Ono_Anna and Sohee. Pronunciation and accent vary by voice and language. Start with
one chapter to check the result. Voice cloning and voice design are not included.

Each chapter containing nonempty text or caption blocks produces one mono MP3 at
**128 kbps**, with a sample rate of 24 kHz. Chapters containing only a title or images
are skipped. The chapter title and text/caption blocks are spoken in source order;
illustrations are skipped. Text is split at sentence or whitespace boundaries into
fragments of at most 400 Unicode characters. There is a short pause between fragments.
A continuous MP3 encoder joins their audio within each chapter.

For a translation, all nonempty text blocks and the title in each narrated chapter
must have a translation. Missing text is reported instead of mixing languages or
silently omitting paragraphs. Source editing and translation quality should be
reviewed before narration.

After completion, **Save MP3 chapters** writes a new `audiobook-<first-8-job-id-characters>`
folder to the chosen directory, containing `00001.mp3`, `00002.mp3`, etc. and a
`book.m3u8` playlist with chapter names. Numbering starts at 1 within the audio job,
including when only a range of book chapters was selected. Existing folders are
never overwritten; choose another parent directory to export the same job again.
Whole-book export requires a completed job. Open the exported MP3s or playlist in an audio
player; the Narration tab provides an **Open folder** button.

## Listen before a chapter finishes

Press **Listen** in an audio job in **Jobs** or **Narration**. As soon as the first
fragment is saved, an inline player can play it without downloading a file or waiting
for the whole chapter. Playback works while generating, paused or after completion.
The player has its own play/pause and seek controls, independent of the narration job.

The sample contains up to 30 seconds from the latest saved fragment, or the end of
the latest completed chapter if its fragment checkpoints have already been removed.
Press **Latest fragment** to refresh it as narration advances. The currently playing
sample stays unchanged when progress updates. Sample preparation reads saved audio;
it does not load the speech model or synthesize text again.

**Save fragment as MP3** optionally writes that exact sample to a selected folder as
`fragment-<job-id-prefix>-<preview-id-prefix>.mp3`. Samples are kept with the audio job
and are removed when its project is deleted. Existing exported files are not overwritten.

## Devices and recovery

The default runtime uses CPU. CPU generation can take considerably longer than
the resulting audio; begin with one chapter. **Automatic** reuses the loaded model;
when loading a new model it uses CUDA if the installed runtime supports it and a
compatible NVIDIA GPU is available. Selecting
CUDA explicitly reports an error when it is unavailable. CUDA builds require the
build option described below. GPU memory needs depend on the device and input;
there is no fixed performance or memory guarantee.

Audio jobs and their progress appear in both **Narration** and the shared **Jobs**
panel, including when the Narration tab is closed. Both views provide pause and
resume controls; the Jobs row also opens Narration for export. Clearing finished
Jobs entries hides them in that panel while preserving audio and the Narration history.
Only one narration job runs at a time across projects. **Pause** finishes and saves
the current fragment, then pauses the job while keeping the model loaded. The UI
shows **Pausing…** during that interval. If paused during initial model loading, the
load finishes before the job settles as paused.
Completed fragments and chapters are checked and reused on **Resume saved text**.
An interrupted fragment is regenerated. After an application restart, interrupted
jobs require an explicit resume; opening a book never starts narration.

Each job stores a snapshot of the selected text, voice, language and device.
Resuming uses that snapshot. Create a new job to use subsequent text edits or
different narration settings. Translation can continue independently without
changing an existing audio job's input.

Model files live under the application's `tts-models/` directory. Audio jobs live
under `audiobooks/<project-id>/<job-id>/`, outside the project database. These
caches and audio files are not included in portable `.bcproj` archives. Deleting
a project stops its active audio worker and removes its audio cache, but leaves explicitly
exported MP3 folders and the shared model download intact.

## Troubleshooting

- **Speech engine missing:** in development, run `npm run tts:prepare` with a supported
  Python version. For an installed application, reinstall a package that includes
  the speech runtime; downloading model weights alone does not supply the engine.
- **Download failed:** restore the connection and start the download again. Completed
  files and valid partial downloads are reused; model verification must finish before
  generation can start.
- **Incomplete translation:** translate or fill in every missing title and text block
  in the selected chapters, select a completed range, or choose the original.
- **CUDA unavailable or insufficient memory:** free memory or create a new CPU job.
  Changing the device selector does not change the settings of a saved job.
- **Storage error:** check available disk space and write access, then resume. Select
  a different export directory if the destination folder already exists.
- **Loading is slow:** preparation includes verifying weights and loading them into
  memory. Leave the ready model loaded to avoid repeating that work between jobs.
  This removes repeated startup time; CPU speech generation itself can still be slow.

## Development and packaging

The build machine needs Python **3.11 or 3.12**, including `venv` and `pip`, plus
the normal Rust/Node/Tauri prerequisites. Use `BOOK_TTS_PYTHON` to select a specific
Python executable. The build script creates an isolated `.cache/tts-venv`; it does
not install packages into the system Python. The Linux `make deps-linux` helper
installs Tauri libraries only; install a supported Python and its `venv`/`pip` support
separately. For example, select an installed Python 3.12 with
`BOOK_TTS_PYTHON=python3.12 npm run tts:prepare`.

```bash
npm run tts:prepare  # CPU development runtime; needed once to use Narration in dev
npm run test:tts     # Real MP3 encoding/decoding and fragment recovery tests
npm run tts:bundle  # Freeze and validate the engine for this OS/architecture
```

The normal Tauri build hook runs `tts:bundle` automatically and includes
`src-tauri/tts-runtime/` as a resource. Model weights are downloaded separately by
the application. Allow several GB of free space for dependencies and build copies
in addition to the model and generated audio. The first build downloads large
dependencies; subsequent builds reuse the matching runtime pack.

Debug builds prefer the prepared development virtual environment and current worker
source, so an older frozen pack cannot hide Python changes. Without that environment,
they fall back to the packaged runtime. Release builds use the frozen `tts-runtime/`
pack; rebuild it with `npm run tts:bundle` after changing the worker.

For an NVIDIA runtime, preserve the CUDA option through the Tauri build hook:

```bash
BOOK_TTS_CUDA=1 npm run tauri -- build
```

```powershell
$env:BOOK_TTS_CUDA = "1"
npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis
```

The CUDA runtime uses PyTorch's CUDA 12.8 wheels. Build on the target OS and
architecture; cross-compiling Rust alone does not produce a compatible Python
runtime. Runtime preparation and the frozen executable both run a dependency and
MP3 encoder self-check. The frozen self-check does not load model weights or prove
speech quality. A real model smoke test must be run separately.

For isolated manual testing, the examples use explicitly supplied directories:

```bash
npm run tts:prepare
mkdir -p /tmp/tts-export
cargo run --manifest-path src-tauri/Cargo.toml --example prepare_narration -- \
  /tmp/tts-models
cargo run --manifest-path src-tauri/Cargo.toml --example narrate_sample -- \
  /tmp/tts-app /tmp/tts-models /absolute/path/to/book-converter/src-tauri \
  /absolute/path/to/short-russian-sample.txt /tmp/tts-export
```

`prepare_narration` downloads and verifies the model; an optional second path also
materializes a model directory for inspection. `narrate_sample` imports
the supplied text into a new isolated project, narrates the original with Ryan on
CPU, checks completion and exports the MP3 and playlist using the application
services. It runs real synthesis and is not part of the fast automated suite.
To resume that saved job, replace `SOURCE_TXT EXPORT_DIR` with
`--resume PROJECT_ID JOB_ID EXPORT_DIR`; completed audio checkpoints are reused.

Upstream references: [Qwen3-TTS](https://github.com/QwenLM/Qwen3-TTS),
[0.6B CustomVoice model](https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-CustomVoice).
Licenses and redistribution notes are in [TTS notices](../third-party/TTS-NOTICES.md).
