/* IR Blaster downloads page.
 *
 * Builds the firmware tables from firmware/manifest.json (written by publish_release.py), so a new
 * publish needs no change here, and summarises catalog/index.json once that section is near the
 * viewport (it is about 200 KB compressed). Paths are relative, so the page works from any host.
 */
(() => {
  'use strict';

  const MANIFEST_URL = 'firmware/manifest.json';
  const FIRMWARE_DIR = 'firmware/';
  const CATALOG_URL = 'catalog/index.json';

  /* Known chips: display name and the boards each image is built for. Unknown chips still list. */
  const CHIPS = {
    esp32: {
      label: 'ESP32',
      node: 'ESP32 DevKit (30-pin), WEMOS D1 Mini ESP32',
      hub: 'ESP32 DevKit',
    },
    esp32c3: {
      label: 'ESP32-C3',
      node: 'ESP32-C3 Super Mini, Waveshare ESP32-C3-Zero',
    },
    esp32s3: {
      label: 'ESP32-S3',
      hub: 'ESP32-S3-DevKitC-1',
    },
  };

  const nf = new Intl.NumberFormat('en');

  /* ---------------------------------------------------------------- helpers */

  function el(tag, attrs, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(attrs || {})) {
      if (value === undefined || value === null || value === false) continue;
      if (key === 'class') node.className = value;
      else if (key === 'text') node.textContent = value;
      else node.setAttribute(key, value === true ? '' : value);
    }
    for (const child of children.flat()) {
      if (child === undefined || child === null || child === false) continue;
      node.append(child instanceof Node ? child : document.createTextNode(String(child)));
    }
    return node;
  }

  function icon(name) {
    const ns = 'http://www.w3.org/2000/svg';
    const svg = document.createElementNS(ns, 'svg');
    svg.setAttribute('class', 'icon');
    svg.setAttribute('aria-hidden', 'true');
    const use = document.createElementNS(ns, 'use');
    use.setAttribute('href', `#i-${name}`);
    svg.append(use);
    return svg;
  }

  function versionKey(version) {
    return String(version || '').split('.').filter(Boolean).map((part) => parseInt(part, 10) || 0);
  }

  function compareVersions(a, b) {
    const ka = versionKey(a);
    const kb = versionKey(b);
    for (let i = 0; i < Math.max(ka.length, kb.length); i++) {
      const diff = (ka[i] || 0) - (kb[i] || 0);
      if (diff) return diff;
    }
    return 0;
  }

  function formatSize(bytes) {
    if (!Number.isFinite(bytes)) return '—';
    if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  }

  function chipInfo(chip) {
    return CHIPS[chip] || { label: chip.toUpperCase() };
  }

  async function fetchJson(url) {
    // no-cache: revalidate with GitHub Pages so a fresh publish shows straight away
    const res = await fetch(url, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`${url} answered HTTP ${res.status}`);
    return res.json();
  }

  function errorCallout(message, url) {
    return el('p', { class: 'callout callout-danger' },
      icon('alert'),
      el('span', {},
        `${message} `,
        el('a', { href: url, text: url }),
        ' may still open directly.'));
  }

  /* ---------------------------------------------------------------- firmware */

  async function copyText(text) {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return;
    }
    const area = el('textarea', { readonly: true, style: 'position:fixed;opacity:0' });
    area.value = text;
    document.body.append(area);
    area.select();
    const ok = document.execCommand('copy');
    area.remove();
    if (!ok) throw new Error('copy refused');
  }

  function hashCell(sha256, what) {
    if (!sha256) return el('span', { class: 'muted', text: '—' });
    const button = el('button', {
      class: 'btn btn-ghost btn-icon btn-sm',
      type: 'button',
      title: 'Copy SHA-256',
      'aria-label': `Copy the SHA-256 of the ${what} image`,
    }, icon('copy'));
    button.addEventListener('click', async () => {
      try {
        await copyText(sha256);
        button.classList.add('is-copied');
        button.replaceChildren(icon('check'));
        button.setAttribute('title', 'Copied');
      } catch {
        button.setAttribute('title', 'Copy failed: select the hash instead');
      }
      clearTimeout(button._reset);
      button._reset = setTimeout(() => {
        button.classList.remove('is-copied');
        button.replaceChildren(icon('copy'));
        button.setAttribute('title', 'Copy SHA-256');
      }, 1600);
    });
    return el('span', { class: 'hash' },
      el('code', { title: sha256, text: `${sha256.slice(0, 12)}…` }),
      button);
  }

  function signatureTag(signed) {
    return signed
      ? el('span', { class: 'tag tag-ok' }, icon('lock'), 'Signed')
      : el('span', { class: 'tag tag-warn', title: 'Devices armed with a signing key refuse unsigned images' }, 'Unsigned');
  }

  function firmwareTable(role, builds) {
    const entries = Object.entries(builds || {})
      .filter(([, build]) => build && typeof build === 'object' && build.file)
      .sort(([a], [b]) => a.localeCompare(b));
    if (!entries.length) {
      return el('p', { class: 'fw-empty', text: role === 'hub' ? 'No hub image is published yet.' : 'No node images are published yet.' });
    }
    const roleName = role === 'hub' ? 'hub' : 'node';
    const rows = entries.map(([chip, build]) => {
      const info = chipInfo(chip);
      const boards = info[roleName];
      const what = `${info.label} ${roleName}`;
      return el('tr', {},
        el('td', { class: 'col-chip' },
          el('div', { class: 'chip' },
            el('span', { class: 'chip-glyph' }, icon(role === 'hub' ? 'hub' : 'node')),
            el('div', {},
              el('div', { class: 'chip-name', text: info.label }),
              boards ? el('div', { class: 'chip-boards', text: boards }) : null))),
        el('td', { class: 'col-ver', 'data-label': 'Version' }, el('span', { class: 'fw-ver', text: build.version || '—' })),
        el('td', { class: 'col-size num', 'data-label': 'Size', title: Number.isFinite(build.size) ? `${nf.format(build.size)} bytes` : null, text: formatSize(build.size) }),
        el('td', { class: 'col-sig', 'data-label': 'Signature' }, signatureTag(build.signed === true)),
        el('td', { class: 'col-hash', 'data-label': 'SHA-256' }, hashCell(build.sha256, what)),
        el('td', { class: 'col-dl' },
          el('a', {
            class: 'btn btn-sm btn-primary',
            href: FIRMWARE_DIR + encodeURIComponent(build.file),
            download: build.file,
            'aria-label': `Download the ${what} image, version ${build.version || 'unknown'}`,
          }, icon('download'), 'Download')));
    });
    return el('table', { class: 'fw-table' },
      el('caption', { class: 'sr-only', text: role === 'hub' ? 'Hub firmware' : 'Node firmware' }),
      el('thead', {}, el('tr', {},
        el('th', { scope: 'col', class: 'col-chip', text: 'Chip' }),
        el('th', { scope: 'col', class: 'col-ver', text: 'Version' }),
        el('th', { scope: 'col', class: 'col-size', text: 'Size' }),
        el('th', { scope: 'col', class: 'col-sig', text: 'Signature' }),
        el('th', { scope: 'col', class: 'col-hash', text: 'SHA-256' }),
        el('th', { scope: 'col', class: 'col-dl' }, el('span', { class: 'sr-only', text: 'Download' })))),
      el('tbody', {}, rows));
  }

  function setRelease(state, ...content) {
    const release = document.getElementById('release');
    release.dataset.state = state;
    document.getElementById('release-text').replaceChildren(...content);
  }

  async function loadFirmware() {
    const nodesBox = document.getElementById('fw-nodes');
    const hubsBox = document.getElementById('fw-hubs');
    try {
      const manifest = await fetchJson(MANIFEST_URL);
      nodesBox.replaceChildren(firmwareTable('node', manifest.nodes));
      hubsBox.replaceChildren(firmwareTable('hub', manifest.hubs));
      const versions = [...Object.values(manifest.nodes || {}), ...Object.values(manifest.hubs || {})]
        .map((build) => build && build.version)
        .filter(Boolean)
        .sort(compareVersions);
      const latest = versions[versions.length - 1];
      if (latest) setRelease('live', el('span', { class: 'link-label', text: 'Latest ' }), el('code', { text: latest }));
      else setRelease('down', 'No releases yet');
    } catch (err) {
      console.error(err);
      nodesBox.replaceChildren(el('div', { class: 'fw-empty' }, errorCallout('Couldn’t load the firmware list.', MANIFEST_URL)));
      hubsBox.replaceChildren(el('p', { class: 'fw-empty', text: 'Unavailable until the firmware list loads.' }));
      setRelease('down', 'Releases unavailable');
    }
  }

  /* ---------------------------------------------------------------- catalog */

  /* Source notices carry a URL; link it, keep the rest as text. */
  function linkified(text) {
    const parts = [];
    let last = 0;
    for (const match of String(text).matchAll(/https?:\/\/[^\s)]+/g)) {
      let url = match[0].replace(/[.,;]+$/, '');
      parts.push(text.slice(last, match.index));
      parts.push(el('a', { href: url, rel: 'noopener', text: url }));
      last = match.index + url.length;
    }
    parts.push(text.slice(last));
    return parts;
  }

  function renderCatalog(index) {
    const remotes = Array.isArray(index.remotes) ? index.remotes.length : 0;
    const brands = index.brands && typeof index.brands === 'object' ? Object.keys(index.brands).length : 0;
    const categories = Array.isArray(index.categories) ? index.categories : [];
    const sources = Array.isArray(index.sources) ? index.sources : [];

    const stat = (label, value) => el('div', { class: 'stat' }, el('dt', { text: label }), el('dd', { text: nf.format(value) }));
    const statsList = el('dl', { class: 'stats' },
      stat('Remotes', remotes),
      stat('Brands', brands),
      stat('Device types', categories.length));

    const cats = el('div', { class: 'cat-tags', role: 'list', 'aria-label': 'Remotes by device type' },
      categories
        .slice()
        .sort((a, b) => (b.remotes || 0) - (a.remotes || 0))
        .map((cat) => el('span', { class: 'tag tag-muted', role: 'listitem' },
          cat.label || cat.key, el('b', { text: nf.format(cat.remotes || 0) }))));

    const sourceList = el('ul', { class: 'sources', 'aria-label': 'Sources' },
      sources.map((src) => el('li', { class: 'card source' },
        el('div', { class: 'source-name', text: src.name || src.id }),
        src.notice ? el('p', { class: 'source-notice' }, linkified(src.notice)) : null)));

    document.getElementById('catalog-body').replaceChildren(statsList, cats, sourceList);
  }

  async function loadCatalog() {
    try {
      renderCatalog(await fetchJson(CATALOG_URL));
    } catch (err) {
      console.error(err);
      document.getElementById('catalog-body').replaceChildren(errorCallout('Couldn’t load the catalog summary.', CATALOG_URL));
    }
  }

  function loadCatalogWhenNear() {
    const section = document.getElementById('catalog');
    if (!('IntersectionObserver' in window)) { loadCatalog(); return; }
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        observer.disconnect();
        loadCatalog();
      }
    }, { rootMargin: '400px 0px' });
    observer.observe(section);
  }

  /* ---------------------------------------------------------------- guides & nav */

  /* Guides marked data-soon are placeholders: announce them as unavailable and keep them inert. */
  function holdPlaceholders() {
    for (const link of document.querySelectorAll('a[data-soon]')) {
      link.setAttribute('aria-disabled', 'true');
      link.addEventListener('click', (event) => event.preventDefault());
    }
  }

  /* Underline the nav entry of the section being read, as in the hub's header. */
  function trackSections() {
    if (!('IntersectionObserver' in window)) return;
    const links = new Map([...document.querySelectorAll('.nav a[href^="#"]')]
      .map((a) => [a.getAttribute('href').slice(1), a]));
    const visible = new Set();
    const observer = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) visible.add(entry.target.id);
        else visible.delete(entry.target.id);
      }
      const current = [...links.keys()].find((id) => visible.has(id));
      for (const [id, a] of links) {
        if (id === current) a.setAttribute('aria-current', 'true');
        else a.removeAttribute('aria-current');
      }
    }, { rootMargin: '-30% 0px -55% 0px' });
    for (const id of links.keys()) {
      const section = document.getElementById(id);
      if (section) observer.observe(section);
    }
  }

  holdPlaceholders();
  trackSections();
  loadFirmware();
  loadCatalogWhenNear();
})();
