---
name: share
description: Export a Markdown document with local images as a shareable standalone file without local images or paths, while preserving the original document.
---

# Export a shareable document

Make a Markdown file readable on its own without damaging the original.

## Rules

Remove local image references, local paths in HTML media, local file links and private editor fields in front matter such as `mdview:`. Keep remote images, `data:` images, remote and anchor links, and everything else. Never change fenced code or inline code: image syntax there is an example. Match backtick and tilde fences separately, including longer fences.

## Images: ask first

Discard the references, or keep placeholders with their alt text. Follow an explicit preference, else project settings; with neither, use placeholders and say so at delivery.

## Preview, then write

Before writing, list every removed line with its line number, text and reason; write after the user confirms. No vague "cleaned up a few places".

## Keep the formatting

Work line by line; do not parse and reserialize the document. Keep blank lines, indentation and table pipe alignment. Add no export attribution.

## Final checks

1. Code blocks match the original exactly.
2. Every removal has a specific reason.
3. No absolute paths, drive letters or `file://` remain outside code examples.
4. Remote links and `data:` images are unchanged.
