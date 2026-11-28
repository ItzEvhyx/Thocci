#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
const srcDesktop = path.join(root, "assets", "desktop");
const destVendor = path.join(root, "ui", "vendor");
const destTauriIcons = path.join(root, "src-tauri", "icons");

const requiredDirs = [
  "fonts",
  "icons",
  "illustrations",
  "mascot",
  "keyboard",
  path.join("brand", "tray"),
];

const requiredFiles = [
  "tokens.json",
  path.join("brand", "thocci-wordmark.svg"),
  path.join("fonts", "fonts.css"),
  path.join("fonts", "dm-sans-bold-latin.woff2"),
  path.join("fonts", "dm-sans-bold-latin-ext.woff2"),
  path.join("fonts", "inter-variable.woff2"),
  path.join("keyboard", "pt-iso.json"),
  path.join("mascot", "stage-poses.json"),
  path.join("mascot", "thocci-quick-actions.svg"),
  path.join("mascot", "thocci-excited.svg"),
  path.join("mascot", "thocci-sleeping.svg"),
  path.join("mascot", "thocci-pillow.svg"),
  path.join("mascot", "slide-trail.svg"),
  path.join("mascot", "jump-shadow.svg"),
  path.join("mascot", "sleep-marks.svg"),
  path.join("illustrations", "onboarding.svg"),
  path.join("illustrations", "stage-light.svg"),
  path.join("illustrations", "stage-dark.svg"),
  path.join("icons", "pin.svg"),
  path.join("icons", "pin-filled.svg"),
  path.join("brand", "thocci-app-icon.icns"),
  path.join("brand", "thocci-app-icon.ico"),
  path.join("brand", "thocci-app-icon.png"),
  path.join("brand", "png", "thocci-32.png"),
  path.join("brand", "png", "thocci-128.png"),
  path.join("brand", "png", "thocci-256.png"),
  path.join("brand", "png", "thocci-512.png"),
];

const PNG_SIG = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);

const CRC_TABLE = (() => {
  const tbl = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) {
      c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    }
    tbl[n] = c >>> 0;
  }
  return tbl;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

function makePngChunk(type, data) {
  const typeBuf = Buffer.from(type, "ascii");
  const lenBuf = Buffer.alloc(4);
  lenBuf.writeUInt32BE(data.length, 0);
  const crcBuf = Buffer.alloc(4);
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([lenBuf, typeBuf, data, crcBuf]);
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

/**
 * Ensure a PNG buffer is 8-bit RGBA (colorType 6), required by Tauri's icon loader.
 * Losslessly upgrades 8-bit RGB (colorType 2) PNGs by appending alpha=255.
 */
function ensureRgbaPng(buf) {
  if (!buf.subarray(0, 8).equals(PNG_SIG)) {
    throw new Error("Invalid PNG signature");
  }
  const width = buf.readUInt32BE(16);
  const height = buf.readUInt32BE(20);
  const bitDepth = buf[24];
  const colorType = buf[25];
  if (colorType === 6) {
    return buf;
  }
  if (bitDepth !== 8 || colorType !== 2) {
    throw new Error(`Unsupported PNG format (bitDepth=${bitDepth}, colorType=${colorType})`);
  }

  const idatParts = [];
  let offset = 8;
  while (offset + 8 <= buf.length) {
    const len = buf.readUInt32BE(offset);
    const type = buf.subarray(offset + 4, offset + 8).toString("ascii");
    const data = buf.subarray(offset + 8, offset + 8 + len);
    if (type === "IDAT") idatParts.push(data);
    offset += 12 + len;
    if (type === "IEND") break;
  }

  const inflated = zlib.inflateSync(Buffer.concat(idatParts));
  const srcStride = width * 3;
  const rgb = Buffer.alloc(height * srcStride);
  for (let y = 0; y < height; y++) {
    const filter = inflated[y * (srcStride + 1)];
    const rowIn = y * (srcStride + 1) + 1;
    const rowOut = y * srcStride;
    for (let x = 0; x < srcStride; x++) {
      const raw = inflated[rowIn + x];
      const a = x >= 3 ? rgb[rowOut + x - 3] : 0;
      const b = y > 0 ? rgb[rowOut - srcStride + x] : 0;
      const c = x >= 3 && y > 0 ? rgb[rowOut - srcStride + x - 3] : 0;
      let recon = raw;
      if (filter === 1) recon = (raw + a) & 0xff;
      else if (filter === 2) recon = (raw + b) & 0xff;
      else if (filter === 3) recon = (raw + ((a + b) >> 1)) & 0xff;
      else if (filter === 4) recon = (raw + paeth(a, b, c)) & 0xff;
      rgb[rowOut + x] = recon;
    }
  }

  const dstStride = width * 4;
  const filteredRgba = Buffer.alloc(height * (dstStride + 1));
  for (let y = 0; y < height; y++) {
    const outRow = y * (dstStride + 1);
    filteredRgba[outRow] = 0; // Filter None
    for (let x = 0; x < width; x++) {
      const s = y * srcStride + x * 3;
      const d = outRow + 1 + x * 4;
      filteredRgba[d] = rgb[s];
      filteredRgba[d + 1] = rgb[s + 1];
      filteredRgba[d + 2] = rgb[s + 2];
      filteredRgba[d + 3] = 255;
    }
  }

  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  ihdr[10] = 0;
  ihdr[11] = 0;
  ihdr[12] = 0;

  const compressed = zlib.deflateSync(filteredRgba, { level: 9 });
  return Buffer.concat([
    PNG_SIG,
    makePngChunk("IHDR", ihdr),
    makePngChunk("IDAT", compressed),
    makePngChunk("IEND", Buffer.alloc(0)),
  ]);
}

function copyRecursive(src, dst) {
  const stat = fs.statSync(src);
  if (stat.isDirectory()) {
    fs.mkdirSync(dst, { recursive: true });
    for (const entry of fs.readdirSync(src)) {
      copyRecursive(path.join(src, entry), path.join(dst, entry));
    }
  } else {
    fs.mkdirSync(path.dirname(dst), { recursive: true });
    fs.copyFileSync(src, dst);
  }
}

for (const rel of requiredFiles) {
  const full = path.join(srcDesktop, rel);
  if (!fs.existsSync(full) || fs.statSync(full).size === 0) {
    throw new Error(`Missing or empty bundled desktop asset: ${rel}`);
  }
}

fs.rmSync(destVendor, { recursive: true, force: true });
fs.mkdirSync(destVendor, { recursive: true });

for (const relDir of requiredDirs) {
  copyRecursive(path.join(srcDesktop, relDir), path.join(destVendor, relDir));
}
for (const relFile of ["tokens.json", path.join("brand", "thocci-wordmark.svg")]) {
  fs.copyFileSync(path.join(srcDesktop, relFile), path.join(destVendor, relFile));
}

// Sync official ESC app icons into src-tauri/icons as RGBA PNGs + ICNS + ICO.
fs.mkdirSync(destTauriIcons, { recursive: true });
const pngIcons = [
  [path.join("brand", "png", "thocci-32.png"), "32x32.png"],
  [path.join("brand", "png", "thocci-128.png"), "128x128.png"],
  [path.join("brand", "png", "thocci-256.png"), "128x128@2x.png"],
  [path.join("brand", "png", "thocci-512.png"), "icon.png"],
];
for (const [srcRel, dstName] of pngIcons) {
  const rgba = ensureRgbaPng(fs.readFileSync(path.join(srcDesktop, srcRel)));
  fs.writeFileSync(path.join(destTauriIcons, dstName), rgba);
}
fs.copyFileSync(
  path.join(srcDesktop, "brand", "thocci-app-icon.ico"),
  path.join(destTauriIcons, "icon.ico"),
);
fs.copyFileSync(
  path.join(srcDesktop, "brand", "thocci-app-icon.icns"),
  path.join(destTauriIcons, "icon.icns"),
);

// Verify keyboard JSON has exactly 79 keys.
const kb = JSON.parse(
  fs.readFileSync(path.join(destVendor, "keyboard", "pt-iso.json"), "utf8"),
);
if (!Array.isArray(kb.keys) || kb.keys.length !== 79) {
  throw new Error(`Expected 79 keys in pt-iso.json, got ${kb.keys?.length}`);
}

console.log(
  `Prepared Thocci UI assets (${kb.keys.length} keyboard keys, fonts, icons, mascot, illustrations, RGBA brand icons).`,
);
