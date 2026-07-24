type Props = {
  t: (key: string, vars?: Record<string, string | number>) => string;
  onOpenBook: () => void;
};

export function Welcome({ t, onOpenBook }: Props) {
  return (
    <div className="welcome">
      <h1>book-converter</h1>
      <p>{t("welcome.subtitle")}</p>
      <button onClick={onOpenBook}>{t("welcome.openBook")}</button>
      <p className="welcome-hint">{t("welcome.hint")}</p>
    </div>
  );
}
