# Third-party notices

MDWisp's own code is MIT-licensed (see `LICENSE`). The fonts below are bundled in the installer
(`fonts` folder next to `mdwisp.exe`) and embedded, cut down to the characters used, in exported
HTML and PDF. They are fetched by `scripts/fonts.ps1` from the pinned URLs and SHA-256 sums listed
there; license texts sit beside the font files in `crates/export/assets/fonts`.

| Font | Used for | Version | License | Copyright | License text |
|---|---|---|---|---|---|
| Noto Serif SC (variable) | Body and headings, CJK | notofonts/noto-cjk Serif2.003 | SIL OFL 1.1 | 2017-2024 Adobe | `OFL-NotoSerifSC.txt` |
| LXGW WenKai | Blockquotes and CJK emphasis | lxgw/LxgwWenKai v1.530 | SIL OFL 1.1 | 2021-2026 LXGW; 2020 The Klee Project Authors | `OFL-LXGWWenKai.txt` |
| Latin Modern Roman 10 (regular, bold, italic, bold italic) | Latin text | GUST lm 2.004 (OpenType) | GUST Font License (LPPL-based) | 2003, 2009 B. Jackowski and J. M. Nowacki | `GUST-FONT-LICENSE-LatinModern.txt` |

Exported HTML embeds subsets of Noto Serif SC and LXGW WenKai. The OFL permits this; the Reserved
Font Name clause of LXGW WenKai is respected because the subsets keep the original family and are
embedded in the document only, never installed as desktop fonts.

Consolas, Microsoft YaHei UI, Sitka and SimSun are Windows system fonts and are not redistributed.
