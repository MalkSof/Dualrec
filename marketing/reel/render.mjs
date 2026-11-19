// Renderiza reel.html cuadro por cuadro con Chrome y lo une con audio.wav.
// Uso: node render.mjs [segundo]   (con un segundo, guarda solo esa captura en frames/)
import puppeteer from 'puppeteer-core';
import { spawn } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';
import path from 'node:path';
import fs from 'node:fs';

const here = path.dirname(fileURLToPath(import.meta.url));
const FPS = 30, DUR = 19;
const ffmpeg = path.resolve(here, '../../apps/desktop/src-tauri/binaries/ffmpeg-x86_64-pc-windows-gnu.exe');
const chrome = process.env.CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';

const browser = await puppeteer.launch({ executablePath: chrome, headless: true, args: ['--hide-scrollbars', '--force-device-scale-factor=1'] });
const page = await browser.newPage();
await page.setViewport({ width: 1080, height: 1920, deviceScaleFactor: 1 });
await page.goto(pathToFileURL(path.join(here, 'reel.html')).href);
await page.evaluate(() => document.fonts.ready);

const stills = process.argv.slice(2).map(Number);
if (stills.length) {
  fs.mkdirSync(path.join(here, 'frames'), { recursive: true });
  for (const t of stills) {
    await page.evaluate(t => window.renderAt(t), t);
    await page.screenshot({ path: path.join(here, 'frames', `t${t.toFixed(2)}.jpg`), type: 'jpeg', quality: 80 });
  }
  await browser.close();
  process.exit(0);
}

const out = path.join(here, 'dualrec-reel.mp4');
const ff = spawn(ffmpeg, ['-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(FPS), '-c:v', 'png', '-i', '-',
  '-i', path.join(here, 'audio.wav'), '-c:v', 'libx264', '-preset', 'slow', '-crf', '17', '-pix_fmt', 'yuv420p',
  '-color_primaries', 'bt709', '-color_trc', 'bt709', '-colorspace', 'bt709',
  '-c:a', 'aac', '-b:a', '256k', '-ar', '48000', '-t', String(DUR), '-movflags', '+faststart', out], { stdio: ['pipe', 'inherit', 'inherit'] });
const done = new Promise(r => ff.on('close', r));
for (let i = 0; i < FPS * DUR; i++) {
  await page.evaluate(t => window.renderAt(t), i / FPS);
  const png = await page.screenshot({ type: 'png', optimizeForSpeed: true });
  if (!ff.stdin.write(png)) await new Promise(r => ff.stdin.once('drain', r));
  if (i % 60 === 0) console.log(`cuadro ${i}/${FPS * DUR}`);
}
ff.stdin.end();
console.log('ffmpeg salió con', await done, '→', out);
await browser.close();
