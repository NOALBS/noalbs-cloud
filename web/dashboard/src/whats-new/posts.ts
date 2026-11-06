// Turns the "What's new" posts (`whats-new/*.md`) into the public page and the
// list the dashboard pops up, at build time. Posts are written by us and
// reviewed in PRs, but raw HTML is still escaped and only known kinds of links
// and files get through, so a post can't break the page.
import { Marked, type Tokens } from "marked";
import type { WhatsNewPost } from "../lib/whats-new";

/** Bigger screenshots should be resized or saved as WebP/AVIF. */
export const MAX_IMAGE_BYTES = 400 * 1024;
/** A few seconds of muted, looping clip; longer demos go on YouTube. */
export const MAX_VIDEO_BYTES = 2 * 1024 * 1024;
export const IMAGE_TYPES = ["webp", "avif", "png", "jpg", "jpeg"];
export const VIDEO_TYPES = ["webm", "mp4"];

const FILE_NAME = /^(\d{4}-\d{2}-\d{2})-[a-z0-9]+(?:-[a-z0-9]+)*\.md$/;
const MAX_ID_LEN = 100;
const MAX_TITLE_LEN = 120;
const MAX_SUMMARY_LEN = 300;
const FIELDS = ["title", "summary", "image", "image_alt", "popup"] as const;

/** A file a post shows, found by the caller. */
export interface Media {
  /** Where the page loads it from, under the dashboard root (`/whats-new/images/x.webp`). */
  url: string;
  video: boolean;
  width?: number;
  height?: number;
}

/** Finds `path` (relative to `whats-new/`) and checks it, or throws. */
export type ResolveMedia = (path: string) => Media;

/** A post: what the dashboard gets, plus its body as HTML for the page. */
export interface Post extends WhatsNewPost {
  html: string;
}

export class PostError extends Error {}

/** The ID and date in a post's file name, or `null` if it isn't named like a post. */
export function postName(fileName: string): { id: string; date: string } | null {
  const match = FILE_NAME.exec(fileName);
  const id = fileName.slice(0, -".md".length);
  if (!match?.[1] || id.length > MAX_ID_LEN) return null;
  if (Number.isNaN(Date.parse(`${match[1]}T00:00:00Z`))) return null;
  if (new Date(`${match[1]}T00:00:00Z`).toISOString().slice(0, 10) !== match[1]) return null;
  return { id, date: match[1] };
}

export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

/** A path inside `whats-new/` (`images/x.webp`), or an error. */
export function mediaPath(href: string): string {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("/") || href.includes("\\")) {
    throw new PostError(`"${href}": put the file in whats-new/images/ and link it as images/…`);
  }
  const parts = href.replace(/^\.\//, "").split("/");
  if (parts.some((part) => part === "" || part === "." || part === "..")) {
    throw new PostError(`"${href}" has to stay inside whats-new/`);
  }
  return parts.join("/");
}

function checkLink(href: string) {
  const ok =
    /^https?:\/\/[^\s/]/.test(href) ||
    /^mailto:/.test(href) ||
    (href.startsWith("/") && !href.startsWith("//")) ||
    href.startsWith("#");
  if (!ok) throw new PostError(`link "${href}": use https://…, mailto:…, /a-path or #a-post`);
}

function mediaTag(media: Media, alt: string, className?: string): string {
  const attrs = [
    className ? `class="${className}"` : "",
    `src="${escapeHtml(media.url)}"`,
    media.width && media.height ? `width="${media.width}" height="${media.height}"` : "",
  ].filter(Boolean);
  if (media.video) {
    return `<video ${attrs.join(" ")} aria-label="${escapeHtml(alt)}" autoplay muted loop playsinline></video>`;
  }
  return `<img ${attrs.join(" ")} alt="${escapeHtml(alt)}" loading="lazy" decoding="async">`;
}

function bodyRenderer(resolve: ResolveMedia) {
  return new Marked({
    gfm: true,
    renderer: {
      html({ text }) {
        return escapeHtml(text);
      },
      heading({ tokens, depth }) {
        // The page's title is the h1 and each post's title an h2.
        const level = Math.min(depth + 2, 6);
        return `<h${level}>${this.parser.parseInline(tokens)}</h${level}>\n`;
      },
      link({ href, title, tokens }: Tokens.Link) {
        checkLink(href);
        const external = /^https?:/.test(href);
        return `<a href="${escapeHtml(href)}"${title ? ` title="${escapeHtml(title)}"` : ""}${
          external ? ' target="_blank" rel="noopener noreferrer"' : ""
        }>${this.parser.parseInline(tokens)}</a>`;
      },
      image({ href, text }: Tokens.Image) {
        if (!text.trim()) throw new PostError(`![…](${href}) needs alt text: what it shows`);
        return mediaTag(resolve(mediaPath(href)), text);
      },
    },
  });
}

/** The `key: value` lines between the `---` at the top of a post. */
function frontMatter(source: string): { fields: Map<string, string>; body: string } {
  const match = /^---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)([\s\S]*)$/.exec(source);
  if (!match) throw new PostError("start with a --- block: title, summary, …");
  const fields = new Map<string, string>();
  for (const line of (match[1] ?? "").split(/\r?\n/)) {
    if (!line.trim()) continue;
    const field = /^([a-z_]+):\s*(.*?)\s*$/.exec(line);
    const key = field?.[1];
    if (!key || !(FIELDS as readonly string[]).includes(key)) {
      throw new PostError(`unknown line "${line}" (known: ${FIELDS.join(", ")})`);
    }
    if (fields.has(key)) throw new PostError(`${key} is there twice`);
    const value = field[2] ?? "";
    if (
      (value.startsWith('"') && value.endsWith('"')) ||
      (value.startsWith("'") && value.endsWith("'"))
    ) {
      throw new PostError(`${key}: no quotes needed around the value`);
    }
    fields.set(key, value);
  }
  return { fields, body: match[2] ?? "" };
}

/** Reads one post; throws with the file name and what to fix. */
export function parsePost(fileName: string, source: string, resolve: ResolveMedia): Post {
  try {
    const name = postName(fileName);
    if (!name) {
      throw new PostError("name it YYYY-MM-DD-a-short-slug.md (lowercase letters, digits, dashes)");
    }
    const { fields, body } = frontMatter(source);
    const title = fields.get("title") ?? "";
    const summary = fields.get("summary") ?? "";
    if (!title || title.length > MAX_TITLE_LEN) {
      throw new PostError(`title: 1 to ${MAX_TITLE_LEN} characters`);
    }
    if (!summary || summary.length > MAX_SUMMARY_LEN) {
      throw new PostError(`summary: 1 to ${MAX_SUMMARY_LEN} characters, shown in the popup`);
    }
    const popup = fields.get("popup") ?? "true";
    if (popup !== "true" && popup !== "false") throw new PostError("popup: true or false");

    let image: Post["image"] = null;
    let imageHtml = "";
    const imagePath = fields.get("image");
    if (imagePath) {
      const alt = fields.get("image_alt") ?? "";
      if (!alt) throw new PostError("image_alt: say what the image shows");
      const media = resolve(mediaPath(imagePath));
      if (media.video || !media.width || !media.height) {
        throw new PostError("image: a picture (a clip can go in the text)");
      }
      image = { url: media.url, width: media.width, height: media.height, alt };
      imageHtml = mediaTag(media, alt, "post-image");
    } else if (fields.has("image_alt")) {
      throw new PostError("image_alt without an image");
    }

    const html = imageHtml + bodyRenderer(resolve).parse(body, { async: false });
    return { ...name, title, summary, image, popup: popup === "true", html };
  } catch (error) {
    if (error instanceof PostError) {
      throw new PostError(`whats-new/${fileName}: ${error.message}`);
    }
    throw error;
  }
}

function longDate(date: string): string {
  return new Date(`${date}T00:00:00Z`).toLocaleDateString("en-US", {
    dateStyle: "long",
    timeZone: "UTC",
  });
}

/** The posts on the public page, newest first. */
export function renderPage(posts: Post[]): string {
  if (posts.length === 0) return "<p>Nothing yet.</p>";
  return posts
    .map(
      (post) => `<article id="${post.id}" class="post">
  <header>
    <time datetime="${post.date}">${longDate(post.date)}</time>
    <h2><a href="#${post.id}">${escapeHtml(post.title)}</a></h2>
  </header>
  ${post.html}
</article>`,
    )
    .join("\n");
}

/** The dashboard's list (`virtual:whats-new`): images are imported, so Vite hashes them. */
export function dashboardModule(posts: Post[]): string {
  const imports: string[] = [];
  const list = posts.map(({ html: _html, image, ...post }) => {
    if (!image) return `${JSON.stringify({ ...post, image: null })}`;
    const name = `image${imports.length}`;
    imports.push(`import ${name} from ${JSON.stringify(image.url)};`);
    const { url: _url, ...rest } = image;
    return `{ ...${JSON.stringify(post)}, image: { ...${JSON.stringify(rest)}, url: ${name} } }`;
  });
  return `${imports.join("\n")}\nexport default [\n${list.join(",\n")}\n];\n`;
}
