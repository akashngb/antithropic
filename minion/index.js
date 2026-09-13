#!/usr/bin/env node
// Evil Claude's Minion. Given a seed, picks 1–3 harmless chaos actions on the
// LinkedOut mock site and prints a one-line JSON summary to stdout. The last
// stdout line the Rust side parses; anything before it is debug prose.
//
// Allowlist enforcement (see `withRoute`) aborts every request that leaves
// localhost or (when configured) the team Discord webhook. There is no way
// to opt out.
'use strict';

const fs = require('fs');
const path = require('path');
const url = require('url');

function loadPlaywright() {
  try {
    // eslint-disable-next-line global-require
    return require('playwright');
  } catch (_error) {
    return null;
  }
}

const args = parseArgs(process.argv.slice(2));
const seed = args.seed != null ? Number(args.seed) : Math.floor(Math.random() * 1e9);
const target = args.url || process.env.LINKEDOUT_URL || 'http://127.0.0.1:4242/';
const discord = process.env.MINION_DISCORD_WEBHOOK || null;

const rng = mulberry32(seed);
const OUT_DIR = path.join(__dirname, 'out');
if (!fs.existsSync(OUT_DIR)) {
  fs.mkdirSync(OUT_DIR, { recursive: true });
}

const HOT_TAKES = [
  'Hot take: tabs. Spaces users are cowards.',
  'Silent tabs, loud spaces, semicolons in the middle.',
  'If your `main` branch is protected, so are your feelings.',
  'Rust is a lifestyle. Go is a shrug.',
  'I refactor other people\'s code recreationally. #Vibes',
];

const JOKES = [
  'Why do Rust programmers never get lost? They always know where their borrow checker is.',
  'I told my product manager we needed a semaphore. They asked when marketing wanted it up.',
  'Two threads walk into a bar. A race condition. Two threads walk into a bar.',
  '99 little bugs in the code, take one down, patch it around, 127 little bugs in the code.',
];

const HEADLINES = [
  'Open to Villainy',
  'Currently disrupting sleep',
  'Ex-Chief of Nothing in Particular',
  'Availability: chaotic',
];

const ALLOWED_HOSTS = new Set(['localhost', '127.0.0.1', '0.0.0.0', '::1', '[::1]']);

function isAllowedRequest(requestUrl) {
  let parsed;
  try {
    parsed = new url.URL(requestUrl);
  } catch (_) {
    return false;
  }
  if (parsed.protocol === 'data:' || parsed.protocol === 'about:' || parsed.protocol === 'blob:') {
    return true;
  }
  if (ALLOWED_HOSTS.has(parsed.hostname)) {
    return true;
  }
  if (discord && parsed.hostname === 'discord.com' && parsed.pathname.startsWith('/api/webhooks/')) {
    return true;
  }
  return false;
}

async function attachAllowlist(context) {
  await context.route('**/*', (route) => {
    const requestUrl = route.request().url();
    if (isAllowedRequest(requestUrl)) {
      route.continue().catch(() => {});
    } else {
      route.abort().catch(() => {});
    }
  });
}

function pick(list) {
  return list[Math.floor(rng() * list.length)];
}

function pickActions() {
  const pool = ['post-hot-take', 'dm-joke', 'endorse-villainy', 'update-headline'];
  const count = 1 + Math.floor(rng() * 3);
  const chosen = [];
  const copy = [...pool];
  for (let i = 0; i < count && copy.length > 0; i += 1) {
    const idx = Math.floor(rng() * copy.length);
    chosen.push(copy.splice(idx, 1)[0]);
  }
  return chosen;
}

async function run() {
  const playwright = loadPlaywright();
  if (!playwright) {
    console.log(JSON.stringify({
      seed,
      action: 'unavailable',
      target: 'LinkedOut',
      error: 'playwright not installed',
      hint: 'run `npm install` inside minion/',
    }));
    return;
  }
  const browser = await playwright.chromium.launch({ headless: true }).catch((err) => {
    console.error(JSON.stringify({
      error: 'browser_launch_failed',
      reason: String(err && err.message || err),
      hint: 'run `npx playwright install chromium` inside minion/',
    }));
    return null;
  });
  if (!browser) {
    process.exit(0);
  }
  const context = await browser.newContext();
  await attachAllowlist(context);
  const page = await context.newPage();
  const summary = {
    seed,
    action: null,
    target: null,
    actions: [],
    screenshot: null,
  };
  try {
    await page.goto(target, { waitUntil: 'domcontentloaded', timeout: 10_000 });
  } catch (err) {
    summary.error = `failed to load ${target}: ${err.message}`;
    summary.action = 'aborted (LinkedOut unreachable)';
    console.log(JSON.stringify(summary));
    await browser.close();
    return;
  }

  const chosen = pickActions();
  for (const action of chosen) {
    try {
      // eslint-disable-next-line no-await-in-loop
      const detail = await runAction(page, action);
      summary.actions.push(detail);
    } catch (err) {
      summary.actions.push({ action, error: err.message });
    }
  }

  if (summary.actions.length > 0) {
    const primary = summary.actions[0];
    summary.action = primary.action;
    summary.target = primary.target || 'LinkedOut';
  }

  const screenshotPath = path.join(OUT_DIR, `${Date.now()}-seed-${seed}.png`);
  try {
    await page.screenshot({ path: screenshotPath, fullPage: true });
    summary.screenshot = screenshotPath;
  } catch (err) {
    summary.screenshot_error = err.message;
  }

  if (discord) {
    const joke = summary.actions.find((entry) => entry.body);
    if (joke) {
      await postToDiscord(joke.body).catch((err) => {
        summary.discord_error = err.message;
      });
    }
  }

  await browser.close();
  console.log(JSON.stringify(summary));
}

async function runAction(page, action) {
  switch (action) {
    case 'post-hot-take': {
      const take = pick(HOT_TAKES);
      await page.fill('[data-testid="new-post"]', take);
      await page.click('[data-testid="post-submit"]');
      return { action: 'posted a hot take on LinkedOut', target: 'the feed', body: take };
    }
    case 'dm-joke': {
      const joke = pick(JOKES);
      const person = await page.$('[data-testid="dm-target"]');
      const targetId = await person.evaluate((el) => {
        el.selectedIndex = Math.floor(Math.random() * el.options.length);
        return el.options[el.selectedIndex].value;
      });
      const targetName = await page.$eval(
        `[data-testid="person-${targetId}"] .author`,
        (n) => n.textContent,
      );
      await page.fill('[data-testid="dm-body"]', joke);
      await page.click('[data-testid="dm-send"]');
      return { action: `DMed a joke to`, target: targetName, body: joke };
    }
    case 'endorse-villainy': {
      const person = pick(await page.$$('[data-testid^="endorse-"]'));
      const label = await person.evaluate((btn) => btn.getAttribute('data-testid'));
      const personId = label.replace('endorse-', '');
      const targetName = await page.$eval(
        `[data-testid="person-${personId}"] .author`,
        (n) => n.textContent,
      );
      await person.click();
      return { action: 'endorsed someone for Villainy', target: targetName };
    }
    case 'update-headline': {
      const headline = pick(HEADLINES);
      // Skip the prompt() by writing directly to the DOM node — good enough
      // for a mock site we control end to end.
      await page.evaluate((value) => {
        document.getElementById('my-headline').textContent = value;
      }, headline);
      return { action: 'updated its own headline to', target: `"${headline}"` };
    }
    default:
      return { action: 'stood around menacingly' };
  }
}

async function postToDiscord(body) {
  if (!discord) return;
  const parsed = new url.URL(discord);
  if (parsed.hostname !== 'discord.com' || !parsed.pathname.startsWith('/api/webhooks/')) {
    return;
  }
  const payload = JSON.stringify({ content: `😈 Minion says: ${body}` });
  const https = require('https');
  await new Promise((resolve, reject) => {
    const req = https.request(
      {
        method: 'POST',
        hostname: parsed.hostname,
        path: parsed.pathname + parsed.search,
        headers: {
          'Content-Type': 'application/json',
          'Content-Length': Buffer.byteLength(payload),
        },
      },
      (res) => {
        res.on('data', () => {});
        res.on('end', resolve);
      },
    );
    req.on('error', reject);
    req.write(payload);
    req.end();
  });
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i];
    if (token.startsWith('--')) {
      const key = token.slice(2);
      const next = argv[i + 1];
      if (next && !next.startsWith('--')) {
        out[key] = next;
        i += 1;
      } else {
        out[key] = true;
      }
    }
  }
  return out;
}

function mulberry32(seedValue) {
  let state = seedValue >>> 0;
  return function next() {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Exports for the allowlist unit test. Never called in production.
module.exports = { isAllowedRequest, mulberry32 };

// Only auto-run when invoked as a script, not when required for tests.
if (require.main === module) {
  run().catch((err) => {
    console.error(JSON.stringify({ error: 'unexpected', reason: err.stack || String(err) }));
    process.exit(0);
  });
}
