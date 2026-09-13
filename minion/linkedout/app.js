// LinkedOut demo state. Everything is in-memory; nothing persists.
'use strict';

const PEOPLE = [
  { id: 'chad-hardcastle', name: 'Chad Hardcastle', title: 'VP of Synergy' },
  { id: 'tabitha-lampert', name: 'Tabitha Lampert', title: 'Chief Momentum Officer' },
  { id: 'dov-quintanilla', name: 'Dov Quintanilla', title: 'Head of Vibes' },
  { id: 'muriel-blane', name: 'Muriel Blane', title: 'Disruptor at Large' },
  { id: 'preston-hodge', name: 'Preston Hodge', title: 'Growth Alchemist' },
  { id: 'yolanda-strife', name: 'Yolanda Strife', title: 'Villainy Specialist' },
];

const posts = [
  {
    author: 'Chad Hardcastle',
    body: 'Hot take: if your standup is under 45 minutes, you\'re not aligned enough. #Synergy',
    timestamp: '2h',
  },
  {
    author: 'Muriel Blane',
    body: 'Just closed a series B for my no-code tool that removes the code. AMA.',
    timestamp: '5h',
  },
];

function el(tag, props = {}, children = []) {
  const node = document.createElement(tag);
  Object.entries(props).forEach(([key, value]) => {
    if (key === 'className') {
      node.className = value;
    } else if (key.startsWith('data-')) {
      node.setAttribute(key, value);
    } else {
      node[key] = value;
    }
  });
  (Array.isArray(children) ? children : [children]).forEach((child) => {
    if (child == null) {
      return;
    }
    node.appendChild(typeof child === 'string' ? document.createTextNode(child) : child);
  });
  return node;
}

function renderPosts() {
  const container = document.getElementById('posts');
  container.innerHTML = '';
  posts.forEach((post) => {
    container.appendChild(
      el('article', { className: 'post' }, [
        el('div', {}, [
          el('span', { className: 'author' }, post.author),
          el('span', { className: 'timestamp' }, post.timestamp),
        ]),
        el('p', {}, post.body),
      ]),
    );
  });
}

function renderPeople() {
  const list = document.getElementById('people-list');
  const dmSelect = document.getElementById('dm-target');
  list.innerHTML = '';
  dmSelect.innerHTML = '';
  PEOPLE.forEach((person) => {
    const button = el('button', {
      className: 'secondary',
      'data-testid': `endorse-${person.id}`,
    }, `Endorse for Villainy`);
    button.addEventListener('click', () => {
      button.textContent = `Endorsed ✓`;
      button.disabled = true;
    });
    list.appendChild(
      el('li', { className: 'person', 'data-testid': `person-${person.id}` }, [
        el('div', {}, [
          el('div', { className: 'author' }, person.name),
          el('div', { className: 'timestamp' }, person.title),
        ]),
        button,
      ]),
    );
    const option = el('option', { value: person.id }, `${person.name} — ${person.title}`);
    dmSelect.appendChild(option);
  });
}

document.addEventListener('DOMContentLoaded', () => {
  renderPosts();
  renderPeople();

  document.getElementById('composer').addEventListener('submit', (event) => {
    event.preventDefault();
    const textarea = document.getElementById('new-post');
    const body = textarea.value.trim();
    if (!body) {
      return;
    }
    posts.unshift({
      author: document.getElementById('my-name').textContent,
      body,
      timestamp: 'now',
    });
    textarea.value = '';
    renderPosts();
  });

  document.getElementById('edit-headline').addEventListener('click', () => {
    const current = document.getElementById('my-headline').textContent;
    // eslint-disable-next-line no-alert
    const next = prompt('New headline?', current);
    if (next) {
      document.getElementById('my-headline').textContent = next;
    }
  });

  document.getElementById('dm-send').addEventListener('click', () => {
    const target = document.getElementById('dm-target');
    const body = document.getElementById('dm-body');
    const message = body.value.trim();
    if (!message) {
      return;
    }
    const person = PEOPLE.find((p) => p.id === target.value);
    const log = document.getElementById('dm-log');
    log.appendChild(
      el('li', { 'data-testid': `dm-${person.id}` }, `${person.name}: ${message}`),
    );
    body.value = '';
  });
});
