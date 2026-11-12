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
      const html = await r.text();
      if (holds(id, html)) { reached(); return; }
      const before = rowsOn(id);
      apply(html);
      markNew(id, before);
      reached();
    } catch (e) { if (e.name !== 'AbortError' && banner) banner.hidden = false; }
  }

  // Live (slice 9e). A fragment that carries a [data-seen] (a count that grows with every request recorded) is a tape: while it
  // is paused, or a drawer is open over it, new answers are kept aside and counted, so what a person reads never moves.
  // Pause is a link that works with no script (the page as it was); here it holds the tape in place instead. Resume (or
  // closing the drawer) brings the newest in. New rows are marked for a moment so the eye finds them.
  const held = {};
  let pausedAt = null;
  const seenOf = (root) => { const s = root && root.querySelector('[data-seen]'); return s ? Number(s.getAttribute('data-seen')) : null; };
  const overlayOpen = () => !!document.querySelector('[data-overlay],dialog[open]');
  const parse = (html) => { const t = document.createElement('template'); t.innerHTML = html; return t.content; };
  const showHeld = (n) => document.querySelectorAll('[data-held]').forEach((el) => { el.textContent = new Intl.NumberFormat('en-US').format(n); });
  function holds(id, html) {
    const el = document.getElementById(id);
    if (!el || seenOf(el) === null) return false;
    if (pausedAt === null && !overlayOpen()) { delete held[id]; return false; }
    held[id] = html;
    if (pausedAt !== null) showHeld(Math.max(0, seenOf(parse(html)) - pausedAt));
    return true;
  }
  function release() {
    for (const id of Object.keys(held)) { const html = held[id]; delete held[id]; const b = rowsOn(id); apply(html); markNew(id, b); }
  }
  const rowsOn = (id) => { const el = document.getElementById(id); return el ? new Set([...el.querySelectorAll('tr[data-row]')].map((r) => r.getAttribute('data-row'))) : new Set(); };
  function markNew(id, before) {
    const el = document.getElementById(id);
    if (!el || before.size === 0) return;
    el.querySelectorAll('tr[data-row]').forEach((r) => {
      if (before.has(r.getAttribute('data-row'))) return;
      r.classList.add('new');
      setTimeout(() => r.classList.remove('new'), 1600);
    });
  }
  function pause(btn) {
    const live = document.querySelector('[data-seen]');
    pausedAt = live ? Number(live.getAttribute('data-seen')) : 0;
    document.querySelectorAll('[data-paused-banner]').forEach((el) => { el.hidden = false; });
    showHeld(0);
    btn.dataset.label = btn.textContent;
    btn.textContent = 'Resume';
    btn.removeAttribute('data-pause');
    btn.setAttribute('data-resume', '');
  }
  function resume(btn) {
    pausedAt = null;
    document.querySelectorAll('[data-paused-banner]').forEach((el) => { el.hidden = true; });
    btn.textContent = btn.dataset.label || 'Pause';
    btn.removeAttribute('data-resume');
    btn.setAttribute('data-pause', '');
    release();
  }
  document.addEventListener('click', (e) => {
    const p = e.target.closest('[data-pause]');
    if (p && !e.defaultPrevented) { e.preventDefault(); pause(p); return; }
    const r = e.target.closest('[data-resume]');
    if (r && pausedAt !== null) { e.preventDefault(); resume(r); }
  });
  // A drawer that closes lets the tape catch up. Keys: j and k move between the rows' links; Enter opens the one in focus.
  document.addEventListener('click', () => setTimeout(() => { if (pausedAt === null && !overlayOpen()) release(); }, 0));
  document.addEventListener('keydown', (e) => {
    if (e.metaKey || e.ctrlKey || e.altKey || (e.key !== 'j' && e.key !== 'k')) return;
    const tag = e.target && e.target.tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return;
    const links = [...document.querySelectorAll('table.wire td.tm a')];
    if (!links.length) return;
    const at = links.indexOf(document.activeElement);
    const next = e.key === 'j' ? Math.min(links.length - 1, at + 1) : Math.max(0, at < 0 ? 0 : at - 1);
    links[next].focus();
    e.preventDefault();
  });
  // A filter that is a select applies as soon as it changes (without a script it is applied with the Enter key or the button).
  document.addEventListener('change', (e) => { const s = e.target.closest && e.target.closest('select[data-autosubmit]'); if (s && s.form) s.form.submit(); });

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

  // A dialog: a link marked data-dialog fetches the dialog's body (the server answers it alone when asked for a fragment)
  // and opens it in the platform's <dialog> over this page. Cancel closes it; a failed fetch follows the link instead.
  // The dialog is a plain POST form, so submitting it navigates, exactly as it does with no script.
  document.addEventListener('click', async (e) => {
    const close = e.target.closest('[data-dialog-close]');
    if (close && close.closest('dialog.dlg')) { e.preventDefault(); close.closest('dialog.dlg').close(); return; }
    const a = e.target.closest('a[data-dialog]');
    if (!a || e.defaultPrevented || e.metaKey || e.ctrlKey || e.shiftKey || e.button) return;
    e.preventDefault();
    try {
      const r = await fetch(a.href, { headers: { 'X-Fragment': '1' } });
      if (!r.ok || !HTMLDialogElement.prototype.showModal) throw new Error(String(r.status));
      const d = document.createElement('dialog');
      d.className = 'dlg';
      d.innerHTML = await r.text();
      d.addEventListener('close', () => d.remove());
      d.addEventListener('click', (ev) => { if (ev.target === d) d.close(); });
      document.body.append(d);
      d.showModal();
    } catch (err) { location.href = a.href; }
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
