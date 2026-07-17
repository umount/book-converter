import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

// --- Типы, зеркалящие DTO из src-tauri/src/commands.rs ---
type BookSummary = { title: string; author: string; total_chapters: number };
type Progress = {
  done: number;
  total: number;
  failed: number;
  running: boolean;
  status_line: string;
};
type Term = {
  source: string;
  target: string;
  kind: string;
  frequency: number;
  pinned: boolean;
};

export default function App() {
  const [book, setBook] = useState<BookSummary | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [error, setError] = useState<string | null>(null);

  // Прогресс приходит событиями из Rust-ядра.
  useEffect(() => {
    const unlisten = listen<Progress>("progress", (e) => setProgress(e.payload));
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  async function selectBook() {
    setError(null);
    const path = await open({
      multiple: false,
      filters: [{ name: "Текст", extensions: ["txt"] }],
    });
    if (typeof path !== "string") return;
    try {
      const summary = await invoke<BookSummary>("parse_book", { path });
      setBook(summary);
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleRun() {
    try {
      if (progress?.running) await invoke("pause_translation");
      else await invoke("start_translation");
    } catch (e) {
      setError(String(e));
    }
  }

  async function refreshGlossary() {
    try {
      setGlossary(await invoke<Term[]>("get_glossary"));
    } catch (e) {
      setError(String(e));
    }
  }

  async function exportBook() {
    try {
      const files = await invoke<string[]>("export_book", {
        outDir: "./output",
        formats: ["txt", "epub"],
      });
      alert("Готово:\n" + files.join("\n"));
    } catch (e) {
      setError(String(e));
    }
  }

  const pct =
    progress && progress.total > 0
      ? Math.round((progress.done / progress.total) * 100)
      : 0;

  return (
    <main className="app">
      <h1>book-converter</h1>
      <p className="subtitle">Перевод книги: китайский → русский (DeepSeek)</p>

      {error && <div className="error">{error}</div>}

      <section className="card">
        <button onClick={selectBook}>Выбрать книгу (.txt)</button>
        {book && (
          <div className="book-info">
            <strong>{book.title}</strong> — {book.author}
            <br />
            Глав: {book.total_chapters}
          </div>
        )}
      </section>

      <section className="card">
        <div className="row">
          <button onClick={toggleRun} disabled={!book}>
            {progress?.running ? "Пауза" : "Старт перевода"}
          </button>
          <button onClick={exportBook} disabled={!progress || progress.done === 0}>
            Экспорт (TXT + EPUB)
          </button>
        </div>
        {progress && (
          <div className="progress">
            <div className="bar">
              <div className="bar-fill" style={{ width: `${pct}%` }} />
            </div>
            <div className="progress-text">
              {progress.done}/{progress.total} ({pct}%)
              {progress.failed > 0 && ` · ошибок: ${progress.failed}`}
              {progress.status_line && ` · ${progress.status_line}`}
            </div>
          </div>
        )}
      </section>

      <section className="card">
        <div className="row">
          <h2>Глоссарий</h2>
          <button onClick={refreshGlossary}>Обновить</button>
        </div>
        <table>
          <thead>
            <tr>
              <th>Оригинал</th>
              <th>Перевод</th>
              <th>Тип</th>
              <th>×</th>
              <th>📌</th>
            </tr>
          </thead>
          <tbody>
            {glossary.map((t) => (
              <tr key={t.source}>
                <td>{t.source}</td>
                <td>{t.target}</td>
                <td>{t.kind}</td>
                <td>{t.frequency}</td>
                <td>{t.pinned ? "📌" : ""}</td>
              </tr>
            ))}
            {glossary.length === 0 && (
              <tr>
                <td colSpan={5} className="empty">
                  пусто — глоссарий наполняется по ходу перевода
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </section>
    </main>
  );
}
