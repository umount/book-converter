import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";

// --- DTOs (mirror src-tauri/src/commands.rs) ---
type BookInfo = {
  title: string;
  author: string;
  total_chapters: number;
  format: string;
  encoding: string;
  needs_delimiter: boolean;
  missing: number;
  duplicates: number;
};
type RefInfo = { title: string; chapters: number; max_covered: number | null };
type Progress = {
  done: number;
  total: number;
  failed: number;
  pending: number;
  running: boolean;
};
type Term = {
  source: string;
  target: string;
  kind: string;
  frequency: number;
  pinned: boolean;
};
type BookDetails = {
  title: string;
  author: string;
  title_translated: string | null;
  summary: string | null;
  cover: string | null;
};

export default function App() {
  const [book, setBook] = useState<BookInfo | null>(null);
  const [ref, setRef] = useState<RefInfo | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [glossary, setGlossary] = useState<Term[]>([]);
  const [details, setDetails] = useState<BookDetails | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const [sample, setSample] = useState(30);
  const [limit, setLimit] = useState<number | "">("");
  const [continueMode, setContinueMode] = useState(false);

  const logRef = useRef<HTMLDivElement>(null);
  const [log, setLog] = useState<string[]>([]);
  const addLog = (m: string) => setLog((l) => [...l.slice(-200), m]);

  useEffect(() => {
    const unsubs = [
      listen<Progress>("progress", (e) => setProgress(e.payload)),
      listen("done", () => {
        addLog("✓ run finished");
        refreshProgress();
        refreshGlossary();
      }),
      listen<string>("job_error", (e) => setError(String(e.payload))),
    ];
    return () => {
      unsubs.forEach((u) => u.then((f) => f()));
    };
  }, []);

  useEffect(() => {
    logRef.current?.scrollTo(0, logRef.current.scrollHeight);
  }, [log]);

  async function call<T>(name: string, args?: Record<string, unknown>): Promise<T | undefined> {
    setError(null);
    try {
      return await invoke<T>(name, args);
    } catch (e) {
      setError(String(e));
      return undefined;
    }
  }

  async function selectSource() {
    const path = await open({ filters: [{ name: "Book", extensions: ["txt", "fb2"] }] });
    if (typeof path !== "string") return;
    setBusy("Loading book…");
    const info = await call<BookInfo>("load_source", { path });
    setBusy(null);
    if (info) {
      setBook(info);
      addLog(`Loaded ${info.total_chapters} chapters (${info.format}, ${info.encoding})`);
      refreshProgress();
      refreshDetails();
      translateTitle();
    }
  }

  async function selectReference() {
    const path = await open({ filters: [{ name: "Reference", extensions: ["fb2", "txt"] }] });
    if (typeof path !== "string") return;
    setBusy("Loading reference…");
    const info = await call<RefInfo>("load_reference", { path });
    setBusy(null);
    if (info) {
      setRef(info);
      addLog(`Reference: ${info.chapters} chapters, covers up to #${info.max_covered ?? "?"}`);
      refreshDetails();
    }
  }

  async function bootstrap() {
    setBusy(`Bootstrapping from ${sample} chapters…`);
    const n = await call<number>("bootstrap_glossary", { sample });
    setBusy(null);
    if (n !== undefined) {
      addLog(`Bootstrapped ${n} pinned terms from the reference`);
      refreshGlossary();
    }
  }

  async function start() {
    if (continueMode && ref) {
      setBusy("Importing reference chapters…");
      const n = await call<number>("use_reference_as_base", {});
      setBusy(null);
      if (n !== undefined) addLog(`Imported ${n} reference chapters as done`);
    }
    const lim = limit === "" ? null : Number(limit);
    await call("start_translation", { limit: lim });
    addLog(`Started translation${lim ? ` (next ${lim} chapters)` : ""}`);
  }

  async function pause() {
    await call("pause_translation", {});
    addLog("Pause requested (stops after current chapter)");
  }

  async function refreshProgress() {
    const p = await call<Progress>("get_progress", {});
    if (p) setProgress(p);
  }

  async function refreshGlossary() {
    const g = await call<Term[]>("get_glossary", {});
    if (g) setGlossary(g);
  }

  async function refreshDetails() {
    const d = await call<BookDetails>("get_book_details", {});
    if (d) setDetails(d);
  }

  async function translateTitle() {
    const t = await call<string>("translate_title", {});
    if (t) refreshDetails();
  }

  async function replaceCover() {
    const path = await open({
      filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png", "gif", "webp"] }],
    });
    if (typeof path !== "string") return;
    await call("set_cover", { path });
    refreshDetails();
  }

  async function saveSummary(text: string) {
    await call("set_summary", { summary: text });
  }

  async function pinTerm(t: Term, target: string) {
    await call("update_term", { term: { ...t, target, pinned: true } });
    refreshGlossary();
  }

  async function exportBook() {
    const outPath = await save({
      filters: [{ name: "Output", extensions: ["fb2", "epub", "txt", "zip"] }],
    });
    if (typeof outPath !== "string") return;
    setBusy("Exporting…");
    const p = await call<string>("export_book", { outPath });
    setBusy(null);
    if (p) addLog(`Exported → ${p}`);
  }

  const pct = progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0;

  return (
    <main className="app">
      <header>
        <h1>book-converter</h1>
        <span className="subtitle">Universal book translator · DeepSeek</span>
      </header>

      {error && <div className="error" onClick={() => setError(null)}>{error}</div>}
      {busy && <div className="busy">{busy}</div>}

      <div className="grid">
        {/* --- Source & reference --- */}
        <section className="card">
          <h2>1 · Source</h2>
          <button onClick={selectSource}>Choose book (.txt / .fb2)</button>
          {book && (
            <div className="info">
              <strong>{book.title || "(untitled)"}</strong> — {book.author || "?"}
              <br />
              {book.total_chapters} chapters · {book.format} · {book.encoding}
              {(book.missing > 0 || book.duplicates > 0) && (
                <div className="warn">
                  ⚠ {book.missing} missing, {book.duplicates} duplicate chapters
                </div>
              )}
              {book.needs_delimiter && <div className="warn">⚠ no chapter pattern detected</div>}
            </div>
          )}
        </section>

        <section className="card">
          <h2>2 · Reference (optional)</h2>
          <button onClick={selectReference} disabled={!book}>Choose reference translation</button>
          {ref && (
            <div className="info">
              <strong>{ref.title || "(untitled)"}</strong>
              <br />
              {ref.chapters} chapters · covers up to #{ref.max_covered ?? "?"}
              <div className="row" style={{ marginTop: 8 }}>
                <label>sample</label>
                <input
                  type="number"
                  min={1}
                  value={sample}
                  onChange={(e) => setSample(Number(e.target.value))}
                  style={{ width: 70 }}
                />
                <button onClick={bootstrap}>Bootstrap glossary</button>
              </div>
              <label className="check">
                <input
                  type="checkbox"
                  checked={continueMode}
                  onChange={(e) => setContinueMode(e.target.checked)}
                />
                Continue mode (keep professional chapters, translate the rest)
              </label>
            </div>
          )}
        </section>

        {/* --- Run --- */}
        <section className="card">
          <h2>3 · Translate</h2>
          <div className="row">
            <label>chapters</label>
            <input
              type="number"
              min={1}
              placeholder="all"
              value={limit}
              onChange={(e) => setLimit(e.target.value === "" ? "" : Number(e.target.value))}
              style={{ width: 80 }}
            />
            <button onClick={start} disabled={!book || progress?.running}>Start</button>
            <button onClick={pause} disabled={!progress?.running}>Pause</button>
          </div>
          {progress && (
            <div className="progress">
              <div className="bar">
                <div className="bar-fill" style={{ width: `${pct}%` }} />
              </div>
              <div className="progress-text">
                {progress.done}/{progress.total} ({pct}%)
                {progress.failed > 0 && ` · failed ${progress.failed}`}
                {progress.running ? " · running" : ""}
              </div>
            </div>
          )}
          <button onClick={exportBook} disabled={!progress || progress.done === 0} style={{ marginTop: 10 }}>
            Export (FB2 / EPUB / TXT / ZIP)
          </button>
        </section>

        {/* --- Log --- */}
        <section className="card">
          <h2>Log</h2>
          <div className="log" ref={logRef}>
            {log.map((l, i) => <div key={i}>{l}</div>)}
          </div>
        </section>
      </div>

      {/* --- Book details (cover / title / summary) --- */}
      {details && (book || ref) && (
        <section className="card">
          <h2>Book</h2>
          <div className="book-details">
            {details.cover ? (
              <img className="cover" src={details.cover} alt="cover" />
            ) : (
              <div className="cover cover-empty">no cover</div>
            )}
            <div className="book-meta">
              <div className="row">
                <strong className="book-title">
                  {details.title_translated || details.title || "(untitled)"}
                </strong>
                {!details.title_translated && (
                  <button onClick={translateTitle}>Translate title</button>
                )}
                <button onClick={replaceCover}>Replace cover</button>
              </div>
              {details.title_translated && details.title && (
                <div className="muted">original: {details.title}</div>
              )}
              <div className="muted">{details.author}</div>
              <textarea
                className="summary"
                placeholder="Summary / annotation…"
                defaultValue={details.summary || ""}
                onBlur={(e) => saveSummary(e.target.value)}
              />
            </div>
          </div>
        </section>
      )}

      {/* --- Glossary --- */}
      <section className="card">
        <div className="row">
          <h2>Glossary</h2>
          <button onClick={refreshGlossary}>Refresh</button>
          <span className="muted">{glossary.length} terms</span>
        </div>
        <div className="table-wrap">
          <table>
            <thead>
              <tr>
                <th>Source</th>
                <th>Translation</th>
                <th>Kind</th>
                <th>×</th>
                <th>📌</th>
              </tr>
            </thead>
            <tbody>
              {glossary.map((t) => (
                <tr key={t.source}>
                  <td>{t.source}</td>
                  <td>
                    <input
                      defaultValue={t.target}
                      onBlur={(e) => e.target.value !== t.target && pinTerm(t, e.target.value)}
                    />
                  </td>
                  <td>{t.kind}</td>
                  <td>{t.frequency}</td>
                  <td>{t.pinned ? "📌" : ""}</td>
                </tr>
              ))}
              {glossary.length === 0 && (
                <tr><td colSpan={5} className="empty">empty — grows during translation / bootstrap</td></tr>
              )}
            </tbody>
          </table>
        </div>
      </section>
    </main>
  );
}
