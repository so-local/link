// Where part `n` of the last read is in the window, brought into view,
// and what a click there would land on. Run in the tool's own world, as
// a function of `n`. Its value: null when the part is gone; else {x, y}
// in the window, and `covered` (a few words, the part's number when it's
// one) when something else is on top of it at that point.
((n) => {
  const parts = globalThis.__linkParts || [];
  const e = parts[n];
  if (!e || !e.isConnected) return null;
  e.scrollIntoView({ block: 'center', inline: 'center' });
  const r = e.getBoundingClientRect();
  let x = r.left + r.width / 2;
  let y = r.top + r.height / 2;
  // A part in a frame: its frames' places added, out to the window.
  let view = e.ownerDocument.defaultView;
  while (view && view.frameElement) {
    const frame = view.frameElement;
    const f = frame.getBoundingClientRect();
    x += f.left + frame.clientLeft;
    y += f.top + frame.clientTop;
    view = frame.ownerDocument.defaultView;
  }
  // What's on top at that point, into frames and shadow roots.
  let hit = document.elementFromPoint(x, y);
  let [hx, hy] = [x, y];
  for (let depth = 0; hit && depth < 32; depth += 1) {
    if (hit.shadowRoot) {
      const inner = hit.shadowRoot.elementFromPoint(hx, hy);
      if (!inner || inner === hit) break;
      hit = inner;
      continue;
    }
    let inner = null;
    try { inner = hit.contentDocument; } catch (_) { inner = null; }
    if (!inner) break;
    const f = hit.getBoundingClientRect();
    [hx, hy] = [hx - f.left - hit.clientLeft, hy - f.top - hit.clientTop];
    const next = inner.elementFromPoint(hx, hy);
    if (!next) break;
    hit = next;
  }
  // The part, something in it, or its label takes the click.
  const within = (node) => {
    for (let at = node; at; at = at.parentNode || at.host) {
      if (at === e) return true;
    }
    return false;
  };
  const labelled = hit && [...(e.labels || [])].some((l) => l.contains(hit));
  if (!hit || within(hit) || labelled || hit.contains(e)) return { x, y };
  let number = -1;
  for (let at = hit; at && number < 1; at = at.parentNode || at.host) number = parts.indexOf(at);
  const words = (hit.innerText || hit.getAttribute('aria-label') || '').replace(/\s+/g, ' ')
    .trim().slice(0, 80);
  const covered = `${hit.tagName.toLowerCase()}${words ? ` "${words}"` : ''}` +
    `${number > 0 ? ` ([${number}])` : ''}`;
  return { x, y, covered };
})
