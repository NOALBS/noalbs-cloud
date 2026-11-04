import { describe, expect, it } from "vitest";
import { redactLog } from "./redact-log";

describe("redactLog", () => {
  it.each([
    ["rtmp://host/app/KEY", "rtmp://host/app/••••"],
    ["rtmp://host/KEY", "rtmp://host/••••"],
    ["rtmp://host/app", "rtmp://host/••••"],
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

  it("redacts valid IP addresses but leaves clock times and versions unchanged", () => {
    expect(redactLog("192.168.1.42 2001:db8::1")).toBe("•••• ••••");
    expect(redactLog("2001:db8::ffff:192.168.1.42")).toBe("••••");
    expect(redactLog("2001:0db8:85a3:0000:0000:8a2e:0370:7334")).toBe("••••");
    expect(redactLog("::1 fe80::1")).toBe("•••• ••••");
    expect(redactLog("16:53:47")).toBe("16:53:47");
    expect(redactLog("time 16:53:47")).toBe("time 16:53:47");
    const line = "16:53:47.275: OBS 30.2.3 connected to 127.0.0.1";
    expect(redactLog(line)).toBe("16:53:47.275: OBS 30.2.3 connected to ••••");
  });
});
