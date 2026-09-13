#!/usr/bin/env node
'use strict';

const { chromium } = require('playwright');

async function main() {
  const browser = await chromium.launch({ channel: 'chrome' });
  const page = await browser.newPage();
  const junk = Array.from({ length: 200 }, (_, i) => `<a href="#${i}">a</a>`).join('');
  await page.setContent(`
    <input role="searchbox" aria-label="Search messages" />
    <ul>${junk}</ul>
    <form class="msg-form">
      <div role="textbox" contenteditable="true" class="msg-form__contenteditable" aria-label="Write a message…"> </div>
      <button type="submit">Send</button>
    </form>
  `);
  const yaml = await page.ariaSnapshot({ mode: 'ai' });
  if (!/Write a message/.test(yaml)) {
    throw new Error(`composer missing from Playwright ARIA snapshot:\n${yaml}`);
  }
  const match = yaml.match(/textbox "Write a message[^"]*" \[ref=([^\]]+)\]/);
  if (!match) {
    throw new Error(`no textbox ref in snapshot:\n${yaml}`);
  }
  const locator = page.locator(`aria-ref=${match[1]}`);
  if ((await locator.count()) !== 1) {
    throw new Error(`aria-ref=${match[1]} did not resolve`);
  }
  await locator.click();
  await page.keyboard.type('test test');
  const text = await locator.innerText();
  if (!text.includes('test test')) {
    throw new Error(`typed text missing, got: ${text}`);
  }
  const form = page.locator('form.msg-form').first();
  const region = await form.ariaSnapshot({ mode: 'ai' });
  if (!/Write a message/.test(region) || !/Send/.test(region)) {
    throw new Error(`compose region missing send/composer:\n${region}`);
  }
  await browser.close();
  process.stdout.write('ok\n');
}

main().catch((error) => {
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exit(1);
});
