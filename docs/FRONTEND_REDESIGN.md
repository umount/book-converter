# book-converter — Frontend Redesign (Cursor-style IDE)

> Status: planned (approved decisions recorded below). Date: 2026-07-24.
> Scope: a full rework of the React/TS frontend into a Cursor-like IDE, plus the
> small backend additions it requires. The Rust translation core is unchanged
> except where noted (advanced settings, book-wide replace).

## 1. Goals (from the request)

1. **Cursor-like IDE look and feel** across the whole app, not a flat gray theme.
2. **Show / hide the original text** as a first-class editor split control.
3. **IDE-style glossary highlighting**, not a solid blanket `<mark>`: clicking a
   glossary term highlights all of its occurrences and offers a jump into the
   glossary entry for that term.
4. **Line-number gutter** in the reader, like a code editor.
5. **Reworked settings** page (Cursor/VSCode-style), including advanced options.
6. **Find & replace + search**, in the current chapter and book-wide.
7. General cleanup and "other functionality" (command palette, hotkeys, status bar).

## 2. Constraints

- **Scale:** a book can have **up to ~1500 chapters** (long web novels). Every
  chapter list must be **virtualized** (render only visible rows) and reachable
  by **quick jump** (⌘P), never a 1500-item `<select>`.
- **A single chapter can also be long**, so the editor renders one chapter at a
  time and highlighting/gutter cost is bounded to that chapter.
- **Almost all of this is frontend.** Data already available on the client covers
  line numbers, occurrence highlight, term→glossary jump, and cross-pane
  source↔target linking (`chapter.source/translated` + the source→target pairs in
  `useGlossary`). Backend work is limited to §8.

## 3. Approved decisions

| Topic | Decision |
|---|---|
| Chapter navigation | **Activity bar + virtualized chapter tree (with search) + editor tabs + ⌘P** |
| Highlight interaction | **Resting: dotted underline. Click: highlight all occurrences + popover** (target rendering + "Open in glossary"). Cross-pane source↔target. |
| Extra features | **All:** command palette (⌘P), find-in-chapter (⌘F), keyboard shortcuts, bottom **status bar** |
| Find & replace | **⌘F find / ⌘H replace**, scope switch **current chapter / whole book** (book-wide needs a backend command) |
| Settings | **Restyle + advanced options** (model, base_url, max_chunk_chars, max_retries, temperature) — needs backend wiring (§8) |
| Theme | Dark only for now (Cursor default); tokens structured so a light theme can be added later |
| Virtualization | Hand-rolled fixed-row-height windowing (no new dependency); revisit `@tanstack/react-virtual` only if needed |

## 4. Target shell

```
┌──────────────────────────────────────────────────────────────┐
│  Title/command bar: File · View · ⌘P · busy indicator          │
├──┬────────────────────┬──────────────────────────────────────┤
│A │  Explorer          │  Tabs: [Overview][#12 Chapter…][#13*]  │
│c │  ▸ Project          │ ┌──────────────────────────────────┐ │
│t │    ▸ Chapters [🔎]   │ │ 1 │ source…    ▏ 1 │ translation…│ │  ← gutter
│i │      ✓ #11 …        │ │ 2 │ …          ▏ 2 │ …           │ │     both panes
│v │      ▸ #12 … (open) │ └──────────────────────────────────┘ │
│i │      · #13 …        ├──────────────────────────────────────┤
│t │  ▸ Glossary (N)     │  Bottom panel: Console (resizable)     │
├──┴────────────────────┴──────────────────────────────────────┤
│ Status: ● 42/1354 · ~3h · GBK · zh→ru · glossary 812 · Ln 12   │
└──────────────────────────────────────────────────────────────┘
```

- **Activity bar** (icon rail): Explorer · Glossary · Settings. Toggles the side panel.
- **Explorer**: project(s) + a **virtualized** chapter tree with a filter box.
  Status glyphs: `✓` done, `◆` reference, `✎` edited, `·` pending. Selecting a
  chapter opens it as a tab.
- **Editor area + tabs**: open chapters, plus special tabs **Overview** and
  **Glossary**. Dirty marker on unsaved edits.
- **Status bar**: progress %, spinner, ETA, current chapter, encoding, format,
  language pair, glossary size, and (in the reader) line position.
- **Bottom panel**: the console, made resizable, with a clear/close header.

## 5. Component and hook map

New/renamed under `src/`:

```
components/
  shell/ActivityBar.tsx        icon rail
  shell/Explorer.tsx           project + virtualized chapter tree + search
  shell/TabBar.tsx             editor tabs (chapters + Overview + Glossary)
  shell/StatusBar.tsx          bottom status line
  shell/BottomPanel.tsx        resizable host for Console
  CommandPalette.tsx           ⌘P quick-open chapters + actions
  reader/Editor.tsx            pane layout + modes (split / original / translation)
  reader/Gutter.tsx            line-number column
  reader/HighlightedText.tsx   tokenized text, occurrence highlight
  reader/TermPopover.tsx       term rendering + "Open in glossary"
  reader/FindReplaceBar.tsx    ⌘F / ⌘H bar (scope: chapter / book)
  settings/SettingsPage.tsx    section nav + search
  settings/SettingRow.tsx      label · description · control
hooks/
  useTabs.ts                   open tabs, active tab, dirty state
  useCommandPalette.ts         palette open/close, item sources
  useHotkeys.ts                global keybindings
  useFindReplace.ts            matches, current index, replace ops
  useVirtualRows.ts            fixed-height windowing helper
lib/
  highlight.ts                 tokenizer (replaces highlight() in types.ts)
```

`App.tsx` becomes a thin composition root (shell + panels + providers). Existing
hooks (`useProjects`, `useBookWorkspace`, `useGlossary`, `useTranslationJob`) are
kept and extended, not rewritten.

## 6. Reader / editor spec

**Line-number gutter.** Split the chapter text into display lines. A gutter column
(monospace, muted) renders the 1-based index aligned to each line. Wrapping of a
long paragraph keeps a single number for that logical line (gutter row height
follows the text row). Both panes get their own gutter; numbering is per pane
(source and translation lines rarely correspond 1:1).

**Pane modes (show/hide original).** A segmented control in the tab toolbar:
`Split` · `Translation only` · `Original only`, backed by the existing `panes`
state. A hotkey toggles the original pane. Both-closed state is not reachable via
the segmented control.

**Glossary highlighting (reworked).** Replace `highlight()` in `types.ts` with a
tokenizer in `lib/highlight.ts`:

- Build once per (chapter text, glossary version): a list of segments where each
  glossary term occurrence is a token carrying its `source` key. Memoized.
- **Resting state:** term tokens get a subtle **dotted underline** in a muted
  accent — not a filled highlight.
- **On click:** set an `activeTerm` (by `source`). All tokens with that key get an
  **occurrence highlight** (soft box, IDE-style). A **popover** anchors to the
  clicked token showing `source → target [kind]` and a button **"Open in
  glossary"** that switches to the Glossary tab, filters/scrolls to that entry,
  and flashes it (deep-link by `source`).
- **Cross-pane:** when a source term is active, its **target rendering** is also
  occurrence-highlighted in the translation pane (uses the source→target map from
  `useGlossary`), and vice-versa.
- Clicking empty space or Esc clears `activeTerm`.
- Performance: cap and ordering like today (longest-first, dedup), but tokenize
  once and reuse; no regex rebuild per render.

## 7. Find & replace + command palette + hotkeys

**Find / replace (`useFindReplace` + `FindReplaceBar`).**
- ⌘F opens find; ⌘H opens find+replace. Options: match case, whole word.
- **Scope: current chapter** (default) — matches highlighted in the active pane,
  next/prev navigation, replace / replace-all operate on the editable translation
  and persist via the existing `update_chapter_translation` path.
- **Scope: whole book** — calls a new backend command (§8) that replaces across all
  chapters' translations and reports how many changed; a confirm dialog first
  (it rewrites stored text). This is the deterministic literal counterpart to the
  glossary model-based `retarget_terms`.

**Command palette (`⌘P`).** Fuzzy list over: chapters (by number/title — the
answer to 1500-chapter navigation), plus actions (Translate this chapter, Start /
Pause, Toggle original, Open glossary, Export…, Settings). Enter opens/executes.

**Hotkeys (`useHotkeys`).** ⌘P palette · ⌘F find · ⌘H replace · Alt+↑/↓ prev/next
chapter · ⌘Enter translate current chapter · ⌘B toggle sidebar · ⌘J toggle
console · ⌘, settings. Shown in the command palette and tooltips.

## 8. Backend changes (minimal)

1. **Advanced settings** (`config.rs`): in `Config::load`, after language pair and
   before the env overrides, read optional keys from the settings DB and apply
   when non-empty: `model`, `base_url`, `max_chunk_chars`, `max_retries`,
   `temperature`. Precedence stays **default < settings DB < env** (env remains
   the power-user override), matching how `source_lang`/`target_lang` already
   work. No new command needed — the UI uses `get_setting`/`set_setting`.
2. **Effective config readback** (optional, nice-to-have): a `get_effective_config`
   command returning the non-secret effective values (model, base_url, chunk,
   retries, temperature, langs) so the settings page can show current defaults and
   whether an env var is overriding a field. Never returns the API key.
3. **Book-wide replace**: a `replace_in_book(project_id, find, replace, match_case,
   whole_word)` command → a `Store` method that iterates chapters, replaces in the
   translated title/body, marks them edited, and returns the count. Emits nothing;
   returns synchronously (fast, local SQLite).

No change to the translation pipeline, glossary logic, or export.

## 9. i18n

All new UI strings go through `i18n.ts` (`en` canonical, plus `ru`, `zh`) — no
literals in components, matching the current convention. New key groups:
`shell.*`, `tabs.*`, `palette.*`, `find.*`, `status.*`, `settings.*` (extended),
`reader.*` (gutter, pane modes, popover). English fallback as today.

## 10. Phasing

- **P0 — Design system + shell skeleton. [done]** Token layer in `styles.css`;
  activity bar, tab bar, status bar, resizable bottom panel; existing Overview /
  Reader / Glossary slotted in unchanged. Sidebar reduced to the project list
  (view navigation moved to the tab bar). Shared `lib/format.ts` (formatEta,
  langAbbr). App stays fully working (`tsc` + `vite build` clean).
- **P1 — Navigation.** Virtualized chapter tree + filter in Explorer; tabs open
  chapters; command palette (⌘P) with chapter quick-open; hotkeys.
- **P2 — Editor.** Gutter line numbers; pane-mode segmented control; new tokenized
  occurrence highlight + popover + jump-to-glossary + cross-pane linking.
- **P3 — Find & replace.** ⌘F / ⌘H in-chapter; book-wide via the backend command.
- **P4 — Glossary + Overview restyle.** Deep-link anchors, kind tags + filter;
  dashboard styling for Overview (all current actions preserved).
- **P5 — Settings.** Section-nav settings page + search; advanced options wired to
  the backend (§8); effective-config readback.
- **P6 — Polish.** Status-bar wiring, i18n completion, performance pass, tooltips
  and empty states.

Each phase is independently shippable and leaves the app runnable.

## 11. Out of scope (this pass)

- Light theme (tokens prepared, theme not delivered).
- Editing the **source** text (only translation stays editable).
- Parallel translation, cloud sync (unchanged from the main architecture doc).
- A minimap (can be added later if wanted).
