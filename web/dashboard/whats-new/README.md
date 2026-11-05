# What's new

Each post is a Markdown file here, shown on the public page `/whats-new` and, unless `popup: false`, popped up
once in the dashboard to everyone who signed up before its date. Publishing is merging the PR and deploying.
- **Name** it `YYYY-MM-DD-a-short-slug.md`. The name is the post's ID and its link (`/whats-new#2026-11-02-a-short-slug`),
  so don't rename a post once it's out. Posts sort by it, so a later date is a newer post.
- **Start** it with:

  ```markdown
  ---
  title: Pick scenes from the live view
  summary: One or two sentences for the popup (up to 300 characters, plain text).
  image: images/2026-11-02-scene-picker.webp
  image_alt: The scene picker open over the live view
  popup: true
  ---

  The details, in Markdown: **bold**, lists, [links](https://docs.noalbs.com/), `## headings`.
  ```

  Values are plain text: don't put them in quotes, even if they contain a colon.

  `image` (with `image_alt`) is optional: shown at the top of the post and in the popup. `popup` defaults to `true`;
  use `false` for small news that only goes on the page.
- **Images** go in `images/`, linked as `images/…` (in the text too: `![What it shows](images/x.webp)`; the alt text is
  required). WebP or AVIF (PNG and JPEG work), around 1600 px wide, at most 400 KB each. No GIFs: a short clip goes
  in as a muted, looping `.webm` or `.mp4` (at most 2 MB) with the same `![…](images/clip.webm)`; longer demos go on
  YouTube and get a link.
- **Links**: `https://…` (opens in a new tab), `mailto:…`, `/a-path` on this site or `#another-post`. Raw HTML is
  shown as text.

The build fails with what to fix when a post breaks one of these rules. `pnpm dev` and open `/whats-new` to see it.
English only.
