import { describe, expect, it } from "vitest";
import type { ObsSource } from "@/api/types";
import {
  keptSections,
  onStream,
  sourceKey,
  sourceKind,
  sourceSections,
  withEnabled,
} from "./obs-sources";

const source = (name: string, depth: number, enabled: boolean, extra: Partial<ObsSource> = {}) =>
  ({
    parent: "LIVE",
    parent_is_group: false,
    item_id: name.length,
    name,
    input_kind: "ffmpeg_source",
    is_scene: false,
    is_group: false,
    enabled,
    depth,
    ...extra,
  }) satisfies ObsSource;

describe("onStream", () => {
  it("hides whatever sits in a hidden scene or group", () => {
    const list = [
      source("Overlay", 0, false, { is_group: true }),
      source("Chat", 1, true),
      source("Alerts", 1, true, { is_scene: true }),
      source("Sound", 2, true),
      source("Camera", 0, true),
      source("Cams", 0, true, { is_scene: true }),
      source("Front", 1, true),
      source("Back", 1, false),
    ];
    expect(onStream(list)).toEqual([false, false, false, false, true, true, true, false]);
  });
});

describe("withEnabled", () => {
  it("changes only that source", () => {
    const camera = source("Camera", 0, true);
    const chat = source("Chat", 0, true);
    const list = withEnabled({ scene: "LIVE", sources: [camera, chat] }, sourceKey(camera), false);
    expect(list.sources.map((item) => item.enabled)).toEqual([false, true]);
  });
});

describe("sourceKind", () => {
  it("groups OBS's input kinds", () => {
    expect(sourceKind(source("a", 0, true, { input_kind: "irl_source" }))).toBe("media");
    expect(sourceKind(source("a", 0, true, { input_kind: "dshow_input" }))).toBe("camera");
    expect(sourceKind(source("a", 0, true, { input_kind: "text_gdiplus_v3" }))).toBe("text");
    expect(sourceKind(source("a", 0, true, { input_kind: "wasapi_input_capture" }))).toBe("audio");
    expect(sourceKind(source("a", 0, true, { input_kind: "game_capture" }))).toBe("capture");
    expect(sourceKind(source("a", 0, true, { input_kind: "color_source_v3" }))).toBe("other");
    expect(sourceKind(source("a", 0, true, { input_kind: null, is_group: true }))).toBe("group");
    expect(sourceKind(source("a", 0, true, { input_kind: null, is_scene: true }))).toBe("scene");
  });
});

describe("sourceSections", () => {
  it("puts the scene on air first, then each nested scene or group", () => {
    const sections = sourceSections([
      source("Overlay", 0, false, { is_group: true }),
      source("Chat", 1, true, { parent: "Overlay", parent_is_group: true }),
      source("Camera", 0, true),
      source("Cams", 0, true, { is_scene: true }),
      source("Front", 1, true, { parent: "Cams" }),
    ]);
    expect(
      sections.map((section) => [
        section.parent,
        section.parentIsGroup,
        section.parentShown,
        section.items.map((item) => [item.source.name, item.shown]),
      ]),
    ).toEqual([
      [
        null,
        false,
        true,
        [
          ["Overlay", false],
          ["Camera", true],
          ["Cams", true],
        ],
      ],
      ["Overlay", true, false, [["Chat", false]]],
      ["Cams", false, true, [["Front", true]]],
    ]);
  });
});

describe("keptSections", () => {
  it("drops removed sources, and sections left empty", () => {
    const sections = sourceSections([
      source("Overlay", 0, true, { is_group: true, uuid: "11111111-1111-4111-8111-111111111111" }),
      source("Chat", 1, true, { parent: "Overlay", parent_is_group: true }),
      source("Camera", 0, true),
    ]);
    const kept = keptSections(
      sections,
      new Set(["name:Chat", "11111111-1111-4111-8111-111111111111"]),
    );
    expect(kept.map((section) => section.items.map((item) => item.source.name))).toEqual([
      ["Camera"],
    ]);
  });
});
