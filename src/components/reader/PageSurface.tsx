import { useMemo } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { tokenizeLines, type Match, type SearchSpec } from "../../lib/highlight";
import type { ChapterBlock } from "../../types";
import { LineTokens } from "./LineTokens";

type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  blocks: ChapterBlock[];
  /** Glossary matches to underline in the page's text. */
  matches: Match[];
  activeKey: string | null;
  onTermClick?: (key: string, rect: DOMRect) => void;
  search?: SearchSpec | null;
  currentSearch?: number;
};

/** URL the webview can fetch a project's image from (see Rust `assets`). */
export function assetUrl(asset: string): string {
  return convertFileSrc(asset, "bookasset");
}

/**
 * A chapter that is not plain prose: text, pictures and captions in the order
 * the page had them.
 *
 * Read-only by design. The translation of such a chapter is edited as text
 * (with `[[img:…]]` lines marking where each picture sits), because a picture
 * is not something a translator edits — only the words around it are.
 */
export function PageSurface({
  t, blocks, matches, activeKey, onTermClick, search, currentSearch,
}: Props) {
  // One tokenization per block, so a long illustrated chapter does not redo the
  // whole page every time the find bar changes one character.
  const lines = useMemo(
    () =>
      blocks.map((b) =>
        b.kind === "image" ? [] : tokenizeLines(b.text ?? "", matches, search),
      ),
    [blocks, matches, search],
  );

  if (blocks.length === 0) {
    return <div className="ch-empty muted">{t("reader.emptyPage")}</div>;
  }

  return (
    <div className="page-surface">
      {blocks.map((block, i) => {
        if (block.kind === "image") {
          return block.asset ? (
            <img
              key={block.ord}
              className="page-image"
              src={assetUrl(block.asset)}
              width={block.width ?? undefined}
              height={block.height ?? undefined}
              alt={t("reader.pageImage")}
              loading="lazy"
              draggable={false}
            />
          ) : (
            <div key={block.ord} className="page-image-missing muted">{t("reader.imageMissing")}</div>
          );
        }
        return (
          <div key={block.ord} className={block.kind === "caption" ? "page-caption" : "page-text"}>
            {lines[i].map((tokens, li) => (
              <div className="page-line" key={li}>
                <LineTokens
                  tokens={tokens} activeKey={activeKey}
                  onTermClick={onTermClick} currentSearch={currentSearch}
                />
              </div>
            ))}
          </div>
        );
      })}
    </div>
  );
}
