import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// @ts-expect-error The browser source is plain JavaScript served from public/.
import { ObsWsRelayAppCore, ReconnectingWebSocket } from "/public/source/obs_ws.js";

class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  static instances: FakeWebSocket[] = [];

  readyState = FakeWebSocket.CONNECTING;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  send = vi.fn<(data: string) => void>();
  close = vi.fn(() => {
    this.readyState = FakeWebSocket.CLOSED;
  });

  constructor(_url: string) {
    FakeWebSocket.instances.push(this);
  }

  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.onopen?.({} as Event);
  }

  receive(value: unknown) {
    const data = typeof value === "string" ? value : JSON.stringify(value);
    this.onmessage?.({ data } as MessageEvent);
  }

  fireClose() {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.({ code: 1006, reason: "closed", wasClean: false } as CloseEvent);
  }
}

const connections: Array<{ stop: () => void }> = [];

function track<T extends { stop: () => void }>(connection: T) {
  connections.push(connection);
  return connection;
}

function sentJson(socket: FakeWebSocket) {
  return socket.send.mock.calls.map(([data]) => JSON.parse(data));
}

function socketAt(index: number) {
  const socket = FakeWebSocket.instances[index];
  if (!socket) throw new Error(`Missing fake socket at index ${index}`);
  return socket;
}

function delayAt(delays: number[], index: number) {
  const delay = delays[index];
  if (delay === undefined) throw new Error(`Missing reconnect delay at index ${index}`);
  return delay;
}

beforeEach(() => {
  FakeWebSocket.instances = [];
  vi.stubGlobal("WebSocket", FakeWebSocket);
  vi.useFakeTimers();
});

afterEach(() => {
  for (const connection of connections.splice(0)) {
    connection.stop();
  }
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("ReconnectingWebSocket", () => {
  it("jitters reconnect delays and caps the exponential base delay", async () => {
    vi.spyOn(Math, "random").mockReturnValueOnce(0).mockReturnValue(0.999);
    const delays: number[] = [];
    const connection = track(
      new ReconnectingWebSocket({
        url: "ws://test/link",
        reconnectDelay: 500,
        maxReconnectDelay: 1000,
        connectTimeout: 100000,
        onStateChange: (state: string, delay?: number) => {
          if (state === "reconnecting" && delay !== undefined) delays.push(delay);
        },
      }),
    );

    connection.start();
    socketAt(0).fireClose();
    expect(delayAt(delays, 0)).toBe(250);
    expect(delayAt(delays, 0)).toBeGreaterThanOrEqual(250);
    expect(delayAt(delays, 0)).toBeLessThanOrEqual(500);
    await vi.advanceTimersByTimeAsync(delayAt(delays, 0));

    socketAt(1).fireClose();
    expect(delayAt(delays, 1)).toBe(999.5);
    expect(delayAt(delays, 1)).toBeGreaterThanOrEqual(500);
    expect(delayAt(delays, 1)).toBeLessThanOrEqual(1000);
    await vi.advanceTimersByTimeAsync(1000);

    socketAt(2).fireClose();
    expect(delayAt(delays, 2)).toBe(999.5);
    expect(delayAt(delays, 2)).toBeGreaterThanOrEqual(500);
    expect(delayAt(delays, 2)).toBeLessThanOrEqual(1000);
  });

  it("abandons a stalled connection and ignores its late close", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const onClose = vi.fn();
    const connection = track(
      new ReconnectingWebSocket({
        url: "ws://test/link",
        onClose,
      }),
    );

    connection.start();
    const stalledSocket = socketAt(0);
    const lateClose = stalledSocket.onclose;
    await vi.advanceTimersByTimeAsync(10000);
    expect(stalledSocket.close).toHaveBeenCalledOnce();
    expect(onClose).toHaveBeenCalledOnce();
    expect(FakeWebSocket.instances).toHaveLength(1);

    await vi.advanceTimersByTimeAsync(249);
    expect(FakeWebSocket.instances).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeWebSocket.instances).toHaveLength(2);

    lateClose?.({ code: 1006, reason: "late close", wasClean: false } as CloseEvent);
    expect(FakeWebSocket.instances).toHaveLength(2);
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("only arms the idle watchdog after requested and resets it on traffic", async () => {
    vi.spyOn(Math, "random").mockReturnValue(0);
    const onClose = vi.fn();
    const connection = track(
      new ReconnectingWebSocket({
        url: "ws://test/link",
        idleTimeout: 45000,
        onClose,
      }),
    );

    connection.start();
    const socket = socketAt(0);
    socket.open();
    await vi.advanceTimersByTimeAsync(180000);
    expect(onClose).not.toHaveBeenCalled();

    connection.armIdleWatchdog();
    await vi.advanceTimersByTimeAsync(30000);
    socket.receive("activity");
    await vi.advanceTimersByTimeAsync(30000);
    expect(onClose).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(20000);
    expect(onClose).toHaveBeenCalledOnce();
    expect(socket.close).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(250);
    expect(FakeWebSocket.instances).toHaveLength(2);
  });
});

describe("ObsWsRelayAppCore", () => {
  it("reports the new OBS connection without letting an old close clear readiness", () => {
    const logger = vi.fn();
    const core = track(
      new ObsWsRelayAppCore("ws://cloud/link", {
        upstreamToken: "token",
        logger: { log: logger },
      }),
    );
    core.start();
    const upstream = socketAt(0);
    upstream.open();
    upstream.send.mockClear();
    upstream.receive({ settings: { obs: { port: 4455 } } });

    const oldObs = socketAt(1);
    oldObs.open();
    oldObs.receive({ op: 2, d: { negotiatedRpcVersion: 1 } });
    expect(core.obsReady).toBe(true);
    expect(sentJson(upstream)).toEqual([{ obsWsConnected: true }]);
    upstream.send.mockClear();
    const lateClose = oldObs.onclose;

    upstream.receive({ settings: { obs: { port: 4456 } } });
    expect(sentJson(upstream)).toEqual([{ obsWsConnected: false }]);
    const newObs = socketAt(2);
    newObs.open();
    newObs.receive({ op: 2, d: { negotiatedRpcVersion: 1 } });
    expect(sentJson(upstream)).toEqual([{ obsWsConnected: false }, { obsWsConnected: true }]);
    expect(core.obsReady).toBe(true);

    lateClose?.({ code: 1000, reason: "", wasClean: true } as CloseEvent);
    expect(sentJson(upstream)).toEqual([{ obsWsConnected: false }, { obsWsConnected: true }]);
    expect(core.obsReady).toBe(true);
  });

  it("arms keepalive monitoring without treating keepalive as OBS configuration", () => {
    const logger = vi.fn();
    const core = track(
      new ObsWsRelayAppCore("ws://cloud/link", {
        logger: { log: logger },
      }),
    );
    core.start();
    const upstream = socketAt(0);
    upstream.open();
    const upConnection = core.upConnection;
    if (!upConnection) throw new Error("Missing upstream connection");
    const armIdleWatchdog = vi.spyOn(upConnection, "armIdleWatchdog");
    const applyObsConfig = vi.spyOn(core, "applyObsConfig");

    upstream.receive({ keepalive: null });

    expect(armIdleWatchdog).toHaveBeenCalledOnce();
    expect(applyObsConfig).not.toHaveBeenCalled();
    expect(core.obsConnection).toBeNull();
    expect(logger.mock.calls.flat().join(" ")).not.toContain("keepalive");
  });
});
