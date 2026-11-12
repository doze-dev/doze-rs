// The console's own script: no library. Pages are complete without it; this adds what a script can: controls that need one
// (copy), fragments that refetch when the server says they changed, forms that swap their answer in place, and the banner
// when the stream is lost. Every name below is defined in src/ui/client.rs, and a test checks that they agree.
// Measured by the D4a spike (docs/port/phase9/client-spike.md); the rules it found are marked (spike n).
(() => {
  const attr = (el, name) => (el ? el.getAttribute(name) : null);
  const banner = document.getElementById('stream-banner');
  document.querySelectorAll('[data-js-only]').forEach((el) => { el.hidden = false; });

  // Copy: the text of the button, or of the element it names. "Copy as" changes the form of the address, for the buttons
  // that carry the address (the client snippets, data-copy-forms) and for no other: a card that copies its own lines copies those.
  let copyAs = 'code';
  const address = () => attr(document.querySelector('[data-address]'), 'data-address') || '';
  const textOf = (btn) => {
    const own = attr(btn, 'data-copy');
    if (own !== null) return own;
    if (btn.hasAttribute('data-copy-forms')) {
      if (copyAs === 'properties') return 'bootstrap.servers=' + address();
      if (copyAs === 'environment') return 'KAFKA_BOOTSTRAP_SERVERS=' + address();
    }
    const from = document.getElementById(attr(btn, 'data-copy-from'));
    return from ? from.textContent : '';
  };
  // The clipboard API exists only in a secure context (https or localhost): over http://<LAN address> it is absent, and the
  // old way (select the text of a field and ask the document to copy it) still works there.
  function copyOld(text) {
    const was = document.activeElement;
    const field = document.createElement('textarea');
    field.value = text;
    field.setAttribute('readonly', '');
    field.style.position = 'fixed';
    field.style.opacity = '0';
    document.body.append(field);
    field.select();
    try { document.execCommand('copy'); } catch (e) { /* nothing to do: the text is on the page */ }
    field.remove();
    if (was && was.focus) was.focus();
  }
  function copy(text) {
    if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).catch(() => copyOld(text));
    else copyOld(text);
  }
  document.addEventListener('click', (e) => {
    const pick = e.target.closest('[data-copy-as]');
    if (pick) {
      copyAs = attr(pick, 'data-copy-as');
      pick.parentElement.querySelectorAll('[data-copy-as]').forEach((b) => b.classList.toggle('on', b === pick));
      return;
    }
    const btn = e.target.closest('[data-copy],[data-copy-from]');
    if (btn) copy(textOf(btn));
  });

  // Edit the nodes on screen into the new ones, so focus, typed text, caret and scroll position stay.
  function morph(a, b) {
    if (a.nodeType !== b.nodeType || a.nodeName !== b.nodeName) return a.replaceWith(b);
    if (a.nodeType === 3) { if (a.data !== b.data) a.data = b.data; return; }
    if (a.nodeType !== 1) return;
    const focused = a === document.activeElement;
    for (const { name, value } of [...b.attributes]) if (a.getAttribute(name) !== value && !(focused && name === 'value')) a.setAttribute(name, value);
    for (const { name } of [...a.attributes]) if (!b.hasAttribute(name)) a.removeAttribute(name);
    if (a.localName === 'input' && !focused && a.value !== b.value) a.value = b.value;
    const ac = [...a.childNodes], bc = [...b.childNodes];
    bc.forEach((n, i) => (ac[i] ? morph(ac[i], n) : a.append(n)));
    ac.slice(bc.length).forEach((n) => n.remove());
  }
  // An answer is HTML whose top-level elements have ids: each one morphs into the element with that id.
  function apply(html) {
    const t = document.createElement('template');
    t.innerHTML = html;
    for (const el of t.content.children) { const target = el.id && document.getElementById(el.id); if (target) morph(target, el); }
  }

  // A fragment: fetched again on a tick. The script keeps the ETag and sends it (spike 1: a browser turns a 304 into a 200
  // for fetch), and the newest request wins per fragment (spike 4).
  const etags = {}, ctl = {};
  // A fetch that came back (even with 304) says the console's process is there: the banner of a lost connection goes.
  const reached = () => { if (banner) banner.hidden = true; };
  async function refresh(id) {
    const url = attr(document.getElementById(id), 'data-frag');
    if (!url) return;
    if (ctl[id]) ctl[id].abort();
    const c = (ctl[id] = new AbortController());
    try {
      const headers = etags[id] ? { 'If-None-Match': etags[id] } : {};
      const r = await fetch(url, { signal: c.signal, headers });
      if (r.status === 304) { reached(); return; }
      if (!r.ok) throw new Error(String(r.status));
      etags[id] = r.headers.get('ETag');
      apply(await r.text());
      reached();
    } catch (e) { if (e.name !== 'AbortError' && banner) banner.hidden = false; }
  }

  // A form: posted in place; the answer is the fragment named by data-target. If that fails the form posts as it would without us.
  document.addEventListener('submit', async (e) => {
    const f = e.target.closest('form[data-ajax]');
    if (!f) return;
    e.preventDefault();
    try {
      const r = await fetch(f.action, { method: 'POST', body: new URLSearchParams(new FormData(f)), headers: { 'X-Fragment': '1' } });
      if (!r.ok) throw new Error(String(r.status));
      apply(await r.text());
    } catch (err) { f.submit(); }
  });

  // The stream: one connection while the page is on screen. A tick names the fragments to fetch again.
  const url = attr(document.body, 'data-stream');
  if (!url || !window.EventSource) return;
  let es = null, wait = 1000, timer = 0;
  const close = () => { clearTimeout(timer); if (es) es.close(); es = null; };
  const connect = () => {
    close();
    es = new EventSource(url);
    es.addEventListener('tick', (e) => e.data.split(',').filter(Boolean).forEach(refresh));
    es.onopen = () => { wait = 1000; if (banner) banner.hidden = true; };
    // Reconnect for ever with a growing wait (spike 3): the console left open while the broker is down must come back.
    es.onerror = () => {
      if (banner) banner.hidden = false;
      close();
      timer = setTimeout(connect, wait);
      wait = Math.min(wait * 2, 15000);
    };
  };
  // A page in the back/forward cache or a hidden tab must not hold a stream open: a browser allows 6 connections per origin (spike 2).
  document.addEventListener('visibilitychange', () => (document.hidden ? close() : connect()));
  addEventListener('pagehide', close);
  addEventListener('pageshow', (e) => { if (e.persisted) connect(); });
  connect();
})();
