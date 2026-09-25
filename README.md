# Book Converter

Desktop application for translating books and manga with a shared glossary,
editable results and resumable background jobs. Built with Rust, Tauri 2 and
React/TypeScript. The interface supports English, Russian and Chinese.

## Books

Import TXT, FB2, EPUB, PDF, or supported books inside ZIP archives. The reader keeps
text and illustrations as separate, ordered blocks. EPUB/FB2 illustrations and
available covers are retained; PDF extraction depends on the source document.

Translate a selected batch of chapters, optionally extracting terminology first.
Book and chapter instructions, matching glossary terms and preceding story context
are included in translation requests. Review and edit translations, search and
replace text, import an aligned reference translation, or use the book assistant
for proposed changes. Title, author, annotation and cover are managed in Overview.

Export books as TXT, FB2 in a `.fb2.zip` archive, EPUB or PDF. Portable `.bcproj`
archives contain project data and assets so work can be continued elsewhere.

## Manga

Import CBZ/ZIP or an image folder. Browse volumes and naturally ordered pages with
thumbnails, zoom and panning. The processing pipeline performs cloud recognition
and translation, followed by local text masks, inpainting and lettering.

Processing requires configured API access, installed model weights and the native
runtime. Regions can be moved/resized and locally rebuilt using existing translated
text. Original images remain unchanged. Portable `.bcproj` export is available;
rendered manga export to CBZ/EPUB is not currently implemented. RAR/CBR must be
extracted to a folder before import.

See [Books](docs/BOOKS.md) and [Manga](docs/MANGA.md) for the workflows and limitations.

## Quick start for development

Install Node.js/npm, stable Rust/Cargo and the native dependencies for Tauri.
On Debian/Ubuntu:

```bash
make deps-linux
make install
make dev
```

`make dev` downloads pdfium if needed and starts the desktop application with Vite.
Tauri dev/build prepares the native manga resources automatically. To prepare them
separately:

```bash
npm run manga:prepare
```

Model weights are downloaded separately in **Settings → Manga models**. Configure
provider access in Settings, create a project, choose its source and target languages,
and start an explicit chapter/page batch. Opening a project does not start processing.

```bash
make binary   # Release application without an installer
make bundle   # Platform-specific installers
```

Release builds prepare the manga runtime automatically. The runtime directory must
remain available alongside an unbundled executable or in the application's resources.
See [Development](docs/DEVELOPMENT.md) for checks, packaging and test commands, and
[Settings](docs/SETTINGS.md) for providers and local data locations.

## Windows: запуск и сборка

Поддерживается Windows x64. Для сборки из исходников установите:

- Node.js 22 с npm.
- Rust через rustup с toolchain `stable-x86_64-pc-windows-msvc`.
- Visual Studio 2022 Build Tools: **Desktop development with C++**, MSVC и Windows SDK.
- Microsoft Edge WebView2 Runtime, если его ещё нет в системе.

Откройте PowerShell в каталоге проекта. Команды выполняются без `make`:

```powershell
npm ci
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/prepare-windows.ps1
npm run tauri -- dev
```

Первый запуск требует интернет: подготовка скачивает ONNX Runtime и собирает
`manga-inference.exe`, затем открывает приложение. Последующие запуски используют
кэш скачивания и инкрементальную сборку Rust. `npm run dev` запускает только веб-интерфейс,
без нативных функций приложения.

Для перевода манги настройте API в **Настройках** и скачайте обе модели обработки.
Модели и исполняемый компонент — разные вещи: скачивание моделей не исправляет
отсутствующий `manga-runtime`. При запуске по инструкции компонент готовится автоматически.

### Обычный установщик

После подготовки зависимостей и pdfium команда выше заменяется на:

```powershell
npm run tauri -- build --target x86_64-pc-windows-msvc --bundles nsis
```

Установщик находится в
`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`.
Передавайте пользователям установщик: отдельный `book-converter.exe` без ресурсов
недостаточен для обработки манги.

### Диагностическая сборка для тестировщиков

```powershell
npm run build:debug:windows
```

Скрипт сам выполняет `npm ci`, подготовку pdfium и сборку установщика с компонентом
манги, отладочной информацией и полным логированием по умолчанию.
Результат: `src-tauri/target/x86_64-pc-windows-msvc/debug/bundle/nsis/*-setup.exe`.
Тестировщикам не нужны Node.js, Rust или Python — только установщик, API-настройки
и скачанные через приложение модели.

После воспроизведения ошибки: **Настройки → Диагностика → Сохранить логи диагностики**.
Галочка **Полное диагностическое логирование** применяется сразу и запоминается;
API-ключи, тексты книг и изображения в логи не записываются. Ранее сохранённое
отключение логирования сохраняется и после установки диагностической сборки.

Также предусмотрен ручной workflow **Windows diagnostic installer** в GitHub Actions.
После его запуска установщик доступен в артефакте `book-converter-windows-debug`.

## Documentation

- [Architecture and interaction diagrams](docs/ARCHITECTURE.md)
- [Book workflow](docs/BOOKS.md)
- [Manga workflow](docs/MANGA.md)
- [Native manga runtime](docs/MANGA_RUNTIME.md)
- [Settings and provider profiles](docs/SETTINGS.md)
- [Book assistant](docs/ASSISTANT.md)
- [Development and verification](docs/DEVELOPMENT.md)
- [Documentation index](docs/README.md)

## License

Book Converter is free and open-source software under the [MIT License](LICENSE.md).
Third-party libraries, fonts and model weights retain their own licenses; bundled
notices are in [third-party](third-party/) and the prepared runtime resources.
