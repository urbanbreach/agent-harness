#!/usr/bin/env node
// Fail-closed pixel parity comparator for TUI reference evidence (Wave T06).
//
// Usage:
//   node scripts/tui-parity/compare-pixels.mjs --self-test
//   node scripts/tui-parity/compare-pixels.mjs --reference <dir-or.png> --actual <dir-or.png>
//   node scripts/tui-parity/compare-pixels.mjs a.png b.png
//
// Rules (fail-closed, exit nonzero on any of these):
//   - missing PNG path
//   - dimension mismatch (no cropping)
//   - any unapproved RGBA difference (zero tolerance; no SSIM / similarity pass)
//
// Optional:
//   --mask <field-mask-registry.json>  field-level approved pixel regions only
//   --report <path.json>               write pixel-diff report JSON
//   --diff-png <path.png>              highlight mismatches (magenta)
//
// Evidence dirs resolve to <dir>/terminal.png when a directory is given.
// Mask registry is field-level (identity/dynamic fields), not layout geometry masks.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { deflateSync, inflateSync } from "node:zlib";

const HELP = `compare-pixels — fail-closed RGBA pixel parity (zero tolerance)

Usage:
  node scripts/tui-parity/compare-pixels.mjs --self-test
  node scripts/tui-parity/compare-pixels.mjs --reference <evidence-dir|png> --actual <evidence-dir|png>
  node scripts/tui-parity/compare-pixels.mjs <reference.png> <actual.png>

Options:
  --mask <json>       Optional field-level mask registry (approved field pixel regions)
  --report <json>     Write pixel-diff report JSON
  --diff-png <png>    Write optional diff PNG (mismatches highlighted)
  --help              Show this help

Exit codes:
  0  exact match outside approved field masks
  1  missing PNG, size mismatch, or any unapproved RGBA difference
`;

function parseArgs(argv) {
  const args = { positional: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") {
      args.help = true;
      continue;
    }
    if (arg === "--self-test") {
      args.selfTest = true;
      continue;
    }
    if (
      arg === "--reference" ||
      arg === "--actual" ||
      arg === "--mask" ||
      arg === "--report" ||
      arg === "--diff-png"
    ) {
      const next = argv[i + 1];
      if (!next) throw new Error(`missing value for ${arg}`);
      i += 1;
      if (arg === "--reference") args.reference = next;
      else if (arg === "--actual") args.actual = next;
      else if (arg === "--mask") args.mask = next;
      else if (arg === "--report") args.report = next;
      else args.diffPng = next;
      continue;
    }
    if (arg.startsWith("-")) throw new Error(`unknown argument: ${arg}`);
    args.positional.push(arg);
  }
  return args;
}

function resolvePngPath(input) {
  const path = resolve(input);
  if (!existsSync(path)) throw new Error(`missing path: ${path}`);
  if (statSync(path).isDirectory()) {
    const png = join(path, "terminal.png");
    if (!existsSync(png)) throw new Error(`missing PNG: ${png}`);
    return png;
  }
  return path;
}

function readU32(buf, offset) {
  return buf.readUInt32BE(offset);
}

function paeth(a, b, c) {
  const p = a + b - c;
  const pa = Math.abs(p - a);
  const pb = Math.abs(p - b);
  const pc = Math.abs(p - c);
  if (pa <= pb && pa <= pc) return a;
  if (pb <= pc) return b;
  return c;
}

/** Minimal 8-bit RGB/RGBA non-interlaced PNG decoder (Node zlib only). */
function decodePngRgba(filePath) {
  const bytes = readFileSync(filePath);
  const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  if (bytes.length < 8 || !bytes.subarray(0, 8).equals(sig)) {
    throw new Error(`not a PNG: ${filePath}`);
  }

  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  const idat = [];
  let offset = 8;

  while (offset + 8 <= bytes.length) {
    const length = readU32(bytes, offset);
    const type = bytes.toString("ascii", offset + 4, offset + 8);
    const dataStart = offset + 8;
    const dataEnd = dataStart + length;
    if (dataEnd + 4 > bytes.length) throw new Error(`truncated PNG chunk ${type} in ${filePath}`);
    const data = bytes.subarray(dataStart, dataEnd);

    if (type === "IHDR") {
      width = readU32(data, 0);
      height = readU32(data, 4);
      bitDepth = data[8];
      colorType = data[9];
      const interlace = data[12];
      if (bitDepth !== 8) throw new Error(`unsupported PNG bit depth ${bitDepth} in ${filePath}`);
      if (colorType !== 2 && colorType !== 6) {
        throw new Error(`unsupported PNG color type ${colorType} in ${filePath} (need RGB or RGBA)`);
      }
      if (interlace !== 0) throw new Error(`interlaced PNG not supported: ${filePath}`);
    } else if (type === "IDAT") {
      idat.push(Buffer.from(data));
    } else if (type === "IEND") {
      break;
    }
    offset = dataEnd + 4;
  }

  if (width <= 0 || height <= 0) throw new Error(`invalid PNG dimensions in ${filePath}`);
  if (idat.length === 0) throw new Error(`no IDAT in ${filePath}`);

  const inflated = inflateSync(Buffer.concat(idat));
  const channels = colorType === 6 ? 4 : 3;
  const stride = width * channels;
  const expected = height * (1 + stride);
  if (inflated.length < expected) {
    throw new Error(`PNG inflate short (${inflated.length} < ${expected}) in ${filePath}`);
  }

  const rgba = new Uint8Array(width * height * 4);
  let src = 0;
  let dst = 0;
  let prev = new Uint8Array(stride);
  for (let y = 0; y < height; y += 1) {
    const filter = inflated[src];
    src += 1;
    const row = inflated.subarray(src, src + stride);
    src += stride;
    const recon = new Uint8Array(stride);
    for (let i = 0; i < stride; i += 1) {
      const x = row[i];
      const left = i >= channels ? recon[i - channels] : 0;
      const up = prev[i];
      const upLeft = i >= channels ? prev[i - channels] : 0;
      let v;
      switch (filter) {
        case 0:
          v = x;
          break;
        case 1:
          v = (x + left) & 0xff;
          break;
        case 2:
          v = (x + up) & 0xff;
          break;
        case 3:
          v = (x + ((left + up) >> 1)) & 0xff;
          break;
        case 4:
          v = (x + paeth(left, up, upLeft)) & 0xff;
          break;
        default:
          throw new Error(`unsupported PNG filter ${filter} in ${filePath}`);
      }
      recon[i] = v;
    }
    if (channels === 4) {
      rgba.set(recon, dst);
      dst += stride;
    } else {
      for (let i = 0; i < width; i += 1) {
        const o = i * 3;
        rgba[dst++] = recon[o];
        rgba[dst++] = recon[o + 1];
        rgba[dst++] = recon[o + 2];
        rgba[dst++] = 255;
      }
    }
    prev = recon;
  }

  return { width, height, data: rgba };
}

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i += 1) {
    c ^= buf[i];
    for (let k = 0; k < 8; k += 1) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
  }
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, "ascii");
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])) >>> 0, 0);
  return Buffer.concat([len, typeBuf, data, crcBuf]);
}

function encodePngRgba(image) {
  const { width, height, data } = image;
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) {
    const rowStart = y * (stride + 1);
    raw[rowStart] = 0;
    Buffer.from(data.buffer, data.byteOffset + y * stride, stride).copy(raw, rowStart + 1);
  }
  const compressed = deflateSync(raw);
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdrData = Buffer.alloc(13);
  ihdrData.writeUInt32BE(width, 0);
  ihdrData.writeUInt32BE(height, 4);
  ihdrData[8] = 8;
  ihdrData[9] = 6;
  return Buffer.concat([
    signature,
    chunk("IHDR", ihdrData),
    chunk("IDAT", compressed),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function loadApprovedMask(maskPath, width, height) {
  if (!maskPath) return null;
  const abs = resolve(maskPath);
  if (!existsSync(abs)) throw new Error(`missing mask registry: ${abs}`);
  const registry = JSON.parse(readFileSync(abs, "utf8"));
  if (!registry || typeof registry !== "object") throw new Error("mask registry must be a JSON object");
  const fields = Array.isArray(registry.fields) ? registry.fields : [];
  const mask = new Array(width * height).fill(false);
  for (const field of fields) {
    if (!field || field.approved !== true) continue;
    const pixels = Array.isArray(field.pixels) ? field.pixels : [];
    for (const rect of pixels) {
      if (!rect || typeof rect !== "object") continue;
      const x0 = Number(rect.x) | 0;
      const y0 = Number(rect.y) | 0;
      const w = Number(rect.w) | 0;
      const h = Number(rect.h) | 0;
      if (w <= 0 || h <= 0) continue;
      for (let y = y0; y < y0 + h && y < height; y += 1) {
        if (y < 0) continue;
        for (let x = x0; x < x0 + w && x < width; x += 1) {
          if (x < 0) continue;
          mask[y * width + x] = true;
        }
      }
    }
  }
  return mask;
}

function compareRgba(reference, actual, approvedMask) {
  if (reference.width !== actual.width || reference.height !== actual.height) {
    return {
      ok: false,
      reason: "dimension_mismatch",
      width: reference.width,
      height: reference.height,
      actualWidth: actual.width,
      actualHeight: actual.height,
      mismatchCount: 0,
      totalPixels: 0,
      approvedSkipped: 0,
      firstMismatches: [],
      diffData: null,
    };
  }

  const { width, height } = reference;
  const totalPixels = width * height;
  let mismatchCount = 0;
  let approvedSkipped = 0;
  const firstMismatches = [];
  let diffData = null;

  for (let i = 0; i < totalPixels; i += 1) {
    if (approvedMask && approvedMask[i]) {
      approvedSkipped += 1;
      continue;
    }
    const o = i * 4;
    const same =
      reference.data[o] === actual.data[o] &&
      reference.data[o + 1] === actual.data[o + 1] &&
      reference.data[o + 2] === actual.data[o + 2] &&
      reference.data[o + 3] === actual.data[o + 3];
    if (!same) {
      mismatchCount += 1;
      if (firstMismatches.length < 32) {
        firstMismatches.push({
          x: i % width,
          y: Math.floor(i / width),
          reference: [
            reference.data[o],
            reference.data[o + 1],
            reference.data[o + 2],
            reference.data[o + 3],
          ],
          actual: [actual.data[o], actual.data[o + 1], actual.data[o + 2], actual.data[o + 3]],
        });
      }
      if (!diffData) diffData = new Uint8Array(reference.data);
      diffData[o] = 255;
      diffData[o + 1] = 0;
      diffData[o + 2] = 255;
      diffData[o + 3] = 255;
    }
  }

  return {
    ok: mismatchCount === 0,
    reason: mismatchCount === 0 ? "exact_match" : "rgba_mismatch",
    width,
    height,
    actualWidth: width,
    actualHeight: height,
    mismatchCount,
    totalPixels,
    approvedSkipped,
    firstMismatches,
    diffData,
  };
}

function writeArtifacts(report, reportPath, diffPngPath, diffData, width, height) {
  if (reportPath) {
    const abs = resolve(reportPath);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, `${JSON.stringify(report, null, 2)}\n`, "utf8");
  }
  if (diffPngPath && diffData) {
    const abs = resolve(diffPngPath);
    mkdirSync(dirname(abs), { recursive: true });
    writeFileSync(abs, encodePngRgba({ width, height, data: diffData }));
  }
}

function sha256File(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function comparePaths(referenceInput, actualInput, opts) {
  const referencePng = resolvePngPath(referenceInput);
  const actualPng = resolvePngPath(actualInput);
  const reference = decodePngRgba(referencePng);
  const actual = decodePngRgba(actualPng);
  const approvedMask =
    reference.width === actual.width && reference.height === actual.height
      ? loadApprovedMask(opts.mask, reference.width, reference.height)
      : null;
  const result = compareRgba(reference, actual, approvedMask);

  const report = {
    schema_version: "tui-parity-pixel-diff-v1",
    criterion: "zero_unapproved_rgba",
    pass: result.ok,
    reason: result.reason,
    reference: {
      path: referencePng,
      width: result.width,
      height: result.height,
      sha256: sha256File(referencePng),
    },
    actual: {
      path: actualPng,
      width: result.actualWidth,
      height: result.actualHeight,
      sha256: sha256File(actualPng),
    },
    mismatchCount: result.mismatchCount,
    totalPixels: result.totalPixels,
    approvedSkipped: result.approvedSkipped,
    firstMismatches: result.firstMismatches,
    mask: opts.mask ? resolve(opts.mask) : null,
    ssim_or_similarity_used: false,
    cropped: false,
  };

  writeArtifacts(
    report,
    opts.report,
    opts.diffPng,
    result.diffData,
    result.width || reference.width,
    result.height || reference.height,
  );

  if (!result.ok) {
    const detail =
      result.reason === "dimension_mismatch"
        ? `dimension mismatch: reference ${result.width}x${result.height} vs actual ${result.actualWidth}x${result.actualHeight}`
        : `${result.mismatchCount} unapproved RGBA pixel difference(s) of ${result.totalPixels}`;
    throw new Error(`pixel parity FAIL: ${detail}`);
  }

  process.stdout.write(
    `pixel parity PASS: ${result.width}x${result.height} exact RGBA` +
      (result.approvedSkipped ? ` (${result.approvedSkipped} approved field pixels skipped)` : "") +
      `\n`,
  );
  return report;
}

function selfTest() {
  // Pure PNG path — no browser/Chrome.
  const a = {
    width: 4,
    height: 2,
    data: Uint8Array.from([
      10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255, 1, 2, 3, 255, 4, 5, 6,
      255, 7, 8, 9, 255, 11, 12, 13, 255,
    ]),
  };
  const identical = { width: a.width, height: a.height, data: new Uint8Array(a.data) };
  const oneOff = { width: a.width, height: a.height, data: new Uint8Array(a.data) };
  oneOff.data[4] = (oneOff.data[4] + 1) & 0xff;

  const same = compareRgba(a, identical, null);
  if (!same.ok || same.mismatchCount !== 0) throw new Error("self-test: identical PNGs must pass");

  const diff = compareRgba(a, oneOff, null);
  if (diff.ok || diff.mismatchCount !== 1) {
    throw new Error(`self-test: 1-pixel difference must fail (got ok=${diff.ok} count=${diff.mismatchCount})`);
  }

  const small = { width: 2, height: 2, data: new Uint8Array(2 * 2 * 4) };
  const dim = compareRgba(a, small, null);
  if (dim.ok || dim.reason !== "dimension_mismatch") {
    throw new Error("self-test: dimension mismatch must fail closed");
  }

  const tmpDir = resolve("scripts/tui-parity/.self-test-tmp");
  mkdirSync(tmpDir, { recursive: true });
  const p1 = join(tmpDir, "a.png");
  const p2 = join(tmpDir, "b.png");
  const p3 = join(tmpDir, "diff.png");
  writeFileSync(p1, encodePngRgba(a));
  writeFileSync(p2, encodePngRgba(identical));
  comparePaths(p1, p2, { report: join(tmpDir, "pass-report.json") });

  writeFileSync(p2, encodePngRgba(oneOff));
  let failed = false;
  try {
    comparePaths(p1, p2, { report: join(tmpDir, "fail-report.json"), diffPng: p3 });
  } catch {
    failed = true;
  }
  if (!failed) throw new Error("self-test: path compare must fail on 1-pixel difference");
  if (!existsSync(p3)) throw new Error("self-test: diff PNG must be written on mismatch");

  const maskPath = join(tmpDir, "mask.json");
  writeFileSync(
    maskPath,
    JSON.stringify({
      schema_version: "tui-parity-field-mask-v1",
      fields: [
        {
          field_id: "identity_probe",
          kind: "identity",
          approved: true,
          pixels: [{ x: 1, y: 0, w: 1, h: 1 }],
        },
      ],
    }),
  );
  comparePaths(p1, p2, { mask: maskPath, report: join(tmpDir, "masked-pass-report.json") });

  process.stdout.write(
    "self-test PASS: identical match; 1-pixel fail-closed; dimension mismatch; field mask; pure PNG (no browser)\n",
  );
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  if (args.help) {
    process.stdout.write(HELP);
    return;
  }
  if (args.selfTest) {
    selfTest();
    return;
  }

  let reference = args.reference;
  let actual = args.actual;
  if (!reference && !actual && args.positional.length === 2) {
    reference = args.positional[0];
    actual = args.positional[1];
  }
  if (!reference || !actual) {
    throw new Error("require --reference and --actual (or two positional PNG/dir paths); see --help");
  }

  comparePaths(reference, actual, {
    mask: args.mask,
    report: args.report,
    diffPng: args.diffPng,
  });
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exit(1);
}
