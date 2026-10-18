---
name: doc
description: Write Markdown documents in the MDWisp portable document-folder format, including reports, plans, notes, tutorials and designs, or add images to an existing document. Supports no-image and image-rich modes.
---

# Write a portable document folder

Write the document as a **folder that carries its images**; copied to another machine, every image still shows.

```text
Notes/
├── Notes.md
└── Notes_img/
    └── architecture.svg
```

```markdown
![Architecture](./Notes_img/architecture.svg)
```

- Folder name, Markdown filename and `_img` prefix match. Without images there is no `_img`, but the file still sits in its matching folder.
- Use the location the user gave, else the current working directory. If the folder exists, ask whether to rename or use it; never overwrite.
- Adding images to an **existing** document: put them in its adjacent `DocumentName_img/` without moving the document.

## Image modes

Default to image-rich when the request does not say.

- **No images** ("no images", "text only"): no `_img`, no image syntax, no missing-image placeholders; use tables, lists or diagrams in code fences instead.
- **Image-rich** (default): illustrate flows, architecture, hierarchy, sequence, states, comparisons, data and layouts wherever a picture explains better. Every image must say something the text cannot; no decoration.

| Image type | Method |
| --- | --- |
| Structure, flow, relationships (most images) | Hand-written SVG |
| Charts | Plot to SVG/PNG with a script; keep only the images in `_img`, not the script |
| Real interfaces, output | Screenshot as PNG when possible; otherwise an SVG wireframe labelled as illustrative |
| Photos, illustrations | Use an image-generation tool if available; otherwise omit and say what is missing |

When unsure, make a useful SVG first and explain possible replacements at delivery. Describing an image without making it does not count.

## Never

1. Absolute image paths: `C:\...`, `/home/...`, `file:///...`.
2. Base64 image data, unless explicitly requested (say it is an exception).
3. Uploading generated images anywhere. Existing remote images may be linked, but can expire.

## Paths and names

- Start paths with `./`, use `/` everywhere, no URL encoding.
- Name files by content, e.g. `login-flow.svg`, not `image1.png`; Chinese is fine. No spaces, parentheses or `# ? % : * " < > |`; separate with `-`.
- Reuse identical files; for different content under a taken name add a suffix like `architecture-2.svg`. Overwrite only with permission.
- Give every image alt text that says what it shows.

## SVG

Set `viewBox` and fixed width/height; end `font-family` with CJK fallbacks (`'PingFang SC', 'Microsoft YaHei UI', 'Noto Sans SC', sans-serif`); no external images, fonts or `@import`; short labels with room to spare; a solid background.

## Writing

Optional YAML front matter (`title`, `tags`); one H1, no skipped heading levels; label code fences. Image syntax inside code is an example, not a file reference.

## Validate before delivery

```bash
node scripts/check.mjs "<document-folder-or-markdown-path>"
```

Add `--no-images` in no-image mode. It checks folder shape, relative existing image paths, unused files in `_img` and self-contained SVGs; fix until it passes. Report the folder path, image count and any image omitted for lack of a tool.
