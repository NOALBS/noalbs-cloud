import { describe, expect, it } from "vitest";
import type { ObsSource } from "@/api/types";
import {
  copyName,
  duplicable,
  editableKind,
  keptSections,
  onStream,
  removable,
  sourceKey,
  sourceKind,
  sourceSections,
  withEnabled,
  withMovedTo,
  withRemoved,
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

describe("withMovedTo", () => {
  const list = (sources: ObsSource[]) => ({ scene: "LIVE", sources });
  const names = (moved: { sources: ObsSource[] }) =>
    moved.sources.map((item) => `${item.depth}${item.name}`);
  const tree = list([
    source("Chat", 0, true, { item_id: 1 }),
    source("Overlay", 0, true, { item_id: 2, is_group: true }),
    source("Alerts", 1, true, { item_id: 1, parent: "Overlay", parent_is_group: true }),
    source("Logo", 1, true, { item_id: 2, parent: "Overlay", parent_is_group: true }),
    source("Camera", 0, true, { item_id: 3 }),
  ]);

  it("moves a nested scene or group with everything in it", () => {
    expect(names(withMovedTo(tree, "scene:LIVE:2", 0))).toEqual([
      "0Overlay",
      "1Alerts",
      "1Logo",
      "0Chat",
      "0Camera",
    ]);
    expect(names(withMovedTo(tree, "scene:LIVE:1", 2))).toEqual([
      "0Overlay",
      "1Alerts",
      "1Logo",
      "0Camera",
      "0Chat",
    ]);
    expect(names(withMovedTo(tree, "scene:LIVE:3", 0))).toEqual([
      "0Camera",
      "0Chat",
      "0Overlay",
      "1Alerts",
      "1Logo",
    ]);
  });

  it("stays within its scene or group", () => {
    expect(names(withMovedTo(tree, "group:Overlay:2", 0))).toEqual([
      "0Chat",
      "0Overlay",
      "1Logo",
      "1Alerts",
      "0Camera",
    ]);
    // Past the end: the bottom of its own group, not the scene.
    expect(names(withMovedTo(tree, "group:Overlay:1", 9))).toEqual([
      "0Chat",
      "0Overlay",
      "1Logo",
      "1Alerts",
      "0Camera",
    ]);
    expect(withMovedTo(tree, "scene:LIVE:1", 0)).toBe(tree);
    expect(withMovedTo(tree, "scene:LIVE:9", 0)).toBe(tree);
  });
});

describe("editableKind", () => {
  it("knows media, VLC and browser sources only", () => {
    expect(editableKind(source("SRT", 0, true))).toBe("media");
    expect(editableKind(source("VLC", 0, true, { input_kind: "vlc_source" }))).toBe("vlc");
    expect(editableKind(source("Chat", 0, true, { input_kind: "browser_source" }))).toBe("browser");
    expect(editableKind(source("Logo", 0, true, { input_kind: "image_source" }))).toBeNull();
    expect(editableKind(source("Cams", 0, true, { input_kind: null, is_scene: true }))).toBeNull();
  });
});

describe("withRemoved", () => {
  it("takes a nested scene away with what's in it", () => {
    const list = {
      scene: "LIVE",
      sources: [
        source("Chat", 0, true, { item_id: 1 }),
        source("Cams", 0, true, { item_id: 2, is_scene: true }),
        source("Front", 1, true, { item_id: 1, parent: "Cams" }),
        source("Camera", 0, true, { item_id: 3 }),
      ],
    };
    expect(withRemoved(list, "scene:LIVE:2").sources.map((item) => item.name)).toEqual([
      "Chat",
      "Camera",
    ]);
    expect(withRemoved(list, "scene:LIVE:9")).toBe(list);
  });

  it("leaves groups and what's in them to OBS", () => {
    expect(removable(source("Chat", 0, true))).toBe(true);
    expect(removable(source("Overlay", 0, true, { is_group: true }))).toBe(false);
    expect(removable(source("Logo", 1, true, { parent: "Overlay", parent_is_group: true }))).toBe(
      false,
    );
  });
});

describe("editableKind for text", () => {
  it("knows GDI+ and FreeType 2 text in any version", () => {
    for (const kind of ["text_gdiplus", "text_gdiplus_v3", "text_ft2_source_v2"]) {
      expect(editableKind(source("Status", 0, true, { input_kind: kind }))).toBe("text");
    }
    expect(editableKind(source("X", 0, true, { input_kind: "text_gdiplus_vx" }))).toBeNull();
  });
});

describe("copyName", () => {
  it("numbers copies like OBS and skips names in use", () => {
    expect(copyName("BELABOX SRT", [])).toBe("BELABOX SRT 2");
    expect(copyName("Cam 2", [])).toBe("Cam 3");
    expect(copyName("Chat", ["Chat 2", "Chat 3"])).toBe("Chat 4");
  });

  it("copies inputs only", () => {
    expect(duplicable(source("SRT", 0, true))).toBe(true);
    expect(duplicable(source("Cams", 0, true, { is_scene: true }))).toBe(false);
    expect(duplicable(source("Overlay", 0, true, { is_group: true }))).toBe(false);
  });
});
