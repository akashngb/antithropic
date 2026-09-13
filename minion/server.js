#!/usr/bin/env node
// Tiny static server for the LinkedOut mock site. No auth, no analytics,
// binds only to 127.0.0.1 so nothing here can be reached from outside the
// container.
'use strict';

const http = require('http');
const fs = require('fs');
const path = require('path');

const HOST = '127.0.0.1';
const PORT = Number(process.env.LINKEDOUT_PORT || 4242);
const ROOT = path.join(__dirname, 'linkedout');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
};

function safeJoin(base, target) {
  const requested = path.normalize(target).replace(/^([\/\\])+/, '');
  const resolved = path.resolve(base, requested);
  if (!resolved.startsWith(path.resolve(base))) {
    return null;
  }
  return resolved;
}

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://${HOST}:${PORT}`);
  let filePath = safeJoin(ROOT, url.pathname === '/' ? '/index.html' : url.pathname);
  if (!filePath) {
    res.writeHead(400).end('bad request');
    return;
  }
  fs.stat(filePath, (err, stat) => {
    if (err) {
      res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found');
      return;
    }
    if (stat.isDirectory()) {
      filePath = path.join(filePath, 'index.html');
    }
    fs.readFile(filePath, (readErr, data) => {
      if (readErr) {
        res.writeHead(404, { 'Content-Type': 'text/plain' }).end('not found');
        return;
      }
      const ext = path.extname(filePath).toLowerCase();
      res.writeHead(200, {
        'Content-Type': MIME[ext] || 'application/octet-stream',
        'Cache-Control': 'no-store',
      });
      res.end(data);
    });
  });
});

server.listen(PORT, HOST, () => {
  // eslint-disable-next-line no-console
  console.log(`LinkedOut serving on http://${HOST}:${PORT}`);
});
