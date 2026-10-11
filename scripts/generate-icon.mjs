import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from '@playwright/test';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const assets = path.join(root, 'assets');
const sizes = [16, 24, 32, 48, 64, 128, 256];
const browser = await chromium.launch({ channel: 'chrome' });
try {
  const page = await browser.newPage();
  await page.goto(pathToFileURL(path.join(assets, 'app-icon.svg')).href);
  const images = [];
  for (const size of [...sizes, 512]) {
    await page.setViewportSize({ width: size, height: size });
    await page.evaluate(size => {
      document.documentElement.setAttribute('width', String(size));
      document.documentElement.setAttribute('height', String(size));
    }, size);
    const png = await page.screenshot({ omitBackground: true });
    if (size === 512) fs.writeFileSync(path.join(assets, 'app-icon.png'), png);
    else images.push(png);
  }
  const directory = Buffer.alloc(6 + 16 * images.length);
  directory.writeUInt16LE(1, 2);
  directory.writeUInt16LE(images.length, 4);
  let offset = directory.length;
  images.forEach((image, index) => {
    const entry = 6 + index * 16;
    directory[entry] = sizes[index] % 256;
    directory[entry + 1] = sizes[index] % 256;
    directory.writeUInt16LE(1, entry + 4);
    directory.writeUInt16LE(32, entry + 6);
    directory.writeUInt32LE(image.length, entry + 8);
    directory.writeUInt32LE(offset, entry + 12);
    offset += image.length;
  });
  fs.writeFileSync(path.join(assets, 'app-icon.ico'), Buffer.concat([directory, ...images]));
  console.log(`Generated app-icon.ico (${sizes.join(', ')} px) and app-icon.png`);
} finally {
  await browser.close();
}
