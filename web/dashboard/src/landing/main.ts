// The landing page (`landing.html`) is static HTML; this adds the styles and
// runs its Live-page mock (`#demo`) and the history sketch (`#history-sketch`).
import "../styles.css";
import { currentLocale } from "@/lib/locale";
import {
  CHART_SECONDS,
  chartPoints,
  check,
  type DemoState,
  feedState,
  formatUptime,
  historySketch,
  LOOKS,
  LOW_KBPS,
  type Look,
  OFFLINE_KBPS,
  type Reading,
  readingAt,
  SCENES,
  SERVER,
  START,
  seeded,
  UPTIME_START,
} from "./demo";

const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const kbps = (value: number) => value.toLocaleString(currentLocale());

function runDemo(root: HTMLElement) {
  const all = (name: string) => [...root.querySelectorAll<HTMLElement>(`[data-demo="${name}"]`)];
  const one = (name: string) =>
    root.querySelector<SVGElement | HTMLElement>(`[data-demo="${name}"]`);

  // Start a few seconds into the loop, with a minute of history already drawn.
  let t = 0;
  let switcher = START;
  const values: number[] = [];
  for (let i = -CHART_SECONDS; i < 0; i++) values.push(readingAt(i).kbps ?? 0);

  const main = chartPoints([], 150);
  for (const [name, value] of [
    ["low-line", LOW_KBPS],
    ["offline-line", OFFLINE_KBPS],
  ] as const) {
    const line = one(name);
    line?.setAttribute("y1", main.y(value).toFixed(1));
    line?.setAttribute("y2", main.y(value).toFixed(1));
  }

  const setTone = (element: Element | null, tone: string) => {
    if (!element) return;
    element.classList.remove("text-live", "text-low", "text-offline");
    element.classList.add(tone);
  };
  /** Replaces an element's state classes (the looks' `keys`) with `look`'s, keeping the rest. */
  const swap = (element: Element | null, keys: (keyof Look)[], look: Look) => {
    if (!element) return;
    for (const key of keys) {
      for (const other of Object.values(LOOKS)) element.classList.remove(...other[key].split(" "));
      element.classList.add(...look[key].split(" "));
    }
  };

  const log = (from: DemoState, to: DemoState, reading: Reading) => {
    const list = one("activity");
    if (!list) return;
    const item = document.createElement("li");
    item.className =
      "flex flex-col gap-0.5 border-b border-divider px-3.5 py-2.5 last:border-b-0 motion-safe:animate-fade-in";
    const head = document.createElement("span");
    head.className = "flex items-baseline justify-between gap-2";
    const title = document.createElement("span");
    title.className = "text-sm font-semibold";
    title.textContent = `${SCENES[from]} → ${SCENES[to]}`;
    const time = document.createElement("time");
    time.className = "font-mono text-[11px] text-muted-foreground tabular";
    time.textContent = new Date().toLocaleTimeString(currentLocale(), { hourCycle: "h23" });
    head.append(title, time);
    const detail = document.createElement("span");
    detail.className = "text-xs text-muted-foreground";
    detail.textContent =
      reading.kbps === null
        ? `Automatic · ${SERVER}`
        : `Automatic · ${SERVER} at ${kbps(reading.kbps)} kbps`;
    item.append(head, detail);
    list.prepend(item);
    while (list.children.length > 4) list.lastElementChild?.remove();
  };

  const render = (reading: Reading) => {
    const look = LOOKS[switcher.state];
    const feed = LOOKS[feedState(reading)];

    swap(one("pill"), ["soft", "text"], look);
    swap(one("pill-dot"), ["fill"], look);
    one("pill-dot")?.classList.toggle("animate-pulse", switcher.state === "live" && !still);
    swap(one("status-card"), ["border"], look);
    for (const label of all("pill-label")) label.textContent = look.label;
    for (const scene of all("scene")) scene.textContent = SCENES[switcher.state];

    for (const value of all("kbps"))
      value.textContent = reading.kbps === null ? "—" : kbps(reading.kbps);
    for (const value of all("rtt"))
      value.textContent = reading.rtt === null ? "—" : String(reading.rtt);
    const server = one("server-status");
    if (server) {
      server.textContent =
        reading.kbps === null ? "No feed" : `RTT ${reading.rtt} ms · ${feed.label.toLowerCase()}`;
    }

    const big = chartPoints(values, 150);
    one("line")?.setAttribute("points", big.line);
    one("area")?.setAttribute("points", big.area);
    setTone(one("chart"), look.text);
    const spark = chartPoints(values, 34);
    one("spark-line")?.setAttribute("points", spark.line);
    one("spark-area")?.setAttribute("points", spark.area);
    setTone(one("spark"), feed.text);

    for (const tile of root.querySelectorAll<HTMLElement>("[data-tile]")) {
      const onAir = tile.dataset.tile === SCENES[switcher.state];
      tile.className = `flex min-h-17 flex-col items-start justify-between gap-1.5 rounded-xl p-3 text-left ${
        onAir ? "border-2 border-primary bg-primary-soft" : "border bg-card"
      }`;
      const tag = tile.querySelector<HTMLElement>("[data-tag]");
      if (tag) {
        tag.className = `flex items-center gap-1.5 text-[11px] font-semibold tracking-wider ${
          onAir ? "text-offline" : "text-muted-foreground"
        }`;
        tag.innerHTML = onAir
          ? '<span class="size-1.5 rounded-full bg-offline"></span>ON AIR'
          : (tile.dataset.role ?? "SCENE");
      }
    }

    for (const uptime of all("uptime")) uptime.textContent = formatUptime(UPTIME_START + t);
  };

  const tick = () => {
    const reading = readingAt(t);
    const before = switcher.state;
    switcher = check(switcher, reading);
    values.push(reading.kbps ?? 0);
    if (values.length > CHART_SECONDS) values.shift();
    if (switcher.state !== before) log(before, switcher.state, reading);
    render(reading);
    t++;
  };

  tick();
  // With reduced motion the mock stays a still LIVE frame.
  if (still) return;
  setInterval(() => {
    if (!document.hidden) tick();
  }, 1000);
}

/** The History page in miniature: min–max band, average line and the state strip. */
function drawHistory(svg: SVGSVGElement) {
  const steps = historySketch();
  const width = 600;
  const height = 120;
  const max = 8000;
  const x = (i: number) => (i / (steps.length - 1)) * width;
  const y = (value: number) => height - 4 - (Math.min(value, max) / max) * (height - 8);
  const line = steps.map((step, i) => `${x(i).toFixed(1)},${y(step.avg).toFixed(1)}`).join(" ");
  const band = [
    ...steps.map((step, i) => `${x(i).toFixed(1)},${y(step.max).toFixed(1)}`),
    ...steps.map((step, i) => `${x(i).toFixed(1)},${y(step.min).toFixed(1)}`).reverse(),
  ].join(" ");
  svg.querySelector('[data-history="band"]')?.setAttribute("points", band);
  svg.querySelector('[data-history="line"]')?.setAttribute("points", line);
  const low = svg.querySelector('[data-history="low"]');
  low?.setAttribute("y1", y(LOW_KBPS).toFixed(1));
  low?.setAttribute("y2", y(LOW_KBPS).toFixed(1));

  const strip = svg.querySelector('[data-history="strip"]');
  if (!strip) return;
  const fills: Record<DemoState, string> = {
    live: "fill-live",
    low: "fill-low",
    offline: "fill-offline",
  };
  const segment = width / steps.length;
  let start = 0;
  steps.forEach((step, i) => {
    const next = steps[i + 1];
    if (next && next.state === step.state) return;
    const rect = document.createElementNS("http://www.w3.org/2000/svg", "rect");
    rect.setAttribute("x", (start * segment).toFixed(1));
    rect.setAttribute("y", String(height + 12));
    rect.setAttribute("width", Math.max(1, (i + 1 - start) * segment - 2).toFixed(1));
    rect.setAttribute("height", "8");
    rect.setAttribute("rx", "2");
    rect.setAttribute("class", fills[step.state]);
    strip.append(rect);
    start = i + 1;
  });
}

/** The BELABOX mock's per-interface sparklines (`data-spark` = the interface's kbps). */
function drawSparks() {
  const random = seeded(3);
  for (const svg of document.querySelectorAll<SVGSVGElement>("[data-spark]")) {
    const level = Number(svg.dataset.spark);
    const values = Array.from({ length: CHART_SECONDS }, () => level * (0.85 + random() * 0.3));
    values[values.length - 1] = level;
    const { line, area } = chartPoints(values, 28);
    const ns = "http://www.w3.org/2000/svg";
    const fill = document.createElementNS(ns, "polygon");
    fill.setAttribute("points", area);
    fill.setAttribute("class", "fill-current opacity-10");
    const stroke = document.createElementNS(ns, "polyline");
    stroke.setAttribute("points", line);
    stroke.setAttribute("class", "fill-none stroke-current");
    stroke.setAttribute("stroke-width", "2");
    stroke.setAttribute("stroke-linejoin", "round");
    stroke.setAttribute("vector-effect", "non-scaling-stroke");
    svg.append(fill, stroke);
  }
}

const demo = document.querySelector<HTMLElement>("#demo");
if (demo) runDemo(demo);
drawSparks();
const history = document.querySelector<SVGSVGElement>("#history-sketch");
if (history) drawHistory(history);
