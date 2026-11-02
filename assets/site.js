/* IR Blaster downloads page.
 *
 * Builds the firmware tables from firmware/manifest.json (written by publish_release.py), so a new
 * publish needs no change here, lists the downloads of the newest recovery-v* GitHub Release, and
 * summarises catalog/index.json once that section is near the viewport (it is about 200 KB
 * compressed). Paths are relative, so the page works from any host.
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

  /* ---------------------------------------------------------------- recovery tool */

  const RELEASES_API = 'https://api.github.com/repos/olkham/irblaster-data/releases?per_page=30';
  const RELEASES_PAGE = 'https://github.com/olkham/irblaster-data/releases';
  /* The files .github/workflows/recovery.yml attaches to each release, by name suffix. */
  const PLATFORMS = [
    { suffix: '-windows-x64.exe', os: 'windows', name: 'Windows', req: 'Windows 10 and 11, 64-bit' },
    { suffix: '-macos.zip', os: 'mac', name: 'macOS', req: 'macOS 11 or later, Apple silicon and Intel' },
    { suffix: '-linux-x64.tar.gz', os: 'linux', name: 'Linux x86-64', req: 'Ubuntu 22.04, Debian 12, Fedora 36 or later' },
    { suffix: '-linux-arm64.tar.gz', os: 'linux-arm', name: 'Linux ARM64', req: 'e.g. Raspberry Pi OS 64-bit' },
  ];

  function visitorOs() {
    const ua = navigator.userAgent || '';
    const platform = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || ua;
    if (/win/i.test(platform)) return 'windows';
    if (/mac/i.test(platform)) return 'mac';
    if (/linux|x11|cros/i.test(platform)) return /aarch64|arm/i.test(platform + ua) ? 'linux-arm' : 'linux';
    return '';
  }

  function recoveryMessage(...content) {
    document.getElementById('rec-body').replaceChildren(el('div', { class: 'card rec-empty' }, ...content));
  }

  function renderRecovery(release) {
    if (!release) {
      recoveryMessage('Not released yet: the first version will appear here.');
      return;
    }
    const version = String(release.tag_name).replace(/^recovery-v/, '');
    const assets = Array.isArray(release.assets) ? release.assets : [];
    const yours = visitorOs();
    const options = PLATFORMS.map((platform) => {
      const asset = assets.find((a) => String(a.name).endsWith(platform.suffix));
      if (!asset) return null;
      const mine = platform.os === yours;
      return el('div', { class: `card rec-option${mine ? ' is-yours' : ''}` },
        el('div', { class: 'rec-top' },
          el('span', { class: 'chip-glyph' }, icon('desktop')),
          el('div', {},
            el('div', { class: 'rec-os', text: platform.name }),
            el('div', { class: 'rec-req', text: platform.req }))),
        el('div', { class: 'rec-meta', text: `${asset.name} · ${formatSize(asset.size)}` }),
        el('a', {
          class: `btn btn-sm${mine ? ' btn-primary' : ''}`,
          href: asset.browser_download_url,
          'aria-label': `Download IR Blaster Recovery ${version} for ${platform.name}`,
        }, icon('download'), 'Download'));
    }).filter(Boolean);
    if (!options.length) {
      recoveryMessage(`Version ${version} has no downloads yet. `, el('a', { href: release.html_url || RELEASES_PAGE, text: 'See the release' }), '.');
      return;
    }
    const published = release.published_at ? new Date(release.published_at) : null;
    const date = published && !Number.isNaN(published.getTime())
      ? published.toLocaleDateString('en-GB', { day: 'numeric', month: 'short', year: 'numeric' })
      : '';
    const sums = assets.find((a) => a.name === 'SHA256SUMS.txt');
    document.getElementById('rec-body').replaceChildren(
      el('div', { class: 'rec-head' },
        el('span', { class: 'rec-version' }, 'Version ', el('code', { text: version })),
        date ? el('span', { class: 'rec-date', text: `released ${date}` }) : null,
        sums ? el('a', { class: 'rec-sums', href: sums.browser_download_url, text: 'SHA256SUMS.txt' }) : null),
      el('div', { class: 'rec-grid' }, options));
  }

  async function loadRecovery() {
    try {
      const res = await fetch(RELEASES_API, { headers: { Accept: 'application/vnd.github+json' } });
      if (!res.ok) throw new Error(`GitHub answered HTTP ${res.status}`);
      const releases = await res.json();
      const release = (Array.isArray(releases) ? releases : [])
        .find((r) => r && !r.draft && !r.prerelease && String(r.tag_name).startsWith('recovery-v'));
      renderRecovery(release || null);
    } catch (err) {
      console.error(err);
      recoveryMessage('Couldn’t check for the latest version. ', el('a', { href: RELEASES_PAGE, text: 'See all releases' }), '.');
    }
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
  loadRecovery();
  loadCatalogWhenNear();
})();
