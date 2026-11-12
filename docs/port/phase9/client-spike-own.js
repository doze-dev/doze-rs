// The own-script candidate: no library. EventSource for the stream, fetch for fragments and forms, a small morph.
(() => {
  const $ = (s) => document.querySelector(s);
  const body = document.body;
  const ids = body.dataset.watch.split(',');
  const tapeId = ids.find((i) => i.startsWith('tape'));
  const etags = {}, ctl = {};
  let paused = false, pausedAt = 0, rows = 0, es;
  const banner = (on) => { $('#banner').hidden = !on; };
  const parse = (html) => { const t = document.createElement('template'); t.innerHTML = html; return t.content; };

  // Edit the nodes on screen into the new ones, so focus, caret, typed text and scroll position stay.
  function morph(a, b) {
    if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return a.replaceWith(b);
    if (a.nodeType === 3) { if (a.data !== b.data) a.data = b.data; return; }
    const focused = a === document.activeElement;
    for (const { name, value } of [...b.attributes]) if (a.getAttribute(name) !== value && !(focused && name === 'value')) a.setAttribute(name, value);
    for (const { name } of [...a.attributes]) if (!b.hasAttribute(name)) a.removeAttribute(name);
    if (a.localName === 'input' && !focused && a.value !== b.value) a.value = b.value;
    const ac = [...a.childNodes], bc = [...b.childNodes];
    bc.forEach((n, i) => (ac[i] ? morph(ac[i], n) : a.append(n)));
    ac.slice(bc.length).forEach((n) => n.remove());
  }
  // a response is HTML whose top-level elements have ids: each one morphs into the element with that id
  function apply(html) { for (const el of parse(html).children) { const t = document.getElementById(el.id); if (t) morph(t, el); } }

  async function refresh(id) {
    ctl[id]?.abort(); // newest wins
    const c = (ctl[id] = new AbortController());
    const q = id === tapeId ? '?client=' + encodeURIComponent($('#filter').value) : '';
    try {
      const r = await fetch(`/frag/${id}${q}`, { signal: c.signal, headers: { 'X-Fragment': '1', ...(etags[id] && { 'If-None-Match': etags[id] }) } });
      if (r.status === 304) return; // unchanged: no body, no swap
      etags[id] = r.headers.get('ETag');
      apply(await r.text());
    } catch (e) { if (e.name !== 'AbortError') banner(true); }
  }

  function connect() {
    es?.close();
    es = new EventSource('/events?watch=' + ids.join(','));
    es.addEventListener('tick', (e) => {
      const d = JSON.parse(e.data);
      rows = d.rows;
      if (paused) held(); else d.ids.forEach(refresh);
    });
    es.onopen = () => banner(false);
    es.onerror = () => { banner(true); if (es.readyState === 2) { es.close(); setTimeout(connect, 1000); } };
  }
  const held = () => { $('#held').textContent = paused ? `${rows - pausedAt} held back` : ''; };

  body.addEventListener('click', async (e) => {
    const b = e.target.closest('#pause');
    if (b) {
      paused = !paused;
      pausedAt = rows;
      b.setAttribute('aria-pressed', paused);
      b.textContent = paused ? 'Resume' : 'Pause';
      held();
      if (!paused) ids.forEach(refresh);
    }
    const a = e.target.closest('a[data-dialog]');
    if (a) {
      e.preventDefault();
      const d = $('#dlg');
      d.replaceChildren(parse(await (await fetch(a.href, { headers: { 'X-Fragment': '1' } })).text()));
      d.showModal();
    }
  });
  let t;
  body.addEventListener('input', (e) => { if (e.target.id === 'filter') { clearTimeout(t); t = setTimeout(() => refresh(tapeId), 150); } });
  body.addEventListener('submit', async (e) => {
    const f = e.target.closest('form[data-ajax]');
    if (!f) return;
    e.preventDefault();
    const r = await fetch(f.action, { method: 'POST', body: new URLSearchParams(new FormData(f)), headers: { 'X-Fragment': '1' } });
    apply(await r.text());
    f.closest('dialog')?.close();
  });
  // a page in the back/forward cache must not keep its stream open (HTTP/1.1 allows 6 connections per origin)
  // and a hidden tab does not hold one either
  document.addEventListener('visibilitychange', () => (document.hidden ? es.close() : connect()));
  addEventListener('pagehide', () => es.close());
  addEventListener('pageshow', (e) => { if (e.persisted) connect(); });
  connect();
})();
