import { Modal } from "../shared/ui/Modal";
import type { T } from "./strings";
export function About({ t, close }: { t: T; close: () => void }) {
  return (
    <Modal title={t("about")} closeLabel={t("close")} onClose={close}>
      <div className="bc-about">
        <div className="bc-about-brand">
          <img src="/logo.svg" alt="" width="36" height="36" />
          <strong>Book Converter</strong>
        </div>
        <p className="bc-hint">{t("distributionNotice")}</p>
        <dl className="bc-about-details">
          <dt>{t("version")}</dt>
          <dd>{__APP_BUILD__.version}</dd>
          <dt>{t("build")}</dt>
          <dd>{__APP_BUILD__.number}</dd>
          <dt>{t("builtAt")}</dt>
          <dd>{new Date(__APP_BUILD__.builtAt).toLocaleString()}</dd>
        </dl>
      </div>
    </Modal>
  );
}
