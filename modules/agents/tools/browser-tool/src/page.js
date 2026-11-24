// Link Browser's reading of a page: its visible text, each part that
// can be clicked or typed into numbered where it stands, `[3]`. Run as
// an expression in a world of the tool's own, apart from the page's
// scripts, so nothing the page runs sees or changes it: the parts are
// kept there by number (`__linkParts`), never written on the page.
// Frames of the page's own process are read where they stand; another
// site's frame is named, `[iframe: <address>]`. Text the page hides
// from a person's eye (no opacity, no size, no color, clipped to a
// pixel, or off the page) isn't read. Its value is {title, url, text}.
(() => {
  const ACTS = 'a[href],button,input:not([type=hidden]),select,textarea,summary,' +
    '[role=button],[role=link],[role=checkbox],[role=radio],[role=tab],[role=menuitem],' +
    '[role=option],[role=switch],[contenteditable=""],[contenteditable=true],[onclick]';
  const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'SVG', 'HEAD']);
  const FRAMES = new Set(['IFRAME', 'FRAME']);
  const BLOCK = new Set(['P', 'DIV', 'SECTION', 'ARTICLE', 'HEADER', 'FOOTER', 'NAV', 'MAIN',
    'ASIDE', 'UL', 'OL', 'LI', 'TR', 'TABLE', 'FORM', 'FIELDSET', 'H1', 'H2', 'H3', 'H4', 'H5',
    'H6', 'PRE', 'BLOCKQUOTE', 'BR', 'HR', 'DL', 'DT', 'DD', 'LABEL', 'FIGURE', 'DETAILS']);
  const parts = [null];
  globalThis.__linkParts = parts;
  const out = [];
  const style = (e) => e.ownerDocument.defaultView.getComputedStyle(e);
  const shown = (e) => {
    const s = style(e);
    if (s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse') {
      return false;
    }
    const r = e.getBoundingClientRect();
    return r.width > 0 || r.height > 0 || s.display === 'contents';
  };
  // Hidden from the eye though laid out: see-through, no font, clipped to a pixel or less on either side, or placed
  // wholly off the page's top or left, where no scroll reaches. A part
  // so hidden is still a part (a styled checkbox is often see-through
  // over its picture).
  const clear = (color) => color === 'transparent' ||
    /^rgba\(.*,\s*0(\.0*)?\)$/.test(color);
  const unseen = (e) => {
    const s = style(e);
    if (Number(s.opacity) === 0 || parseFloat(s.fontSize) === 0) return true;
    if (s.display === 'contents') return false;
    const r = e.getBoundingClientRect();
    if ((s.overflowX !== 'visible' && r.width <= 1) ||
      (s.overflowY !== 'visible' && r.height <= 1)) return true;
    const view = e.ownerDocument.defaultView;
    return r.right + view.scrollX <= 0 || r.bottom + view.scrollY <= 0;
  };
  const words = (t) => (t || '').replace(/\s+/g, ' ').trim();
  const label = (e) => {
    const tag = e.tagName;
    const named = words(e.getAttribute('aria-label') || e.getAttribute('title') || '');
    if (tag === 'INPUT') {
      const type = (e.getAttribute('type') || 'text').toLowerCase();
      if (type === 'checkbox' || type === 'radio') {
        return `${type}${e.checked ? ' (checked)' : ''} ${named || words(e.labels?.[0]?.innerText)}`.trim();
      }
      if (type === 'submit' || type === 'button' || type === 'reset') {
        return `button "${words(e.value) || named || type}"`;
      }
      const hint = named || words(e.placeholder) || words(e.labels?.[0]?.innerText) || e.name || '';
      const value = type === 'password' ? (e.value ? '•••' : '') : words(e.value);
      return `${type} field${hint ? ` "${hint}"` : ''}${value ? ` = "${value}"` : ''}`;
    }
    if (tag === 'TEXTAREA') {
      const hint = named || words(e.placeholder) || e.name || '';
      return `text box${hint ? ` "${hint}"` : ''}${e.value ? ` = "${words(e.value).slice(0, 200)}"` : ''}`;
    }
    if (tag === 'SELECT') {
      const options = [...e.options].map((o) => (o.selected ? `*${words(o.text)}*` : words(o.text)));
      return `choice${named ? ` "${named}"` : ''}: ${options.slice(0, 30).join(' | ')}`;
    }
    if (e.isContentEditable && tag !== 'A' && tag !== 'BUTTON') {
      return `editable "${named || words(e.innerText).slice(0, 200)}"`;
    }
    const text = words(e.innerText) || named || words(e.querySelector('img')?.alt) || '';
    if (tag === 'A') return `link "${text}"`;
    return `button "${text}"`;
  };
  const walk = (node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      // Text with no color is unseen; its element's other children may
      // have a color of their own.
      if (node.parentElement && clear(style(node.parentElement).color)) return;
      const t = words(node.textContent);
      if (t) out.push(t + ' ');
      return;
    }
    if (node.nodeType !== Node.ELEMENT_NODE) return;
    const e = node;
    const tag = e.tagName.toUpperCase();
    if (SKIP.has(tag) || !shown(e)) return;
    if (FRAMES.has(tag)) {
      // Another site's document isn't reachable from here: named.
      let inner = null;
      try { inner = e.contentDocument; } catch (_) { inner = null; }
      if (inner && inner.body) {
        out.push('\n');
        walk(inner.body);
        out.push('\n');
      } else {
        out.push(`[iframe: ${words(e.getAttribute('src') || 'about:blank').slice(0, 200)}] `);
      }
      return;
    }
    const acts = e.matches(ACTS) && !e.disabled;
    if (!acts && unseen(e)) return;
    if (acts) {
      parts.push(e);
      const n = parts.length - 1;
      // A clickable wrapper around other parts: named by its own label,
      // then walked, so the parts inside it are numbered too.
      if (e.querySelector(ACTS)) {
        const named = words(e.getAttribute('aria-label') || e.getAttribute('title') || '');
        out.push(`[${n}] ${e.tagName === 'A' ? 'link' : 'button'}${named ? ` "${named}"` : ''}: `);
      } else {
        out.push(`[${n}] ${label(e)} `);
        return;
      }
    }
    const block = BLOCK.has(e.tagName);
    if (block) out.push('\n');
    if (/^H[1-6]$/.test(e.tagName)) out.push('#'.repeat(Number(e.tagName[1])) + ' ');
    if (e.tagName === 'LI') out.push('- ');
    for (const child of e.shadowRoot ? e.shadowRoot.childNodes : e.childNodes) walk(child);
    if (block) out.push('\n');
  };
  if (document.body) walk(document.body);
  const text = out.join('').replace(/[ \t]+\n/g, '\n').replace(/\n[ \t]+/g, '\n')
    .replace(/\n{3,}/g, '\n\n').trim();
  return { title: document.title, url: location.href, text, count: parts.length - 1 };
})()
