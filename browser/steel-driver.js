#!/usr/bin/env node
'use strict';

/**
 * JSON-line sidecar that drives a Steel cloud Chrome session over CDP.
 * stdout is protocol only; logs go to stderr. Mock mode (--mock / CLAW_BROWSER_MOCK=1)
 * never talks to Steel and needs no Playwright install.
 */

const { spawn } = require('child_process');
const fs = require('fs');
const http = require('http');
const os = require('os');
const path = require('path');
const readline = require('readline');

const MOCK =
  process.argv.includes('--mock') || process.env.CLAW_BROWSER_MOCK === '1';

/** @type {null | {
 *   sessionId: string,
 *   debugUrl: string,
 *   profileId: string | null,
 *   url: string,
 *   title: string,
 *   nodes: Array<{ref: string, role: string, name: string, tag: string}>,
 *   lastTyped: Record<string, string>,
 *   lastClicked: string | null,
 *   sent: boolean,
 * }} */
let mockState = null;

/** @type {null | {
 *   session: object | null,
 *   browser: object,
 *   page: object,
 *   client: object | null,
 *   backend: 'steel' | 'local',
 * }} */
let liveState = null;

function writeResult(id, ok, payload) {
  const body = ok
    ? { id, ok: true, result: payload }
    : { id, ok: false, error: String(payload) };
  process.stdout.write(`${JSON.stringify(body)}\n`);
}

function watchUrl(debugUrl) {
  if (!debugUrl) {
    return null;
  }
  const sep = debugUrl.includes('?') ? '&' : '?';
  return `${debugUrl}${sep}interactive=true`;
}

function cdpEndpoint(session, apiKey) {
  if (session.websocketUrl) {
    const sep = session.websocketUrl.includes('?') ? '&' : '?';
    return `${session.websocketUrl}${sep}apiKey=${encodeURIComponent(apiKey)}`;
  }
  return `wss://connect.steel.dev?apiKey=${encodeURIComponent(apiKey)}&sessionId=${session.id}`;
}

function mockNodesForUrl(url) {
  const lower = url.toLowerCase();
  if (lower.includes('mail.google.com') || lower.includes('gmail')) {
    return [
      { ref: 'e1', role: 'button', name: 'Compose', tag: 'button' },
      { ref: 'e2', role: 'link', name: 'Inbox', tag: 'a' },
      { ref: 'e3', role: 'searchbox', name: 'Search mail', tag: 'input' },
    ];
  }
  if (lower.includes('drive.google.com') || lower.includes('docs.google.com')) {
    return [
      { ref: 'e1', role: 'heading', name: 'resume claude', tag: 'h1' },
      { ref: 'e2', role: 'textbox', name: 'Document', tag: 'div' },
      { ref: 'e3', role: 'searchbox', name: 'Search Drive', tag: 'input' },
    ];
  }
  if (lower.includes('discord.com') || lower.includes('discord')) {
    return [
      { ref: 'e1', role: 'textbox', name: 'Message', tag: 'div' },
      { ref: 'e2', role: 'button', name: 'Send Message', tag: 'button' },
      { ref: 'e3', role: 'searchbox', name: 'Search', tag: 'input' },
    ];
  }
  return [
    { ref: 'e1', role: 'link', name: 'Home', tag: 'a' },
    { ref: 'e2', role: 'textbox', name: 'Search', tag: 'input' },
  ];
}

function mockComposeNodes() {
  return [
    { ref: 'e3', role: 'textbox', name: 'To', tag: 'input' },
    { ref: 'e4', role: 'textbox', name: 'Subject', tag: 'input' },
    { ref: 'e5', role: 'textbox', name: 'Message Body', tag: 'div' },
    { ref: 'e6', role: 'button', name: 'Send', tag: 'button' },
  ];
}

function requireMockSession() {
  if (!mockState) {
    throw new Error('No browser session. Call BrowserStart first.');
  }
  return mockState;
}

function formatSnapshot(state) {
  const lines = state.nodes.map(
    (node) => `[ref=${node.ref}] ${node.role} "${node.name}"`,
  );
  return {
    url: state.url,
    title: state.title,
    snapshot: lines.join('\n'),
  };
}

async function mockHandle(method, params) {
  switch (method) {
    case 'start': {
      mockState = {
        sessionId: 'mock-session',
        debugUrl: 'https://api.steel.dev/v1/sessions/mock-session/player',
        profileId: params.profileId || 'mock-profile',
        url: 'about:blank',
        title: 'New Tab',
        nodes: [],
        lastTyped: {},
        lastClicked: null,
        sent: false,
      };
      return {
        sessionId: mockState.sessionId,
        debugUrl: mockState.debugUrl,
        watchUrl: watchUrl(mockState.debugUrl),
        profileId: mockState.profileId,
        backend: defaultBackend(params),
        mock: true,
      };
    }
    case 'navigate': {
      const state = requireMockSession();
      const url = String(params.url || '');
      if (!url) {
        throw new Error('url is required');
      }
      state.url = url;
      state.title = url.includes('mail.google')
        ? 'Inbox - Gmail'
        : url.includes('drive.google') || url.includes('docs.google')
          ? 'resume claude - Google Drive'
          : url.includes('discord')
            ? 'Discord'
            : url;
      state.nodes = mockNodesForUrl(url);
      state.sent = false;
      return { ...formatSnapshot(state), navigated: url };
    }
    case 'snapshot': {
      return formatSnapshot(requireMockSession());
    }
    case 'click': {
      const state = requireMockSession();
      const ref = String(params.ref || '');
      const node = state.nodes.find((item) => item.ref === ref);
      if (!node) {
        throw new Error(`unknown ref: ${ref}`);
      }
      state.lastClicked = ref;
      if (node.name === 'Compose') {
        state.nodes = mockComposeNodes();
        state.title = 'Compose - Gmail';
      }
      if (node.name === 'Send') {
        state.sent = true;
        state.nodes = mockNodesForUrl('https://mail.google.com');
        state.title = 'Inbox - Gmail (sent)';
      }
      return { clicked: ref, name: node.name, ...formatSnapshot(state) };
    }
    case 'type': {
      const state = requireMockSession();
      const ref = String(params.ref || '');
      const text = String(params.text || '');
      const node = state.nodes.find((item) => item.ref === ref);
      if (!node) {
        throw new Error(`unknown ref: ${ref}`);
      }
      if (!text) {
        throw new Error('text is required');
      }
      state.lastTyped[ref] = text;
      return { typed: text, ref, ...formatSnapshot(state) };
    }
    case 'press': {
      requireMockSession();
      return { pressed: String(params.key || ''), ...formatSnapshot(mockState) };
    }
    case 'scroll': {
      requireMockSession();
      return {
        scrolled: Number(params.deltaY || 0),
        ...formatSnapshot(mockState),
      };
    }
    case 'wait': {
      const state = requireMockSession();
      const urlContains = params.urlContains
        ? String(params.urlContains)
        : null;
      const titleContains = params.titleContains
        ? String(params.titleContains)
        : null;
      const matched =
        (!urlContains || state.url.includes(urlContains)) &&
        (!titleContains || state.title.includes(titleContains));
      if (!matched) {
        throw new Error(
          `wait timed out (url=${state.url} title=${state.title}). Sign in in the live viewer if this is a login wall.`,
        );
      }
      return { waited: true, ...formatSnapshot(state) };
    }
    case 'status': {
      if (!mockState) {
        return { live: false, mock: true };
      }
      return {
        live: true,
        mock: true,
        backend: defaultBackend({}),
        sessionId: mockState.sessionId,
        debugUrl: mockState.debugUrl,
        watchUrl: watchUrl(mockState.debugUrl),
        profileId: mockState.profileId,
        url: mockState.url,
        title: mockState.title,
      };
    }
    case 'stop': {
      const previous = mockState;
      mockState = null;
      return {
        released: true,
        sessionId: previous ? previous.sessionId : null,
        profileId: previous ? previous.profileId : null,
      };
    }
    default:
      throw new Error(`unsupported method: ${method}`);
  }
}

async function loadPlaywright() {
  try {
    return require('playwright');
  } catch (error) {
    throw new Error(
      `Playwright is not installed. From the repo root run: cd browser && npm install (${error.message})`,
    );
  }
}

async function loadPlaywrightAndSteel() {
  const playwright = await loadPlaywright();
  let Steel;
  try {
    Steel = require('steel-sdk');
  } catch (error) {
    throw new Error(
      `steel-sdk is not installed. From the repo root run: cd browser && npm install (${error.message})`,
    );
  }
  return { playwright, Steel };
}

function defaultBackend(params) {
  return String(
    (params && params.backend) || process.env.CLAW_BROWSER_BACKEND || 'local',
  ).toLowerCase();
}

function localCdpUrl(params) {
  return (
    (params && params.cdpUrl) ||
    process.env.CLAW_CHROME_CDP_URL ||
    'http://127.0.0.1:9222'
  );
}

function autoLaunchEnabled(params) {
  if (params && params.autoLaunch === false) {
    return false;
  }
  const flag = String(process.env.CLAW_BROWSER_NO_LAUNCH || '').toLowerCase();
  return flag !== '1' && flag !== 'true';
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function withTimeout(promise, ms, label) {
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      setTimeout(() => reject(new Error(`${label} timed out after ${ms}ms`)), ms);
    }),
  ]);
}

/** Chrome M144+ inspect debugging has no HTTP /json/version (404). Classic --remote-debugging-port still returns 200. */
const INSPECT_CONNECT_TIMEOUT_MS = 90_000;

function cdpReady(cdpUrl, timeoutMs = 800) {
  return new Promise((resolve) => {
    let url;
    try {
      url = new URL('/json/version', cdpUrl.endsWith('/') ? cdpUrl : `${cdpUrl}/`);
    } catch (_error) {
      resolve(false);
      return;
    }
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      res.resume();
      resolve(res.statusCode === 200);
    });
    req.on('error', () => resolve(false));
    req.on('timeout', () => {
      req.destroy();
      resolve(false);
    });
  });
}

function chromeUserDataDir() {
  if (process.env.CLAW_CHROME_USER_DATA_DIR) {
    return process.env.CLAW_CHROME_USER_DATA_DIR;
  }
  if (process.platform === 'darwin') {
    return path.join(
      os.homedir(),
      'Library',
      'Application Support',
      'Google',
      'Chrome',
    );
  }
  if (process.platform === 'win32') {
    return path.join(
      process.env.LOCALAPPDATA || '',
      'Google',
      'Chrome',
      'User Data',
    );
  }
  return path.join(os.homedir(), '.config', 'google-chrome');
}

function readDevtoolsPort(userDataDir) {
  const portPath = path.join(userDataDir, 'DevToolsActivePort');
  let content;
  try {
    content = fs.readFileSync(portPath, 'utf8');
  } catch (_error) {
    return null;
  }
  const lines = content
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length < 2) {
    return null;
  }
  const port = Number(lines[0]);
  const wsPath = lines[1];
  if (!Number.isInteger(port) || port <= 0 || !wsPath.startsWith('/')) {
    return null;
  }
  return {
    port,
    wsPath,
    wsUrl: `ws://127.0.0.1:${port}${wsPath}`,
    portPath,
  };
}

function openInspectPage() {
  const url = 'chrome://inspect/#remote-debugging';
  if (process.platform === 'darwin') {
    spawn('open', ['-a', 'Google Chrome', url], {
      stdio: 'ignore',
      detached: true,
    }).unref();
    return;
  }
  if (process.platform === 'win32') {
    spawn('cmd', ['/C', 'start', '', url], {
      stdio: 'ignore',
      detached: true,
    }).unref();
    return;
  }
  spawn('xdg-open', [url], { stdio: 'ignore', detached: true }).unref();
}

function inspectHint(details) {
  const extra = details ? ` (${details})` : '';
  return [
    'Could not attach to Chrome.',
    'Chrome 144+ inspect debugging has no HTTP /json/version endpoint; each BrowserStart opens a new Allow dialog.',
    'Open chrome://inspect/#remote-debugging, turn Remote debugging ON, click Allow once, and wait for this call to finish.',
    'Do not retry BrowserStart while the dialog is up — retrying asks Allow again.',
    extra,
  ]
    .filter(Boolean)
    .join(' ');
}

async function connectOverEndpoint(playwright, endpoint, timeoutMs = 8000) {
  return playwright.chromium.connectOverCDP(endpoint, { timeout: timeoutMs });
}

function logAllowWait(wsUrl) {
  process.stderr.write(
    `Waiting up to ${Math.round(INSPECT_CONNECT_TIMEOUT_MS / 1000)}s for Chrome Allow on ${wsUrl}. Click Allow once; do not retry.\n`,
  );
}

async function connectLocalChrome(playwright, cdpUrl, autoLaunch) {
  const userDataDir = chromeUserDataDir();
  // Classic --remote-debugging-port still serves HTTP /json/version. Chrome 144+
  // inspect mode listens on the same port but returns 404 — skip HTTP there.
  if (await cdpReady(cdpUrl)) {
    const browser = await connectOverEndpoint(playwright, cdpUrl, 15_000);
    return { browser, via: 'http-cdp', openedInspect: false };
  }

  const existing = readDevtoolsPort(userDataDir);
  if (existing) {
    logAllowWait(existing.wsUrl);
    try {
      const browser = await connectOverEndpoint(
        playwright,
        existing.wsUrl,
        INSPECT_CONNECT_TIMEOUT_MS,
      );
      return { browser, via: 'inspect', openedInspect: false };
    } catch (error) {
      throw new Error(inspectHint(`${existing.wsUrl}: ${error.message}`));
    }
  }

  if (!autoLaunch) {
    throw new Error(inspectHint(`DevToolsActivePort not found in ${userDataDir}`));
  }

  openInspectPage();
  process.stderr.write(
    'Opened chrome://inspect/#remote-debugging. Turn Remote debugging ON, then click Allow once.\n',
  );
  const deadline = Date.now() + INSPECT_CONNECT_TIMEOUT_MS;
  while (Date.now() < deadline) {
    await sleep(500);
    const ready = readDevtoolsPort(userDataDir);
    if (!ready) {
      continue;
    }
    const remaining = Math.max(5_000, deadline - Date.now());
    logAllowWait(ready.wsUrl);
    try {
      const browser = await connectOverEndpoint(playwright, ready.wsUrl, remaining);
      return { browser, via: 'inspect', openedInspect: true };
    } catch (error) {
      // A second connectOverCDP would spawn another Allow dialog.
      throw new Error(inspectHint(`${ready.wsUrl}: ${error.message}`));
    }
  }
  throw new Error(
    inspectHint(`DevToolsActivePort not found in ${userDataDir} after ${INSPECT_CONNECT_TIMEOUT_MS}ms`),
  );
}

async function openUrlInNewTab(state, url) {
  const context =
    (state.page && typeof state.page.context === 'function' && state.page.context()) ||
    (state.browser && state.browser.contexts()[0]) ||
    null;
  let page;
  if (context) {
    page = await context.newPage();
  } else if (state.page) {
    page = state.page;
  } else {
    throw new Error('No browser context to open a tab.');
  }
  try {
    await page.bringToFront();
  } catch (_error) {
    // CDP attach may not support bringToFront
  }
  await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 30000 });
  await page.waitForTimeout(1200);
  state.page = page;
  return page;
}

function pickPage(browser) {
  const contexts = browser.contexts();
  const pages = [];
  for (const context of contexts) {
    for (const page of context.pages()) {
      pages.push(page);
    }
  }
  const usable = pages.filter((page) => {
    const url = page.url();
    return url && !url.startsWith('devtools://') && !url.startsWith('chrome-extension://');
  });
  return usable[usable.length - 1] || pages[pages.length - 1] || null;
}

const SNAPSHOT_CHAR_CAP = 14000;

const COMPOSE_REGION_SELECTORS = [
  '[class="msg-form"], form[class*="msg-form"]',
  '[class*="channelTextArea"]',
  'div[role="dialog"]:has([aria-label="To"]), div[role="dialog"]:has([name="to"])',
  'form[class*="compose"]',
  '[aria-label*="Write a message" i]',
  '[aria-label*="Type a message" i]',
  '[aria-label="Message Body"]',
];

function extractHotLines(yaml) {
  return String(yaml || '')
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => {
      if (!/\[ref=/.test(line)) {
        return false;
      }
      if (/\b(textbox|searchbox|combobox)\b/i.test(line)) {
        return true;
      }
      return /\bbutton\b/i.test(line) && /\b(send|submit|compose)\b/i.test(line);
    });
}

async function firstVisible(page, selector) {
  const locator = page.locator(selector);
  const count = await locator.count().catch(() => 0);
  for (let i = 0; i < count; i += 1) {
    const item = locator.nth(i);
    if (await item.isVisible().catch(() => false)) {
      return item;
    }
  }
  return null;
}

async function composeRegionLocator(page) {
  for (const selector of COMPOSE_REGION_SELECTORS) {
    const found = await firstVisible(page, selector);
    if (found) {
      return found;
    }
  }
  const boxes = page.locator(
    '[role="textbox"][contenteditable="true"], [contenteditable="true"]',
  );
  const count = Math.min(await boxes.count().catch(() => 0), 12);
  for (let i = count - 1; i >= 0; i -= 1) {
    const box = boxes.nth(i);
    if (!(await box.isVisible().catch(() => false))) {
      continue;
    }
    const search = await box.evaluate((el) => {
      const blob = `${el.getAttribute('aria-label') || ''} ${el.className || ''}`.toLowerCase();
      return /\bsearch\b/.test(blob);
    }).catch(() => false);
    if (!search) {
      return box;
    }
  }
  return null;
}

async function formatLiveSnapshot(page) {
  if (typeof page.ariaSnapshot !== 'function') {
    throw new Error(
      'Playwright ariaSnapshot is missing. From the repo root run: cd browser && npm install playwright@latest',
    );
  }
  let pageYaml = '';
  let composeYaml = '';
  for (let attempt = 0; attempt < 2; attempt += 1) {
    try {
      pageYaml = await withTimeout(
        page.ariaSnapshot({ mode: 'ai' }),
        12_000,
        'page snapshot',
      );
      const compose = await withTimeout(
        composeRegionLocator(page),
        4_000,
        'compose locator',
      );
      if (compose && typeof compose.ariaSnapshot === 'function') {
        composeYaml = await withTimeout(
          compose.ariaSnapshot({ mode: 'ai' }),
          6_000,
          'compose snapshot',
        );
      }
    } catch (_error) {
      await page.waitForTimeout(400);
      continue;
    }
    if (pageYaml || composeYaml) {
      break;
    }
    await page.waitForTimeout(500);
  }

  const hot = [];
  const seen = new Set();
  for (const line of [...extractHotLines(composeYaml), ...extractHotLines(pageYaml)]) {
    if (seen.has(line)) {
      continue;
    }
    seen.add(line);
    hot.push(line);
  }

  const blob = `${composeYaml}\n${pageYaml}`.toLowerCase();
  const hints = [];
  if (/\btextbox\b/.test(blob)) {
    hints.push(
      'Hint: type chat/email into a textbox (Write a message, Message Body, To) from Type targets / Compose area. Never type into a searchbox. For LinkedIn, BrowserType clicks Send after typing. For Discord/Slack, it presses Enter. Do not leave a DM sitting in the composer.',
    );
  }

  let body = pageYaml || '';
  if (body.length > SNAPSHOT_CHAR_CAP) {
    body = `${body.slice(0, SNAPSHOT_CHAR_CAP)}\n[ARIA snapshot truncated from ${pageYaml.length} characters]`;
  }

  const parts = [];
  if (hints.length) {
    parts.push(hints.join('\n'));
  }
  if (hot.length) {
    parts.push(['Type targets:', ...hot].join('\n'));
  }
  if (composeYaml) {
    parts.push(`Compose area:\n${composeYaml}`);
  }
  if (body) {
    parts.push(body);
  }

  return {
    url: page.url(),
    title: await page.title().catch(() => ''),
    snapshot: parts.join('\n\n'),
  };
}

async function locatorForRef(page, ref) {
  const clean = String(ref || '').replace(/^@/, '').trim();
  if (!clean) {
    throw new Error('ref is required');
  }
  const aria = page.locator(`aria-ref=${clean}`);
  if ((await aria.count()) > 0) {
    return aria.first();
  }
  const stamped = page.locator(`[data-claw-ref="${clean}"]`);
  if ((await stamped.count()) > 0) {
    return stamped.first();
  }
  throw new Error(`unknown ref: ${ref}. Call BrowserSnapshot and use a current ref.`);
}

async function isSearchLocator(locator) {
  return locator.evaluate((el) => {
    if (el.getAttribute('data-claw-kind') === 'search') {
      return true;
    }
    const role = (el.getAttribute('role') || '').toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    const label = `${el.getAttribute('aria-label') || ''} ${
      el.getAttribute('placeholder') || ''
    }`.toLowerCase();
    return (
      role === 'searchbox' ||
      type === 'search' ||
      /\bsearch\b/.test(label)
    );
  });
}

async function composerLocator(page) {
  const byRole = page.getByRole('textbox', {
    name: /write a message|type a message|message composer|message body|message karma/i,
  });
  const roleCount = await byRole.count().catch(() => 0);
  for (let i = roleCount - 1; i >= 0; i -= 1) {
    const loc = byRole.nth(i);
    if (await loc.isVisible().catch(() => false)) {
      return loc;
    }
  }
  return composeRegionLocator(page);
}

async function isTypeableLocator(locator) {
  return locator.evaluate((el) => {
    const tag = el.tagName.toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (['checkbox', 'radio', 'hidden', 'submit', 'button', 'file'].includes(type)) {
      return false;
    }
    if (el.isContentEditable) {
      return true;
    }
    if (tag === 'textarea' || tag === 'input' || tag === 'select') {
      return true;
    }
    const role = (el.getAttribute('role') || '').toLowerCase();
    return ['textbox', 'searchbox', 'combobox'].includes(role);
  });
}

async function isChatComposerLocator(locator) {
  return locator.evaluate((el) => {
    const root =
      el.closest(
        '[class*="msg-form"], [class*="msg-s-message"], [class*="channelTextArea"], [data-slate-editor="true"], form[class*="msg"]',
      ) || el;
    const blob = `${root.className || ''} ${el.getAttribute('aria-label') || ''} ${
      el.getAttribute('placeholder') || ''
    } ${el.getAttribute('data-placeholder') || ''}`.toLowerCase();
    return /msg-form|msg-s-message|channeltextarea|slate|write a message|type a message|message karma|message…|message\.\.\./.test(
      blob,
    );
  });
}

async function findSendButton(page) {
  const selectors = [
    'button.msg-form__send-button',
    'button[type="submit"].msg-form__send-button',
    'button[aria-label="Send"]',
    'button[aria-label="Send message"]',
    'button[aria-label*="Send" i]',
  ];
  for (const selector of selectors) {
    const found = await firstVisible(page, selector);
    if (!found) {
      continue;
    }
    const label = await found.getAttribute('aria-label').catch(() => '');
    if (/feedback|invite|file|photo/i.test(label || '')) {
      continue;
    }
    if (await found.isDisabled().catch(() => false)) {
      continue;
    }
    return found;
  }
  const byRole = page.getByRole('button', { name: /^send( message)?$/i });
  const count = await byRole.count().catch(() => 0);
  for (let i = count - 1; i >= 0; i -= 1) {
    const btn = byRole.nth(i);
    if (
      (await btn.isVisible().catch(() => false)) &&
      !(await btn.isDisabled().catch(() => false))
    ) {
      return btn;
    }
  }
  return null;
}

async function submitChatMessage(page) {
  const sendBtn = await findSendButton(page);
  if (sendBtn) {
    await sendBtn.click({ timeout: 5000 });
    await page.waitForTimeout(400);
    return 'clicked-send';
  }
  await page.keyboard.press('Enter');
  await page.waitForTimeout(250);
  return 'pressed-enter';
}

async function typeIntoLocator(page, locator, text) {
  await locator.scrollIntoViewIfNeeded();
  await locator.click({ timeout: 5000 });
  const editable = await locator.evaluate((el) => {
    const host = el.closest('[contenteditable="true"]') || el;
    return Boolean(host && host.isContentEditable);
  });
  if (editable) {
    await page.keyboard.press('ControlOrMeta+A');
    await page.keyboard.press('Backspace');
    await page.keyboard.type(text, { delay: 8 });
    return;
  }
  await locator.fill(text);
}

async function releaseLive() {
  if (!liveState) {
    return { released: false, sessionId: null, profileId: null, backend: null };
  }
  const current = liveState;
  liveState = null;
  const local = current.backend === 'local';
  if (!local) {
    try {
      if (current.browser) {
        await current.browser.close();
      }
    } catch (_error) {
      // ignore close errors so we still release the Steel session
    }
    try {
      if (current.client && current.session && current.session.id) {
        await current.client.sessions.release(current.session.id);
      }
    } catch (_error) {
      // already released or network failure
    }
  }
  return {
    released: true,
    backend: current.backend,
    sessionId: current.session ? current.session.id : local ? 'local-chrome' : null,
    profileId: current.session ? current.session.profileId || null : null,
  };
}

async function liveHandle(method, params) {
  switch (method) {
    case 'start': {
      await releaseLive();
      const backend = defaultBackend(params);
      if (backend === 'local') {
        const playwright = await loadPlaywright();
        const cdpUrl = localCdpUrl(params);
        const connected = await connectLocalChrome(
          playwright,
          cdpUrl,
          autoLaunchEnabled(params),
        );
        const browser = connected.browser;
        let page = pickPage(browser);
        if (!page) {
          const context = browser.contexts()[0] || (await browser.newContext());
          page = await context.newPage();
        }
        liveState = {
          session: null,
          browser,
          page,
          client: null,
          backend: 'local',
        };
        const snap = await formatLiveSnapshot(page);
        return {
          sessionId: 'local-chrome',
          backend: 'local',
          cdpUrl,
          debugUrl: null,
          watchUrl: null,
          profileId: null,
          mock: false,
          launched: connected.openedInspect,
          via: connected.via,
          hint:
            connected.via === 'inspect'
              ? 'Attached to your Chrome. If a permission dialog appeared, click Allow. Watch that window — Gmail/Discord stay logged in.'
              : 'Attached to your local Chrome. Watch the Chrome window on this Mac — that is the live view.',
          ...snap,
        };
      }
      const apiKey = process.env.STEEL_API_KEY || params.apiKey;
      if (!apiKey) {
        throw new Error(
          'Steel is not configured. Copy .env.example to .env and set STEEL_API_KEY, or omit backend=steel to use local Chrome.',
        );
      }
      const { playwright, Steel } = await loadPlaywrightAndSteel();
      const baseURL = process.env.STEEL_BASE_URL || params.baseUrl || undefined;
      const client = baseURL
        ? new Steel({ steelAPIKey: apiKey, baseURL })
        : new Steel({ steelAPIKey: apiKey });
      const timeoutMs = Number(params.timeoutMs || 900000);
      const createPayload = {
        timeout: timeoutMs,
        persistProfile: true,
        debugConfig: { interactive: true },
        dimensions: { width: 1280, height: 768 },
      };
      if (params.profileId) {
        createPayload.profileId = params.profileId;
      }
      const session = await client.sessions.create(createPayload);
      const browser = await playwright.chromium.connectOverCDP(
        cdpEndpoint(session, apiKey),
      );
      const context = browser.contexts()[0];
      const page =
        context && context.pages().length > 0
          ? context.pages()[0]
          : await context.newPage();
      liveState = { session, browser, page, client, backend: 'steel' };
      return {
        sessionId: session.id,
        backend: 'steel',
        debugUrl: session.debugUrl,
        watchUrl: watchUrl(session.debugUrl),
        profileId: session.profileId || params.profileId || null,
        mock: false,
      };
    }
    case 'navigate': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const url = String(params.url || '');
      if (!url) {
        throw new Error('url is required');
      }
      const page = await openUrlInNewTab(liveState, url);
      return {
        navigated: url,
        newTab: true,
        ...(await formatLiveSnapshot(page)),
      };
    }
    case 'snapshot': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      return formatLiveSnapshot(liveState.page);
    }
    case 'click': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const ref = String(params.ref || '');
      const locator = await locatorForRef(liveState.page, ref);
      await locator.click();
      await liveState.page.waitForTimeout(300);
      return { clicked: ref, ...(await formatLiveSnapshot(liveState.page)) };
    }
    case 'type': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const ref = String(params.ref || '');
      const text = String(params.text || '');
      if (!text) {
        throw new Error('text is required');
      }
      let locator = await locatorForRef(liveState.page, ref);
      let typedRef = ref;
      let redirectedFrom = null;
      const search = await isSearchLocator(locator);
      const typeable = search ? false : await isTypeableLocator(locator);
      if (search || !typeable) {
        const composer = await composerLocator(liveState.page);
        if (composer) {
          redirectedFrom = ref;
          locator = composer;
          typedRef = ref;
        }
      }
      await typeIntoLocator(liveState.page, locator, text);
      let sent = false;
      let sendHow = null;
      const onLinkedIn = /linkedin\.com/i.test(liveState.page.url());
      const chatComposer = await isChatComposerLocator(locator).catch(() => false);
      const shouldSend =
        chatComposer ||
        (onLinkedIn && (Boolean(redirectedFrom) || (!search && typeable)));
      if (shouldSend) {
        sendHow = await submitChatMessage(liveState.page);
        sent = true;
      }
      const result = {
        typed: text,
        ref: typedRef,
        sent,
        sendHow,
        ...(await formatLiveSnapshot(liveState.page)),
      };
      if (redirectedFrom) {
        result.redirectedFrom = redirectedFrom;
        result.note = sent
          ? `${redirectedFrom} is not the message box. Typed into the chat composer and sent (${sendHow}).`
          : `${redirectedFrom} is not the message box. Typed into the composer instead.`;
      } else if (sent) {
        result.note =
          sendHow === 'clicked-send'
            ? 'Clicked Send to deliver the chat message.'
            : 'Pressed Enter to send (chat composer).';
      }
      return result;
    }
    case 'press': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const key = String(params.key || '');
      if (!key) {
        throw new Error('key is required');
      }
      await liveState.page.keyboard.press(key);
      return { pressed: key, ...(await formatLiveSnapshot(liveState.page)) };
    }
    case 'scroll': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const deltaY = Number(params.deltaY || 400);
      await liveState.page.mouse.wheel(0, deltaY);
      return { scrolled: deltaY, ...(await formatLiveSnapshot(liveState.page)) };
    }
    case 'wait': {
      if (!liveState) {
        throw new Error('No browser session. Call BrowserStart first.');
      }
      const timeoutMs = Number(params.timeoutMs || 120000);
      const urlContains = params.urlContains
        ? String(params.urlContains)
        : null;
      const titleContains = params.titleContains
        ? String(params.titleContains)
        : null;
      const deadline = Date.now() + timeoutMs;
      while (Date.now() < deadline) {
        const snap = await formatLiveSnapshot(liveState.page);
        const matched =
          (!urlContains || snap.url.includes(urlContains)) &&
          (!titleContains || snap.title.includes(titleContains));
        if (matched) {
          return { waited: true, ...snap };
        }
        await liveState.page.waitForTimeout(1500);
      }
      const snap = await formatLiveSnapshot(liveState.page);
      throw new Error(
        `wait timed out (url=${snap.url} title=${snap.title}). Sign in in the live viewer if this is a login wall.`,
      );
    }
    case 'status': {
      if (!liveState) {
        return { live: false, mock: false };
      }
      const local = liveState.backend === 'local';
      const url = liveState.page.url();
      const title = await liveState.page.title().catch(() => '');
      return {
        live: true,
        mock: false,
        backend: liveState.backend,
        sessionId: local ? 'local-chrome' : liveState.session.id,
        debugUrl: local ? null : liveState.session.debugUrl,
        watchUrl: local ? null : watchUrl(liveState.session.debugUrl),
        profileId: local ? null : liveState.session.profileId || null,
        url,
        title,
      };
    }
    case 'stop': {
      return releaseLive();
    }
    default:
      throw new Error(`unsupported method: ${method}`);
  }
}

async function dispatch(method, params) {
  if (MOCK) {
    return mockHandle(method, params || {});
  }
  return liveHandle(method, params || {});
}

async function onLine(line) {
  const trimmed = line.trim();
  if (!trimmed) {
    return;
  }
  let message;
  try {
    message = JSON.parse(trimmed);
  } catch (error) {
    writeResult(null, false, `invalid json: ${error.message}`);
    return;
  }
  const id = Object.prototype.hasOwnProperty.call(message, 'id')
    ? message.id
    : null;
  try {
    const result = await dispatch(message.method, message.params);
    writeResult(id, true, result);
  } catch (error) {
    writeResult(id, false, error.message || String(error));
  }
}

function startRepl() {
  const rl = readline.createInterface({
    input: process.stdin,
    crlfDelay: Infinity,
  });
  let queue = Promise.resolve();
  rl.on('line', (line) => {
    queue = queue
      .then(() => onLine(line))
      .catch((error) => {
        process.stderr.write(`${error.stack || error.message}\n`);
      });
  });
  const shutdown = async () => {
    try {
      if (MOCK) {
        mockState = null;
      } else {
        await releaseLive();
      }
    } finally {
      process.exit(0);
    }
  };
  rl.on('close', () => {
    shutdown().catch(() => process.exit(0));
  });
  process.on('SIGTERM', () => {
    shutdown().catch(() => process.exit(0));
  });
  process.on('SIGINT', () => {
    shutdown().catch(() => process.exit(0));
  });
}

startRepl();
