import { describe, expect, it } from "vitest";
import { redactLog } from "./redact-log";

describe("redactLog", () => {
  it.each([
    ["rtmp://host/app/KEY", "rtmp://host/app/••••"],
    ["rtmps://host/app/KEY", "rtmps://host/app/••••"],
    ["srt://host:9000/app/KEY", "srt://host:9000/app/••••"],
    ["rist://host/app/KEY", "rist://host/app/••••"],
  ])("redacts stream keys in %s", (input, expected) => {
    expect(redactLog(input)).toBe(expected);
  });

  it("redacts sensitive query parameters and bearer tokens", () => {
    expect(
      redactLog(
        "url?key=abc&token=xyz&passphrase=pw&streamid=id&pass=p&password=pw&secret=s&auth=a Bearer abc.def",
      ),
    ).toBe(
      "url?key=••••&token=••••&passphrase=••••&streamid=••••&pass=••••&password=••••&secret=••••&auth=•••• Bearer ••••",
    );
  });

  it("leaves ordinary OBS lines unchanged", () => {
    const line = "22:14:07.100: [obs-websocket] Connected to server";
    expect(redactLog(line)).toBe(line);
  });
});
