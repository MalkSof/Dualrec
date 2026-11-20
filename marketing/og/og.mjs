// Genera las imágenes para compartir (1200×630) de la landing, en español e inglés.
// Uso: node og.mjs   (usa puppeteer-core de ../reel; correr `npm i` ahí antes)
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.join(here, '../reel/package.json'));
const puppeteer = require('puppeteer-core');
const chrome = process.env.CHROME || 'C:/Program Files/Google/Chrome/Application/chrome.exe';

const TEXT = {
  es: { h: 'Grabá tu pantalla y tu cámara <em>por separado</em>', s: 'Una grabación, cuatro archivos sincronizados', b: 'Gratis para Windows' },
  en: { h: 'Record your screen and webcam <em>as separate files</em>', s: 'One recording, four synced files', b: 'Free for Windows' },
};
const LOGO = '<svg viewBox="0 0 64 64" width="92" height="92"><defs><linearGradient id="s" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#8b7cff"/><stop offset="1" stop-color="#4da3ff"/></linearGradient><linearGradient id="c" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff6b81"/><stop offset="1" stop-color="#ff3d57"/></linearGradient></defs><rect x="4" y="10" width="44" height="32" rx="7" fill="url(#s)"/><circle cx="44" cy="42" r="16" fill="#0b0d12"/><circle cx="44" cy="42" r="12.5" fill="url(#c)"/><circle cx="44" cy="42" r="4.5" fill="#fff"/></svg>';

const html = t => `<!doctype html><meta charset="utf-8"><style>
*{box-sizing:border-box;margin:0}
body{width:1200px;height:630px;overflow:hidden;background:#0b0d12;color:#f3f5fb;font-family:"Segoe UI",sans-serif;padding:56px 64px;position:relative}
.g{position:absolute;border-radius:50%;filter:blur(110px);opacity:.55}
.g1{width:620px;height:620px;background:#5b4bff;left:-200px;top:-260px}.g2{width:560px;height:560px;background:#ff3d57;right:-200px;bottom:-280px}
.top{display:flex;align-items:center;gap:20px;position:relative}
.top b{font-size:60px;font-weight:900;letter-spacing:-2px}
.top span{margin-left:auto;font-size:28px;font-weight:700;border:2px solid rgba(255,255,255,.25);border-radius:99px;padding:10px 26px 13px;background:rgba(21,25,36,.7)}
h1{position:relative;font-size:74px;line-height:1.06;font-weight:900;letter-spacing:-2.5px;margin-top:44px;max-width:1040px}
em{font-style:normal;background:linear-gradient(100deg,#a99dff,#4da3ff 55%,#6ee7ff);-webkit-background-clip:text;color:transparent}
p{position:relative;font-size:32px;color:#b6bed3;margin-top:20px}
.f{position:absolute;left:64px;right:64px;bottom:54px;display:flex;gap:16px}
.f div{flex:1;background:rgba(21,25,36,.92);border:2px solid #2a3145;border-radius:20px;padding:18px 0 22px;text-align:center;font:700 30px Consolas,monospace}
</style><div class="g g1"></div><div class="g g2"></div>
<div class="top">${LOGO}<b>Dualrec</b><span>${t.b}</span></div>
<h1>${t.h}</h1><p>${t.s}</p>
<div class="f"><div>screen.mp4</div><div>camera.mp4</div><div>mic.m4a</div><div>system.m4a</div></div>`;

const browser = await puppeteer.launch({ executablePath: chrome, headless: true });
const page = await browser.newPage();
await page.setViewport({ width: 1200, height: 630, deviceScaleFactor: 1 });
for (const [lang, t] of Object.entries(TEXT)) {
  await page.setContent(html(t));
  await page.evaluate(() => document.fonts.ready);
  await page.screenshot({ path: path.join(here, `../../apps/landing/assets/og-${lang}.png`), type: 'png' });
  console.log('og-' + lang + '.png');
}
await browser.close();
