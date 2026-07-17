import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { OperationFailureNotice } from "./OperationFailureNotice";

describe("OperationFailureNotice", () => {
  it("states the safe outcome and the possible side effect", () => {
    const markup = renderToStaticMarkup(
      <OperationFailureNotice
        message="The operation failed."
        safe="restoreFailureSafe"
        changed="restoreFailureChanged"
        t={(key) => key}
      />,
    );

    expect(markup).toContain("failureSafeLabel");
    expect(markup).toContain("restoreFailureSafe");
    expect(markup).toContain("failureChangedLabel");
    expect(markup).toContain("restoreFailureChanged");
  });
});
