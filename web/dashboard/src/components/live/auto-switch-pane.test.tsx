import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AutoSwitchPane } from "./auto-switch-pane";
import type { ObsControls } from "./quick-actions";

describe("Operator auto switch pane", () => {
  const controls = { autoSwitch: { isPending: false, mutate: () => {} } } as unknown as ObsControls;
  it("shows actual thresholds and the watched server without inventing a hold option", () => {
    const html = renderToStaticMarkup(
      <AutoSwitchPane
        controls={controls}
        status={null}
        enabled
        onlyWhileStreaming
        canControl
        triggers={{ low: 2500, offline: 500, rtt: 800, rtt_offline: 2000 }}
        watching="Main feed"
        points={[]}
        now={0}
      />,
    );
    expect(html).toContain("Main feed");
    expect(html).toContain("2,500 kbps");
    expect(html).toContain("800 ms RTT");
    expect(html).not.toContain("Hold");
  });
  it("handles unavailable status, feed history and disabled thresholds", () => {
    const html = renderToStaticMarkup(
      <AutoSwitchPane
        controls={controls}
        status={undefined}
        enabled={false}
        onlyWhileStreaming
        canControl={false}
        triggers={{}}
        watching={undefined}
        points={[]}
        now={0}
      />,
    );
    expect(html).toContain("No active server");
    expect(html).toContain("Triggers off");
    expect(html).toContain("Paused");
    expect(html).toContain("disabled");
  });
});
