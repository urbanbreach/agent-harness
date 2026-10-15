# Bundled fonts

Downloaded from upstream on 2026-10-14. Glyph bitmaps are unchanged; the two small fonts are subsets. Source files retain their embedded notices. In the larger BDFs, the STARTCHAR labels for U+03A0 and U+03C0 are renamed to neutral Unicode labels; encodings and pixels are unchanged. Silver is the only outline font; see its section below.

| File | Upstream source | Bundled coverage | License evidence |
| --- | --- | --- | --- |
| `5x8.bdf` | https://gitlab.freedesktop.org/xorg/font/misc-misc/-/raw/master/5x8.bdf | 191 glyphs with encodings 32 through 255; `CHARS` adjusted | Embedded `COPYRIGHT "Public domain font. Share and enjoy."` (upstream uses two spaces). Revision 1.32, 2006-01-05. |
| `6x12.bdf` | https://gitlab.freedesktop.org/xorg/font/misc-misc/-/raw/master/6x12.bdf | All 4531 upstream glyphs | Embedded `COPYRIGHT "Public domain terminal emulator font. Share and enjoy."` (two spaces upstream). Revision 1.32, 2008-06-26. |
| `8x13.bdf` | https://gitlab.freedesktop.org/xorg/font/misc-misc/-/raw/master/8x13.bdf | All 3703 upstream glyphs | Embedded public-domain notice. Revision 1.57, 2006-01-05. |
| `unscii-8.hex` | https://raw.githubusercontent.com/viznut/unscii/master/fontfiles/unscii-8.hex | 255 glyphs with encodings 1 through 255 | No license is embedded in the HEX asset. Upstream repository README expressly permits Public Domain or CC-0 for files other than the listed Unifont-derived files. Unscii-8 is not one of those exceptions. |

The X.org font headers credit Markus Kuhn as the bug-report contact. Unscii is maintained by viznut (Ville-Matias Heikkilä). The upstream Unscii licensing declaration is retained verbatim in `Unscii.README`, downloaded from https://raw.githubusercontent.com/viznut/unscii/main/README.md:

> Licensing: You can consider it Public Domain (or CC-0) except for the files
> derived from or containing parts of Roman Czyborra's Unifont project
> (unifont.hex, hex2bdf.pl, unscii-16-full.*) which fall under GPL.

This is upstream licensing evidence, not a license inferred from the HEX file. None of the listed GPL exceptions is bundled here.

## Silver

`Silver.ttf` is the Silver pixel font for games, bundled unmodified and rasterized at runtime with the `fontdue` crate.

| Field | Value |
| --- | --- |
| Source | https://poppyworks.itch.io/silver |
| Authors | Poppy Works and Wolfgang Wozniak |
| Major contributors | Itou Hiro (PixelMPlus), leedheo (DOSGothic) and ぶち (Buchidot) |
| License | Creative Commons Attribution 4.0 International (CC BY 4.0), https://creativecommons.org/licenses/by/4.0/ |
| Required attribution | Silver by Poppy Works |
| SHA-256 | `7adcf56d93142ded08ff18196f78fcaaf552411b9a94dc559dac90eb5c91cef1` |

Silver is not public domain, and the project software license does not supersede its asset license. Redistribution keeps this attribution. Silver covers Japanese kana, Chinese and Korean characters and other glyphs in its character map; coverage is exactly membership in that map (a nonzero glyph index), not a range heuristic.

The `silver16-bw` shape draws every character in one cell with Silver and always writes RGB frames. Bitmap shapes draw a character missing from their bitmap font with Silver for that character only; a character Silver also lacks leaves its cells blank, which normalization avoids by writing a placeholder instead.

SHA-256 of bundled assets:

```text
2d35ab65463d96f79a44763a81341a3a915f3c9125fb347d336263494d747207  5x8.bdf
491e040ddab699f0ef8756a67b1ab62d6793951854a56c86c890b72d66091924  unscii-8.hex
8930007841d1bd7bad0e075b7a2703dcfaa7a865ce2ab55cfbc31ffd220685be  6x12.bdf
a048341ce22fe93b363eb6ffeca4d79cc8f433dda75728c880e655b5048355e6  8x13.bdf
687095d7e32c466192a81c9b52ddeeaf99036fa2bc9ae4f818f1cb8b96096b3c  Unscii.README
7adcf56d93142ded08ff18196f78fcaaf552411b9a94dc559dac90eb5c91cef1  Silver.ttf
```

Rendering uses the `image` crate's Lanczos3 implementation for stretched cells, rather than a separate resampler. Thumbnails use triangle-filter grayscale downsampling and a luminance threshold of 240, packed MSB-first with no per-row byte padding; their dimensions are two by four dots per requested braille cell. An invalid PNG yields an all-white thumbnail because the thumbnail API is infallible. Frame rendering itself reports geometry and encoding errors.
