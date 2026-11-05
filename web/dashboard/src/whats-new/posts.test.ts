import { describe, expect, it } from "vitest";
import { dashboardModule, type Media, mediaPath, parsePost, postName, renderPage } from "./posts";

const picture = (path: string): Media => ({
  url: `/whats-new/${path}`,
  video: path.endsWith(".webm"),
  ...(path.endsWith(".webm") ? {} : { width: 1600, height: 900 }),
});

const post = (front: string, body = "Hello.", name = "2026-11-02-scene-picker.md") =>
  parsePost(name, `---\n${front}\n---\n\n${body}\n`, picture);

const basic = "title: Scene picker\nsummary: Pick scenes.";

describe("postName", () => {
  it("takes the date and ID from the file name", () => {
    expect(postName("2026-11-02-scene-picker.md")).toEqual({
      id: "2026-11-02-scene-picker",
      date: "2026-11-02",
    });
  });

  it("refuses names that wouldn't sort or link well", () => {
    for (const name of [
      "scene-picker.md",
      "2026-11-02.md",
      "2026-02-30-x.md",
      "2026-11-02-Scene.md",
      "2026-11-02-a--b.md",
      "2026-11-02-x.markdown",
    ]) {
      expect(postName(name), name).toBeNull();
    }
  });
});

describe("parsePost", () => {
  it("reads the fields and renders the text", () => {
    const parsed = post(`${basic}\nimage: images/picker.webp\nimage_alt: The picker`, "**New**");
    expect(parsed).toMatchObject({
      id: "2026-11-02-scene-picker",
      title: "Scene picker",
      summary: "Pick scenes.",
      popup: true,
      image: { url: "/whats-new/images/picker.webp", width: 1600, height: 900, alt: "The picker" },
    });
    expect(parsed.html).toContain('<img class="post-image" src="/whats-new/images/picker.webp"');
    expect(parsed.html).toContain("<strong>New</strong>");
    expect(post(`${basic}\npopup: false`).popup).toBe(false);
  });

  it("refuses quoted field values", () => {
    expect(() => post('title: "Quoted"\nsummary: Pick scenes.')).toThrow(/no quotes needed/);
  });

  it("opens outside links in a new tab", () => {
    const { html } = post(basic, "[Docs](https://docs.noalbs.com/) and [home](/)");
    expect(html).toContain(
      '<a href="https://docs.noalbs.com/" target="_blank" rel="noopener noreferrer">Docs</a>',
    );
    expect(html).toContain('<a href="/">home</a>');
  });

  it("escapes HTML instead of rendering it", () => {
    const { html } = post(basic, '<script>alert(1)</script>\n\n<img src=x onerror="alert(1)">');
    expect(html).not.toContain("<script");
    expect(html).not.toContain("<img src=x");
    expect(html).toContain("&lt;script&gt;");
  });

  it("refuses links that could run script", () => {
    expect(() => post(basic, "[x](javascript:alert(1))")).toThrow(/link "javascript:alert\(1\)"/);
    expect(() => post(basic, "[x](//evil.example)")).toThrow(/link/);
  });

  it("only shows files from whats-new/, with alt text", () => {
    expect(() => post(basic, "![](images/x.webp)")).toThrow(/alt text/);
    expect(() => post(basic, "![x](https://example.com/x.png)")).toThrow(/whats-new\/images/);
    expect(() => post(basic, "![x](../src/x.png)")).toThrow(/inside whats-new/);
    expect(post(basic, "![A clip](images/clip.webm)").html).toContain(
      '<video src="/whats-new/images/clip.webm" aria-label="A clip" autoplay muted loop playsinline>',
    );
  });

  it("makes the post's headings smaller than its title", () => {
    expect(post(basic, "## Details").html).toContain("<h4>Details</h4>");
  });

  it("says which file is wrong and how", () => {
    expect(() => post("title: x")).toThrow("whats-new/2026-11-02-scene-picker.md: summary");
    expect(() => post(`${basic}\nimage: images/x.webp`)).toThrow(/image_alt/);
    expect(() => post(`${basic}\npopup: yes`)).toThrow(/popup: true or false/);
    expect(() => post(`${basic}\nauthor: me`)).toThrow(/unknown line/);
    expect(() => post(`${basic}\ntitle: again`)).toThrow(/twice/);
    expect(() => parsePost("2026-11-02-x.md", "No front matter", picture)).toThrow(/---/);
    expect(() => post(basic, "Hello.", "scene-picker.md")).toThrow(/YYYY-MM-DD/);
  });
});

describe("mediaPath", () => {
  it("keeps paths relative to whats-new/", () => {
    expect(mediaPath("./images/x.webp")).toBe("images/x.webp");
    expect(() => mediaPath("/assets/x.webp")).toThrow();
    expect(() => mediaPath("images//x.webp")).toThrow();
  });
});

describe("output", () => {
  const posts = [post(`${basic}\nimage: images/picker.webp\nimage_alt: The picker`)];

  it("anchors each post on the page", () => {
    expect(renderPage(posts)).toContain('<article id="2026-11-02-scene-picker" class="post">');
    expect(renderPage(posts)).toContain('<time datetime="2026-11-02">November 2, 2026</time>');
    expect(renderPage([])).toContain("Nothing yet");
  });

  it("imports images for the dashboard, so they get hashed, and leaves out the text", () => {
    const module = dashboardModule(posts);
    expect(module).toContain('import image0 from "/whats-new/images/picker.webp";');
    expect(module).toContain("url: image0");
    expect(module).not.toContain("Hello");
  });
});
