#!/usr/bin/env node
'use strict';

const { spawn } = require('child_process');
const path = require('path');
const readline = require('readline');

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

async function main() {
  const child = spawn(process.execPath, [path.join(__dirname, 'steel-driver.js'), '--mock'], {
    stdio: ['pipe', 'pipe', 'inherit'],
  });
  const rl = readline.createInterface({ input: child.stdout, crlfDelay: Infinity });
  let nextId = 1;

  function rpc(method, params) {
    const id = nextId;
    nextId += 1;
    return new Promise((resolve, reject) => {
      const onLine = (line) => {
        let message;
        try {
          message = JSON.parse(line);
        } catch (error) {
          rl.off('line', onLine);
          reject(error);
          return;
        }
        if (message.id !== id) {
          return;
        }
        rl.off('line', onLine);
        if (message.ok) {
          resolve(message.result);
        } else {
          reject(new Error(message.error));
        }
      };
      rl.on('line', onLine);
      child.stdin.write(`${JSON.stringify({ id, method, params })}\n`);
    });
  }

  const started = await rpc('start', {});
  if (!started.sessionId || !started.watchUrl) {
    fail('start should return sessionId and watchUrl');
  }

  const navigated = await rpc('navigate', { url: 'https://mail.google.com' });
  if (!navigated.snapshot.includes('[ref=e1]')) {
    fail(`expected compose ref in snapshot, got: ${navigated.snapshot}`);
  }

  const clicked = await rpc('click', { ref: 'e1' });
  if (!clicked.snapshot.includes('To')) {
    fail(`expected compose form after click, got: ${clicked.snapshot}`);
  }

  await rpc('type', { ref: 'e3', text: 'alice@example.com' });
  await rpc('type', { ref: 'e4', text: 'Hello' });
  const sent = await rpc('click', { ref: 'e6' });
  if (!sent.title.includes('sent')) {
    fail(`expected sent title, got: ${sent.title}`);
  }

  const status = await rpc('status', {});
  if (!status.live) {
    fail('status should report live session');
  }

  const stopped = await rpc('stop', {});
  if (!stopped.released) {
    fail('stop should release the session');
  }

  const local = await rpc('start', { backend: 'local' });
  if (local.backend !== 'local') {
    fail(`expected local backend, got: ${JSON.stringify(local)}`);
  }

  const stoppedLocal = await rpc('stop', {});
  if (!stoppedLocal.released) {
    fail('stop should release the local mock session');
  }

  child.stdin.end();
  await new Promise((resolve) => child.on('exit', resolve));
  process.stdout.write('ok\n');
}

main().catch((error) => fail(error.stack || error.message));
