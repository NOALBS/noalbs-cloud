import { i18n } from "@lingui/core";
import { I18nProvider } from "@lingui/react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { SwitcherStatus } from "@/api/types";
import { BitratePane } from "./bitrate-pane";
import { LiveTimer, RecordingStatus } from "./live-header";
import { hasFeed, streamState } from "./live-status";
import { OnAirPane } from "./on-air-pane";
import type { ObsControls } from "./quick-actions";

const now = Date.parse("2026-01-01T12:00:00Z");
const status: SwitcherStatus = {
  updated_at: new Date(now).toISOString(),
  obs_connected: true,
  browser_source: true,
  streaming: true,
  recording: false,
  recording_paused: false,
  auto_switch: true,
  state: "live",
};
const controls = {
  autoSwitch: { isPending: false, mutate: vi.fn() },
  fix: { isPending: false, mutate: vi.fn() },
  marker: { isPending: false, mutate: vi.fn() },
} as unknown as ObsControls;

describe("Live status truth", () => {
  it("shows the recording chip only while OBS is known to be recording", () => {
    const markup = (value: SwitcherStatus | null) =>
      renderToStaticMarkup(<RecordingStatus status={value} now={now} />);
    expect(markup(status)).toBe("");
    expect(
      markup({
        ...status,
        recording: true,
        recording_started_at: new Date(now - 60_000).toISOString(),
      }),
    ).toContain("1:00");
    expect(markup({ ...status, recording: true, recording_paused: true })).toContain("paused");
    expect(markup(null)).toBe("");
    expect(markup({ ...status, recording: true, obs_connected: false })).toBe("");
  });
  it("distinguishes live, stopped, and unavailable OBS state", () => {
    expect(streamState(status, now)).toBe("live");
    expect(streamState({ ...status, streaming: false }, now)).toBe("idle");
    for (const unavailable of [
      undefined,
      null,
      { ...status, obs_connected: false },
      { ...status, browser_source: false },
      { ...status, updated_at: new Date(now - 46_000).toISOString() },
      { ...status, updated_at: "invalid" },
    ]) {
      expect(streamState(unavailable, now)).toBe("unknown");
      const header = renderToStaticMarkup(<LiveTimer status={unavailable} now={now} />);
      expect(header).toContain("OBS disconnected");
      expect(header).toContain("text-low");
      expect(header).not.toContain("Off air");
    }
  });

  it("does not call a live stream without a start timestamp Off air", () => {
    expect(renderToStaticMarkup(<LiveTimer status={status} now={now} />)).toContain("On air");
    expect(
      renderToStaticMarkup(<LiveTimer status={{ ...status, streaming: false }} now={now} />),
    ).toContain("Off air");
  });

  it("distinguishes a healthy zero sample from missing, offline and stale samples", () => {
    const sample = { at: now, bitrate: 0, rtt: null, state: "live" as const };
    expect(hasFeed(sample, now)).toBe(true);
    expect(hasFeed({ ...sample, bitrate: 6200 }, now)).toBe(true);
    expect(hasFeed(undefined, now)).toBe(false);
    expect(hasFeed({ ...sample, state: "offline" }, now)).toBe(false);
    expect(hasFeed({ ...sample, at: now - 46_000 }, now)).toBe(false);
    expect(hasFeed({ ...sample, at: Number.NaN }, now)).toBe(false);
  });

  it("marks the last known scene instead of calling disconnected OBS off air", () => {
    const markup = renderToStaticMarkup(
      <I18nProvider i18n={i18n}>
        <OnAirPane
          status={null}
          controls={controls}
          disabled
          paused={false}
          by={undefined}
          lastKnownScene="City walk"
        />
      </I18nProvider>,
    );
    expect(markup).toContain("OBS disconnected");
    expect(markup).toContain("Last known scene");
    expect(markup).toContain("City walk");
    expect(markup).toContain("opacity-50");
    expect(markup).not.toContain("Off air");
  });

  it("renders a red No feed state, not a healthy zero, for unavailable samples", () => {
    const sample = { at: now, bitrate: 0, rtt: null, state: "live" as const };
    const render = (
      current: typeof sample | undefined,
      currentStatus: SwitcherStatus | null = status,
    ) =>
      renderToStaticMarkup(
        <BitratePane
          controls={controls}
          status={currentStatus}
          enabled
          onlyWhileStreaming
          canControl={false}
          triggers={{ low: 2500, offline: 500, rtt: null, rtt_offline: null }}
          server="Main"
          sample={current}
          points={[]}
          now={now}
        />,
      );
    expect(render(sample)).not.toContain("No feed");
    for (const markup of [
      render(undefined),
      render({ ...sample, at: now - 46_000 }),
      render(sample, null),
    ]) {
      expect(markup).toContain("No feed");
      expect(markup).toContain("text-offline");
      expect(markup).toContain("bitrate unavailable");
    }
  });
});
