// Captures the demo: 4K stills of the real Syscura UI (demo data), caption
// overlays, title cards, and README screenshots. build.py turns them into the video.
import { chromium } from "playwright";
import { readFileSync, mkdirSync, writeFileSync } from "node:fs";

const here = new URL(".", import.meta.url);
const mock = readFileSync(new URL("./mock.js", here), "utf8");
mkdirSync(new URL("./frames", here), { recursive: true });
mkdirSync(new URL("./readme", here), { recursive: true });
const out = (name) => new URL(`./frames/${name}.png`, here).pathname.slice(1);

const browser = await chromium.launch();
const scenes = [];

// ---------------------------------------------------------------- app stills
const page = await browser.newPage({ viewport: { width: 1600, height: 900 }, deviceScaleFactor: 2.4 });
page.on("pageerror", (e) => console.log("pageerror:", e.message));
await page.addInitScript(mock);
await page.addInitScript(() => { window.__demoFixMs = 900; });
await page.goto("http://localhost:4173/");
await page.waitForTimeout(2500);

// A visible cursor (headless Chromium draws none).
async function cursor(x, y, click = false) {
  await page.evaluate(({ x, y, click }) => {
    let c = document.getElementById("demo-cursor");
    if (!c) {
      c = document.createElement("div");
      c.id = "demo-cursor";
      c.style.cssText = "position:fixed;z-index:99999;pointer-events:none;width:34px;height:34px;";
      c.innerHTML = '<svg viewBox="0 0 24 24" width="34" height="34"><path d="M4 2l15 11.5-6.6.9 3.9 7.6-2.9 1.4-3.8-7.7L4 20z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>';
      document.body.appendChild(c);
    }
    c.style.left = `${x - 5}px`;
    c.style.top = `${y - 3}px`;
    document.getElementById("demo-ripple")?.remove();
    if (click) {
      const r = document.createElement("div");
      r.id = "demo-ripple";
      r.style.cssText = `position:fixed;z-index:99998;pointer-events:none;left:${x - 26}px;top:${y - 26}px;width:52px;height:52px;border-radius:50%;background:rgba(79,70,229,.25);border:2px solid rgba(79,70,229,.7);`;
      document.body.appendChild(r);
    }
  }, { x, y, click });
}
const hideCursor = () => page.evaluate(() => { document.getElementById("demo-cursor")?.remove(); document.getElementById("demo-ripple")?.remove(); });
async function center(locator) {
  const b = await locator.boundingBox();
  return { x: b.x + b.width / 2, y: b.y + b.height / 2 };
}
/** Focus point as a fraction of the frame, for zooming toward it. */
async function focusOf(locator) {
  const c = await center(locator);
  return [c.x / 1600, c.y / 900];
}
const tab = (name) => page.getByRole("button", { name: new RegExp(`^${name}`) }).first();
async function shot(name) { await page.screenshot({ path: out(name) }); }

// 1. Overview
await shot("02-overview");
scenes.push({ img: "02-overview", dur: 3.6, zoom: [1.0, 1.08], focus: [0.33, 0.45], caption: "Finds what's really wrong with your PC" });

// README screenshots use a calmer size.
// (taken at the end from a separate page)

// 2. Fix a crashed service
await page.locator(".close").first().click(); // close the serious-problem banner
await tab("Problems").click();
await page.waitForTimeout(900);
await page.getByText("A Windows service crashed").first().click();
await page.waitForTimeout(900);
const fixRow = page.locator(".fix", { hasText: "Make sure the service is running again" }).first();
const fixBtn = fixRow.getByRole("button", { name: "Run" });
await fixBtn.scrollIntoViewIfNeeded();
await page.evaluate(() => window.scrollBy(0, 160));
await page.waitForTimeout(400);
let f = await focusOf(fixRow);
let c = await center(fixBtn);
await cursor(c.x + 60, c.y + 40);
await shot("03-problem");
await cursor(c.x, c.y, true);
await shot("04-click-fix");
await fixBtn.click();
await page.waitForTimeout(250);
await hideCursor();
await page.waitForTimeout(4600); // the page reloads problems every 4 s
const askBtn = page.getByRole("button", { name: "Ask AI if it worked" }).first();
if (await askBtn.isVisible().catch(() => false)) await askBtn.click();
await page.waitForTimeout(1500);
await shot("06-fixed");
const fAi = await focusOf(page.locator(".aicheck").first());
scenes.push({ img: "03-problem", dur: 2.3, zoom: [1.2, 1.32], focus: f, caption: "Explains it in plain words, with a safe fix" });
scenes.push({ img: "04-click-fix", dur: 0.9, zoom: [1.32, 1.36], focus: f, caption: "Explains it in plain words, with a safe fix", cut: true });
scenes.push({ img: "06-fixed", dur: 3.0, zoom: [1.42, 1.48], focus: fAi, caption: "Fixes it, then checks the fix really worked ✓" });

// 3. Security score
await page.evaluate(() => window.scrollTo(0, 0));
await tab("Security").click();
await page.waitForTimeout(1200);
const score = page.locator(".score").first();
const fw = page.getByRole("button", { name: "Turn the firewall on" }).first();
const sc = await center(score);
c = await center(fw);
f = [((sc.x + c.x) / 2) / 1600, ((sc.y + c.y) / 2) / 900];
await cursor(c.x - 40, c.y + 50);
await shot("07-security");
await cursor(c.x, c.y, true);
await shot("08-click-firewall");
await fw.click();
await hideCursor();
await page.waitForTimeout(7200); // fix runs, page checks again
await shot("09-security-after");
scenes.push({ img: "07-security", dur: 2.4, zoom: [1.12, 1.24], focus: f, caption: "Security score for Windows' own protection" });
scenes.push({ img: "08-click-firewall", dur: 0.9, zoom: [1.24, 1.27], focus: f, caption: "Security score for Windows' own protection", cut: true });
scenes.push({ img: "09-security-after", dur: 2.8, zoom: [1.27, 1.3], focus: [sc.x / 1600 - 0.2, sc.y / 900 + 0.14], caption: "One click: firewall on, score 75 → 87" });

// 4. Malware checks
await tab("Problems").click();
await page.waitForTimeout(900);
await page.getByRole("button", { name: /^Security$/ }).nth(1).click().catch(() => {});
await page.waitForTimeout(500);
await page.getByText("The hosts file sends a popular site somewhere else").first().click();
await page.waitForTimeout(900);
await page.evaluate(() => window.scrollTo(0, 0));
await shot("10-malware");
const fMal = await focusOf(page.locator("h1, h2").filter({ hasText: "The hosts file sends" }).first());
scenes.push({ img: "10-malware", dur: 3.4, zoom: [1.12, 1.26], focus: [fMal[0] - 0.08, fMal[1] + 0.12], caption: "Catches what antivirus misses: fake sites, hijacks, risky drivers" });

// 5. Drive health
await tab("Hardware").click();
await page.waitForTimeout(1200);
await page.getByRole("tab", { name: "Storage" }).click().catch(async () => { await page.getByText("Storage", { exact: true }).first().click(); });
await page.waitForTimeout(800);
const trends = page.locator(".trends").first();
await page.evaluate(() => { const pad = document.createElement("div"); pad.style.cssText = "height:700px;flex:none"; document.querySelector(".app").appendChild(pad); });
await trends.evaluate((el) => el.scrollIntoView({ block: "center" }));
await page.waitForTimeout(400);
f = await focusOf(trends);
await shot("11-drives");
scenes.push({ img: "11-drives", dur: 3.0, zoom: [1.06, 1.14], focus: [0.5, f[1] + 0.1], caption: "Tracks drive health every day, and backs up your files" });
await page.close();

// ---------------------------------------------------------------- cards and captions
const card = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 2 });
const font = (w) => `@font-face{font-family:Outfit;font-weight:${w};src:url("/fonts/outfit-latin-${w}-normal.woff2") format("woff2")}`;
const css = `${font(400)}${font(500)}${font(600)}${font(700)}
  html,body{margin:0;width:1920px;height:1080px;font-family:Outfit,sans-serif;-webkit-font-smoothing:antialiased}
  .bg{width:100%;height:100%;background:radial-gradient(1200px 700px at 30% 20%,#6366f1 0%,#4f46e5 45%,#3730a3 100%);color:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:28px;text-align:center}
  .logo{width:150px;height:150px;border-radius:36px;background:#fff;display:grid;place-items:center;box-shadow:0 30px 80px rgba(0,0,0,.25)}
  .logo img{width:104px;height:104px}
  h1{margin:0;font-size:120px;font-weight:600;letter-spacing:-.045em;line-height:1}
  p{margin:0;font-size:44px;font-weight:400;color:#e0e7ff;letter-spacing:-.01em}
  .chips{display:flex;gap:16px;margin-top:10px}
  .chip{font-size:30px;font-weight:500;padding:12px 26px;border-radius:999px;background:rgba(255,255,255,.14);border:1px solid rgba(255,255,255,.28)}
  .url{font-size:40px;font-weight:600;color:#fff;margin-top:14px}`;
const logo = "/logo.svg";
await card.goto("http://localhost:4173/");
async function cardShot(name, body) {
  await card.setContent(`<html><head><style>${css}</style></head><body>${body}</body></html>`);
  await card.waitForTimeout(600);
  await card.screenshot({ path: out(name) });
}
await cardShot("01-title", `<div class="bg"><div class="logo"><img src="${logo}"></div><h1>Syscura</h1><p>Your PC's doctor and bodyguard.</p></div>`);
await cardShot("12-outro", `<div class="bg"><div class="logo"><img src="${logo}"></div><h1>Free &amp; open source</h1>
  <div class="chips"><span class="chip">Windows 10 &amp; 11</span><span class="chip">x64 &amp; ARM</span><span class="chip">~13 MB RAM</span><span class="chip">No account</span></div>
  <div class="url">github.com/hiibrarahmad/syscura</div></div>`);
scenes.unshift({ img: "01-title", dur: 2.4, zoom: [1.0, 1.03], focus: [0.5, 0.5] });
scenes.push({ img: "12-outro", dur: 3.4, zoom: [1.03, 1.0], focus: [0.5, 0.5] });

// Captions: transparent 1920x1080 overlays, placed by build.py.
const captions = [...new Set(scenes.map((s) => s.caption).filter(Boolean))];
for (const [i, text] of captions.entries()) {
  await card.setContent(`<html><head><style>${css} html,body{background:transparent}
    .cap{position:absolute;left:50%;bottom:64px;transform:translateX(-50%);background:rgba(23,21,46,.88);color:#fff;font-size:46px;font-weight:600;letter-spacing:-.015em;padding:22px 42px;border-radius:999px;white-space:nowrap;box-shadow:0 16px 40px rgba(0,0,0,.25)}</style></head>
    <body><div class="cap">${text.replace(/&/g, "&amp;")}</div></body></html>`);
  await card.waitForTimeout(300);
  await card.screenshot({ path: out(`cap-${i}`), omitBackground: true });
  for (const s of scenes) if (s.caption === text) s.cap = `cap-${i}`;
}
await card.close();
writeFileSync(new URL("./scenes.json", here), JSON.stringify(scenes, null, 2));

// ---------------------------------------------------------------- README screenshots
const rp = await browser.newPage({ viewport: { width: 1600, height: 1000 }, deviceScaleFactor: 1.25 });
await rp.addInitScript(mock);
await rp.goto("http://localhost:4173/");
await rp.waitForTimeout(2500);
const rshot = (n) => rp.screenshot({ path: new URL(`./readme/${n}.png`, here).pathname.slice(1) });
await rshot("overview");
await rp.locator(".close").first().click();
const rtab = (name) => rp.getByRole("button", { name: new RegExp(`^${name}`) }).first();
await rtab("Problems").click(); await rp.waitForTimeout(1200); await rshot("problems");
await rtab("Security").click(); await rp.waitForTimeout(1500); await rshot("security");
await rtab("Hardware").click(); await rp.waitForTimeout(1500); await rshot("hardware");
await rtab("Events").click(); await rp.waitForTimeout(1500); await rshot("events");
await browser.close();
console.log("done", scenes.length, "scenes");
