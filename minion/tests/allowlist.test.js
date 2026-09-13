'use strict';

const assert = require('assert');
const { isAllowedRequest } = require('..');

let failures = 0;

function check(label, actual, expected) {
  try {
    assert.strictEqual(actual, expected);
    console.log(`ok  ${label}`);
  } catch (err) {
    failures += 1;
    console.error(`FAIL ${label} — ${err.message}`);
  }
}

// Baseline: mock site.
check('allows localhost', isAllowedRequest('http://127.0.0.1:4242/'), true);
check('allows localhost hostname', isAllowedRequest('http://localhost:4242/api/x'), true);
check('allows ::1', isAllowedRequest('http://[::1]:4242/'), true);

// Anything else must abort.
check('blocks linkedin.com', isAllowedRequest('https://www.linkedin.com/feed'), false);
check('blocks twitter.com', isAllowedRequest('https://x.com/foo'), false);
check('blocks arbitrary host', isAllowedRequest('http://evil.example.com/api'), false);

// Discord is only allowed via the webhook path, and only when configured.
// The module-level `discord` binding is null here (no env var), so all
// Discord traffic must be blocked.
check('blocks discord without env var', isAllowedRequest('https://discord.com/api/webhooks/1/x'), false);
check('blocks discord homepage', isAllowedRequest('https://discord.com/'), false);

// data:/about: URIs are harmless plumbing and must not be blocked (Playwright
// uses them internally).
check('allows data: URIs', isAllowedRequest('data:text/html,<b>hi</b>'), true);

if (failures > 0) {
  console.error(`\n${failures} failure(s)`);
  process.exit(1);
}
console.log('\nall good');
