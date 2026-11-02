// Builds the "What's new" posts: fills in `whats-new.html` and provides
// `virtual:whats-new` to the dashboard (see `posts.ts`).
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, resolve, sep } from "node:path";
import { imageSize } from "image-size";
import type { Plugin } from "vite";
import {
  dashboardModule,
  IMAGE_TYPES,
  MAX_IMAGE_BYTES,
  MAX_VIDEO_BYTES,
  type Media,
  type Post,
  PostError,
  parsePost,
  renderPage,
  VIDEO_TYPES,
} from "./posts";

const MODULE = "virtual:whats-new";
const RESOLVED = `\0${MODULE}`;
/** Where `whats-new.html` gets the posts. */
const PLACEHOLDER = "<!-- whats-new:posts -->";

/** A file a post shows, checked: inside `whats-new/`, a known type, small enough. */
function resolveMedia(dir: string, path: string): Media {
  const file = resolve(dir, path);
  if (!file.startsWith(dir + sep)) throw new PostError(`"${path}" has to stay inside whats-new/`);
  if (!existsSync(file)) throw new PostError(`"${path}" isn't there`);
  const type = extname(file).slice(1).toLowerCase();
  const video = VIDEO_TYPES.includes(type);
  if (!video && !IMAGE_TYPES.includes(type)) {
    throw new PostError(
      `"${path}": use ${[...IMAGE_TYPES, ...VIDEO_TYPES].join(", ")} (no GIFs: make a .webm)`,
    );
  }
  const bytes = statSync(file).size;
  const max = video ? MAX_VIDEO_BYTES : MAX_IMAGE_BYTES;
  if (bytes > max) {
    throw new PostError(
      `"${path}" is ${Math.round(bytes / 1024)} KB, over ${max / 1024} KB: resize it or save it as WebP/AVIF`,
    );
  }
  const url = `/whats-new/${path}`;
  if (video) return { url, video };
  const { width, height } = imageSize(readFileSync(file));
  return { url, video, width, height };
}

export function loadPosts(dir: string): Post[] {
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((name) => name.endsWith(".md") && name !== "README.md")
    .map((name) =>
      parsePost(name, readFileSync(resolve(dir, name), "utf8"), (path) => resolveMedia(dir, path)),
    )
    .sort((a, b) => (a.id < b.id ? 1 : -1));
}

export function whatsNew(): Plugin {
  let dir = resolve("whats-new");
  return {
    name: "whats-new",
    configResolved(config) {
      dir = resolve(config.root, "whats-new");
    },
    resolveId(id) {
      return id === MODULE ? RESOLVED : undefined;
    },
    load(id) {
      if (id !== RESOLVED) return undefined;
      this.addWatchFile(dir);
      return dashboardModule(loadPosts(dir));
    },
    transformIndexHtml: {
      // Before Vite looks at <img src>, so the images get hashed into /assets.
      order: "pre",
      handler(html) {
        return html.includes(PLACEHOLDER)
          ? html.replace(PLACEHOLDER, renderPage(loadPosts(dir)))
          : html;
      },
    },
    configureServer(server) {
      server.watcher.add(dir);
      server.watcher.on("all", (_event, file) => {
        if (!file.startsWith(dir + sep)) return;
        const module = server.moduleGraph.getModuleById(RESOLVED);
        if (module) server.moduleGraph.invalidateModule(module);
        server.ws.send({ type: "full-reload" });
      });
    },
  };
}
