import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { RecoveryBundlePanel } from "./RecoveryBundlePanel";

describe("RecoveryBundlePanel", () => {
  it("requires the recovery-bundle passphrase to be entered twice", () => {
    const markup = renderToStaticMarkup(<RecoveryBundlePanel resourcesRevision={0} />);

    expect(markup.match(/type="password"/g)).toHaveLength(2);
    expect(markup).toContain("Повторите пароль");
  });
});
