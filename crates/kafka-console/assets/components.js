// The console's interactive components, as Alpine components (the CSP build: every behaviour is a named method, no expression is
// evaluated from a string, so the page needs neither `unsafe-eval` nor an inline script). The markup comes from the server
// (src/ui/components/*.rs) and is complete without this file: a Select is a real <select>, a menu a list of links, a tooltip a
// `title`. When Alpine starts, a component adds `is-on` to its root, which hides the plain control and shows the enhanced one.
// Contract per component: docs/port/console/components.md. Tests: tools/dom/components.test.ts, tools/ux/scenarios/.
//
//   dzSelect    single choice (listbox, select-only combobox pattern)       dzCombobox  searchable choice, optional free text
//   dzMenu      "..." actions menu (menu button pattern)                    dzTip       tooltip, 400 ms, hover and focus
//   dzPopover   a small floating card on click                              dzSeg       segmented control, roving tabindex
document.addEventListener('alpine:init', () => {
  const A = window.Alpine;
  let serial = 0;
  const uid = (p) => p + '-' + (++serial);
  const keyOf = (e) => e.key;

  // One list of options, kept by a closure: the options are the server's elements; the component moves `data-active` and
  // `aria-selected` over them and never builds markup of its own (so hostile labels stay text).
  function listOf(root) {
    const list = root.querySelector('[role=listbox],[role=menu]');
    const items = () => [...list.querySelectorAll('[role=option],[role=menuitem]')].filter((o) => !o.hidden);
    let at = -1, typed = '', typedAt = 0;
    return {
      list,
      items,
      get at() { return at; },
      activate(i, scroll = true) {
        const all = items();
        all.forEach((o) => o.removeAttribute('data-active'));
        at = all.length ? Math.max(0, Math.min(all.length - 1, i)) : -1;
        const o = all[at];
        if (!o) return null;
        o.setAttribute('data-active', '');
        if (scroll && o.scrollIntoView) o.scrollIntoView({ block: 'nearest' });
        return o;
      },
      clear() { items().forEach((o) => o.removeAttribute('data-active')); at = -1; },
      // Type-ahead: letters typed within a second make a prefix; the first item whose label starts with it is active.
      type(ch) {
        const now = Date.now();
        typed = now - typedAt > 800 ? ch : typed + ch;
        typedAt = now;
        const all = items(), lower = typed.toLowerCase();
        const from = typed.length === 1 ? at + 1 : at;
        const order = [...all.slice(Math.max(0, from)), ...all.slice(0, Math.max(0, from))];
        const hit = order.find((o) => (o.getAttribute('data-label') || o.textContent).trim().toLowerCase().startsWith(lower));
        return hit ? this.activate(all.indexOf(hit)) : null;
      },
    };
  }
  const printable = (e) => keyOf(e).length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey && keyOf(e) !== ' ';

  // ---- Select ----------------------------------------------------------------------------------------------------------
  // Markup: <div x-data="dzSelect"> <select class="dz-native"> + <button x-ref="trigger" role="combobox"> + <div x-ref="list" role="listbox">.
  // The value lives in the <select> (it is what a form submits); choosing sets it and fires `change`.
  A.data('dzSelect', () => {
    let root, native, trigger, L;
    const label = (o) => (o.getAttribute('data-label') || o.textContent).trim();
    return {
      open: false,
      init() {
        root = this.$el; native = root.querySelector('select.dz-native'); trigger = this.$refs.trigger;
        L = listOf(root);
        const id = native.id || uid('sel');
        trigger.setAttribute('aria-controls', L.list.id || (L.list.id = id + '-list'));
        const lab = native.id && document.querySelector('label[for="' + native.id + '"]');
        if (lab) {
          if (!lab.id) lab.id = id + '-label';
          trigger.setAttribute('aria-labelledby', lab.id);
          lab.addEventListener('click', (e) => { e.preventDefault(); trigger.focus(); });
        } else if (native.getAttribute('aria-label')) trigger.setAttribute('aria-label', native.getAttribute('aria-label'));
        if (native.disabled) trigger.disabled = true;
        native.tabIndex = -1;
        native.setAttribute('aria-hidden', 'true');
        root.classList.add('is-on');
        trigger.hidden = false;
        this.paint();
      },
      // The trigger shows the chosen option; the options that are chosen say so.
      paint() {
        const v = native.value;
        let shown = null;
        L.items().forEach((o) => { const on = o.getAttribute('data-value') === v; o.setAttribute('aria-selected', String(on)); if (on) shown = o; });
        const t = trigger.querySelector('.dz-value');
        if (t) { t.textContent = shown ? label(shown) : (trigger.getAttribute('data-placeholder') || ''); t.classList.toggle('muted', !shown); }
      },
      show() {
        if (this.open || trigger.disabled) return;
        this.open = true;
        L.list.style.minWidth = trigger.offsetWidth + 'px';
        const sel = L.items().findIndex((o) => o.getAttribute('aria-selected') === 'true');
        this.$nextTick(() => { const o = L.activate(sel < 0 ? 0 : sel); trigger.setAttribute('aria-activedescendant', o ? o.id : ''); });
      },
      hide(refocus) {
        if (!this.open) return;
        this.open = false;
        L.clear();
        trigger.removeAttribute('aria-activedescendant');
        if (refocus) trigger.focus();
      },
      move(i) { const o = L.activate(i); if (o) trigger.setAttribute('aria-activedescendant', o.id); },
      choose(o) {
        if (!o || o.getAttribute('aria-disabled') === 'true') return;
        const changed = native.value !== o.getAttribute('data-value');
        native.value = o.getAttribute('data-value');
        this.paint();
        this.hide(true);
        if (changed) native.dispatchEvent(new Event('change', { bubbles: true }));
      },
      toggle() { if (this.open) this.hide(true); else this.show(); },
      click(e) {
        const o = e.target.closest('[role=option]');
        if (o && L.list.contains(o)) { this.choose(o); return; }
        if (trigger.contains(e.target)) this.toggle();
      },
      hover(e) {
        const o = e.target.closest && e.target.closest('[role=option]');
        if (o && o.getAttribute('data-active') === null) this.move(L.items().indexOf(o));
      },
      away() { this.hide(false); },
      focusout(e) { if (this.open && !root.contains(e.relatedTarget)) this.hide(false); },
      key(e) {
        const k = keyOf(e);
        if (!this.open) {
          if (k === 'Enter' || k === ' ' || k === 'ArrowDown' || k === 'ArrowUp') { e.preventDefault(); this.show(); }
          else if (printable(e) && e.target === trigger) { this.show(); this.$nextTick(() => { const o = L.type(k); if (o) trigger.setAttribute('aria-activedescendant', o.id); }); }
          return;
        }
        if (k === 'ArrowDown') { e.preventDefault(); this.move(L.at + 1); }
        else if (k === 'ArrowUp') { e.preventDefault(); this.move(L.at - 1); }
        else if (k === 'Home') { e.preventDefault(); this.move(0); }
        else if (k === 'End') { e.preventDefault(); this.move(L.items().length - 1); }
        else if (k === 'Enter' || k === ' ') { e.preventDefault(); this.choose(L.items()[L.at]); }
        else if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); this.hide(true); }
        else if (k === 'Tab') { const o = L.items()[L.at]; if (o) this.choose(o); else this.hide(false); }
        else if (printable(e)) { const o = L.type(k); if (o) trigger.setAttribute('aria-activedescendant', o.id); }
      },
    };
  });

  // ---- Combobox --------------------------------------------------------------------------------------------------------
  // A Select you can type into. Markup: .dz-native (a <select> for a closed list, an <input list> for free text), the visible
  // <input x-ref="input" role="combobox"> (no name), the listbox, and an empty-state row. `data-create` offers "Use <text>".
  A.data('dzCombobox', () => {
    let root, native, input, L, empty, free, create, createRow;
    const text = (o) => (o.getAttribute('data-label') || o.textContent).trim();
    const selectedLabel = () => {
      const o = [...L.list.querySelectorAll('[role=option][data-value]')].find((x) => x.getAttribute('data-value') === native.value && x !== createRow);
      return o ? text(o) : (free ? native.value : '');
    };
    return {
      open: false,
      init() {
        root = this.$el; native = root.querySelector('.dz-native'); input = this.$refs.input; L = listOf(root);
        empty = root.querySelector('.dz-empty'); free = native.tagName !== 'SELECT'; create = root.hasAttribute('data-create');
        createRow = root.querySelector('[data-create-row]');
        const id = native.id || uid('cbx');
        input.setAttribute('aria-controls', L.list.id || (L.list.id = id + '-list'));
        const lab = native.id && document.querySelector('label[for="' + native.id + '"]');
        if (lab) { if (!lab.id) lab.id = id + '-label'; input.setAttribute('aria-labelledby', lab.id); lab.addEventListener('click', (e) => { e.preventDefault(); input.focus(); }); }
        else if (native.getAttribute('aria-label')) input.setAttribute('aria-label', native.getAttribute('aria-label'));
        if (free) { try { native.type = 'hidden'; } catch (e) { /* an old engine keeps it a text field: the value is still right */ } native.removeAttribute('list'); }
        native.tabIndex = -1;
        native.setAttribute('aria-hidden', 'true');
        root.classList.add('is-on');
        root.querySelector('.dz-field').hidden = false;
        input.value = selectedLabel();
        this.mark();
      },
      mark() { L.items().forEach((o) => { if (o !== createRow) o.setAttribute('aria-selected', String(o.getAttribute('data-value') === native.value)); }); },
      filter() {
        const q = input.value.trim().toLowerCase();
        let shown = 0;
        L.list.querySelectorAll('[role=option]').forEach((o) => {
          if (o === createRow) return;
          const hit = !q || text(o).toLowerCase().includes(q);
          o.hidden = !hit;
          if (hit) shown++;
        });
        if (createRow) {
          const exact = [...L.list.querySelectorAll('[role=option]')].some((o) => o !== createRow && text(o).toLowerCase() === q);
          createRow.hidden = !create || !q || exact;
          const c = createRow.querySelector('.dz-create-text');
          if (c) c.textContent = input.value.trim();
          createRow.setAttribute('data-value', input.value.trim());
        }
        if (empty) empty.hidden = shown > 0 || (createRow && !createRow.hidden);
        L.clear();
        const first = L.activate(0);
        input.setAttribute('aria-activedescendant', first ? first.id : '');
      },
      show() {
        if (this.open) return;
        this.open = true;
        L.list.style.minWidth = root.querySelector('.dz-field').offsetWidth + 'px';
        this.$nextTick(() => this.filter());
      },
      hide(revert) {
        if (!this.open) return;
        this.open = false;
        L.clear();
        input.removeAttribute('aria-activedescendant');
        if (revert) input.value = selectedLabel();
      },
      move(i) { const o = L.activate(i); if (o) input.setAttribute('aria-activedescendant', o.id); },
      choose(o) {
        if (!o) return;
        const v = o.getAttribute('data-value');
        const changed = native.value !== v;
        if (!free || o !== createRow) { if (native.tagName === 'SELECT' && ![...native.options].some((x) => x.value === v)) { native.add(new Option(text(o), v)); } }
        native.value = v;
        input.value = o === createRow ? v : text(o);
        this.mark();
        this.hide(false);
        if (changed) native.dispatchEvent(new Event('change', { bubbles: true }));
      },
      input() { this.open ? this.filter() : this.show(); },
      click(e) {
        const o = e.target.closest('[role=option]');
        if (o && L.list.contains(o)) { this.choose(o); input.focus(); return; }
        if (e.target.closest('.dz-chev')) { this.open ? this.hide(true) : this.show(); input.focus(); }
        else if (e.target === input) this.show();
      },
      hover(e) { const o = e.target.closest && e.target.closest('[role=option]'); if (o && o.getAttribute('data-active') === null) this.move(L.items().indexOf(o)); },
      away() { this.hide(true); },
      focusout(e) {
        if (root.contains(e.relatedTarget)) return;
        // Free text keeps what was typed; a closed list goes back to the chosen item.
        if (free && create && input.value.trim() !== native.value) { native.value = input.value.trim(); native.dispatchEvent(new Event('change', { bubbles: true })); }
        this.hide(!(free && create));
      },
      key(e) {
        const k = keyOf(e);
        if (k === 'ArrowDown' || k === 'ArrowUp') { e.preventDefault(); if (!this.open) this.show(); else this.move(L.at + (k === 'ArrowDown' ? 1 : -1)); }
        else if (!this.open) { return; }
        else if (k === 'Home' && e.ctrlKey) { e.preventDefault(); this.move(0); }
        else if (k === 'End' && e.ctrlKey) { e.preventDefault(); this.move(L.items().length - 1); }
        else if (k === 'Enter') { e.preventDefault(); const o = L.items()[L.at]; if (o) this.choose(o); else if (free && create && input.value.trim()) this.choose(Object.assign(document.createElement('i'), { textContent: input.value.trim() })); }
        else if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); this.hide(true); }
        else if (k === 'Tab') { this.hide(true); }
      },
    };
  });

  // ---- Menu ------------------------------------------------------------------------------------------------------------
  // A "..." button and a role=menu of links and buttons. Focus moves into the menu (roving), Esc returns it to the button.
  A.data('dzMenu', () => {
    let root, trigger, L;
    return {
      open: false,
      init() {
        root = this.$el; trigger = this.$refs.trigger; L = listOf(root);
        trigger.setAttribute('aria-controls', L.list.id || (L.list.id = uid('menu')));
        L.items().forEach((o) => o.setAttribute('tabindex', '-1'));
        root.classList.add('is-on');
        trigger.hidden = false;
      },
      show(last) {
        if (this.open) return;
        this.open = true;
        this.$nextTick(() => { const all = L.items(); const o = L.activate(last ? all.length - 1 : 0); if (o) o.focus(); });
      },
      hide(refocus) {
        if (!this.open) return;
        this.open = false;
        L.clear();
        if (refocus) trigger.focus();
      },
      toggle() { if (this.open) this.hide(true); else this.show(false); },
      move(i) { const o = L.activate(i); if (o) o.focus(); },
      click(e) {
        if (trigger.contains(e.target)) { this.toggle(); return; }
        const o = e.target.closest('[role=menuitem]');
        if (o && !o.hasAttribute('aria-disabled')) this.hide(false);
      },
      away() { this.hide(false); },
      focusout(e) { if (this.open && e.relatedTarget && !root.contains(e.relatedTarget)) this.hide(false); },
      key(e) {
        const k = keyOf(e);
        if (!this.open) {
          if (e.target === trigger && (k === 'ArrowDown' || k === 'ArrowUp')) { e.preventDefault(); this.show(k === 'ArrowUp'); }
          return;
        }
        const at = L.items().indexOf(document.activeElement);
        if (k === 'ArrowDown') { e.preventDefault(); this.move(at + 1 >= L.items().length ? 0 : at + 1); }
        else if (k === 'ArrowUp') { e.preventDefault(); this.move(at <= 0 ? L.items().length - 1 : at - 1); }
        else if (k === 'Home') { e.preventDefault(); this.move(0); }
        else if (k === 'End') { e.preventDefault(); this.move(L.items().length - 1); }
        else if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); this.hide(true); }
        else if (k === 'Tab') { this.hide(false); }
        else if (k === ' ' && document.activeElement && document.activeElement !== trigger) { e.preventDefault(); document.activeElement.click(); }
        else if (printable(e)) { const o = L.type(k); if (o) o.focus(); }
      },
    };
  });

  // ---- Tooltip ---------------------------------------------------------------------------------------------------------
  // Hover or focus for 400 ms shows it; leaving, blur, Esc or a click hides it. The plain `title` is the no-script tooltip and is
  // taken off the trigger once this one exists, so the browser does not show a second.
  A.data('dzTip', () => {
    let trigger, timer = 0;
    return {
      shown: false,
      init() {
        trigger = this.$refs.trigger || this.$el.firstElementChild;
        const t = trigger.getAttribute('title');
        if (t) trigger.removeAttribute('title');
        this.$el.classList.add('is-on');
      },
      later() { clearTimeout(timer); timer = setTimeout(() => { this.shown = true; }, 400); },
      now() { clearTimeout(timer); this.shown = true; },
      off() { clearTimeout(timer); this.shown = false; },
      key(e) { if (keyOf(e) === 'Escape' && this.shown) { this.shown = false; } },
    };
  });

  // ---- Popover ---------------------------------------------------------------------------------------------------------
  // A button that opens a small card; Esc or a click outside closes it and returns focus to the button.
  A.data('dzPopover', () => {
    let root, trigger;
    return {
      open: false,
      init() { root = this.$el; trigger = this.$refs.trigger; trigger.hidden = false; root.classList.add('is-on'); },
      toggle() { this.open = !this.open; },
      away() { this.open = false; },
      key(e) { if (keyOf(e) === 'Escape' && this.open) { e.preventDefault(); e.stopPropagation(); this.open = false; trigger.focus(); } },
      focusout(e) { if (this.open && e.relatedTarget && !root.contains(e.relatedTarget)) this.open = false; },
    };
  });

  // ---- Segmented control -----------------------------------------------------------------------------------------------
  // A row of links or buttons, one marked current. One stop in the Tab order (roving tabindex); the arrows, Home and End move
  // focus between them; Enter or Space activates (they are links or buttons, so the click does what it always did).
  A.data('dzSeg', () => {
    let items = [];
    const cur = () => items.findIndex((i) => i.getAttribute('tabindex') === '0');
    const rove = (to) => { items.forEach((i, n) => i.setAttribute('tabindex', n === to ? '0' : '-1')); if (items[to]) items[to].focus(); };
    return {
      init() {
        items = [...this.$el.querySelectorAll('[data-seg]')];
        const on = Math.max(0, items.findIndex((i) => i.getAttribute('aria-current') === 'true' || i.getAttribute('aria-pressed') === 'true' || i.classList.contains('on')));
        items.forEach((i, n) => i.setAttribute('tabindex', n === on ? '0' : '-1'));
      },
      key(e) {
        const k = keyOf(e), n = items.length;
        const at = items.indexOf(document.activeElement);
        if (at < 0) return;
        if (k === 'ArrowRight' || k === 'ArrowDown') { e.preventDefault(); rove((at + 1) % n); }
        else if (k === 'ArrowLeft' || k === 'ArrowUp') { e.preventDefault(); rove((at - 1 + n) % n); }
        else if (k === 'Home') { e.preventDefault(); rove(0); }
        else if (k === 'End') { e.preventDefault(); rove(n - 1); }
      },
      focus(e) { const at = items.indexOf(e.target); if (at >= 0 && at !== cur()) items.forEach((i, n) => i.setAttribute('tabindex', n === at ? '0' : '-1')); },
    };
  });
});

// ---- Keep every floating card inside the page ----------------------------------------------------------------------------
// Floating UI (inside x-anchor) flips and shifts a card to stay in the window, but it knows only the window: a menu opened from a
// trigger near the left of the content (anchored bottom-end, 200 px wide) shifts to 5 px from the window's edge, over the rail.
// Whatever the anchor wrote, a card that is shown is nudged sideways (by a margin, which the anchor never touches) so that it is
// right of the rail and inside the window. Vertically Floating UI's flip is right and a nudge would move a card under the pointer.
// It converges in one pass and runs only when a card's style changes.
(() => {
  const GAP = 8;
  function fit(pop) {
    if (!pop.isConnected) return;
    if (pop.style.display === 'none') { if (pop.style.marginLeft) pop.style.marginLeft = ''; return; }
    // A list is at least as wide as what opened it, and as wide as its longest option up to 420 px (the CSS): never a column the width of
    // a narrow trigger with words broken in the middle.
    const root = pop.closest('.dz'), ref = root && root.querySelector('.dz-trigger,.dz-field');
    if (ref && pop.matches('[role=listbox]')) { const w = Math.round(ref.getBoundingClientRect().width) + 'px'; if (pop.style.minWidth !== w) pop.style.minWidth = w; }
    const ml = parseFloat(pop.style.marginLeft) || 0;
    const r = pop.getBoundingClientRect();
    if (!r.width) return;
    // Where the anchor put it: the box without the nudge of an earlier pass (a margin moves an absolute box by exactly itself).
    const left = r.left - ml, right = r.right - ml;
    const rail = document.getElementById('rail');
    const minX = Math.max(GAP, rail ? rail.getBoundingClientRect().right + GAP : 0);
    const maxX = document.documentElement.clientWidth - GAP;
    let dx = 0;
    if (left < minX) dx = minX - left; else if (right > maxX) dx = Math.max(maxX - right, minX - left);
    if (Math.abs(dx - ml) >= 0.5) pop.style.marginLeft = dx + 'px';
  }
  const seen = new WeakSet();
  const mo = new MutationObserver((list) => {
    for (const m of list) {
      const el = m.target;
      if (el.nodeType === 1 && el.classList.contains('dz-pop')) requestAnimationFrame(() => fit(el));
    }
  });
  const watch = () => { document.querySelectorAll('.dz-pop:not(.dz-tip-shared)').forEach((p) => { if (!seen.has(p)) { seen.add(p); mo.observe(p, { attributes: true, attributeFilter: ['style'] }); } }); };
  document.addEventListener('alpine:initialized', watch);
  document.addEventListener('htmx:afterSettle', watch);
  document.addEventListener('focusin', watch);
  document.addEventListener('click', watch, true);
})();

// ---- Tooltips for what the browser would show a plain `title` for ----------------------------------------------------------
// A truncated middle-ellipsis value, an exact time behind a relative one, an icon button: each has its text in `title` or
// `aria-label`, which the browser shows late and in its own style. One shared card shows it the way the Tooltip component does
// (400 ms after the pointer rests, at once on keyboard focus, gone on leave, blur or Esc), with the `title` set aside while shown so
// the browser's own does not also appear. The text is set with `textContent`. Without this script the `title` is still there.
(() => {
  let tip = null, on = null, timer = 0, held = null;
  const textOf = (el) => {
    const t = el.getAttribute('title');
    if (t) return t;
    // A rail item whose name is hidden (the narrow window) says it.
    if (el.matches('.ni')) return getComputedStyle(el).fontSize === '0px' ? [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.data).join('').trim() : '';
    if (el.matches('button,a') && !el.textContent.trim()) return el.getAttribute('aria-label') || '';
    return '';
  };
  const candidate = (n) => {
    const el = n && n.closest && n.closest('[title],.ni,button[aria-label],a[aria-label]');
    if (!el || el.closest('.dz-tip,.dz-pop,[role=listbox],[role=menu]') || (el.matches('button,a') && !el.getAttribute('title') && !el.matches('.ni') && el.textContent.trim())) return null;
    return textOf(el) ? el : null;
  };
  function show(el) {
    hide();
    const text = textOf(el);
    if (!text) return;
    if (!tip) {
      tip = document.createElement('div');
      tip.className = 'dz-pop dz-tip-pop dz-tip-shared';
      tip.id = 'dz-tip-shared';
      tip.setAttribute('role', 'tooltip');
      tip.style.position = 'fixed';
      tip.style.pointerEvents = 'none';
      document.body.appendChild(tip);
    }
    on = el;
    if (el.hasAttribute('title')) { held = el.getAttribute('title'); el.removeAttribute('title'); }
    el.setAttribute('aria-describedby', 'dz-tip-shared');
    tip.textContent = text;
    tip.style.display = 'block';
    const r = el.getBoundingClientRect(), w = tip.getBoundingClientRect();
    const rail = document.getElementById('rail');
    const minX = rail ? rail.getBoundingClientRect().right + 8 : 8;
    let x = Math.min(Math.max(minX, r.left + r.width / 2 - w.width / 2), document.documentElement.clientWidth - w.width - 8);
    let y = r.bottom + 6;
    if (y + w.height > window.innerHeight - 8) y = r.top - w.height - 6;
    tip.style.left = x + 'px';
    tip.style.top = y + 'px';
  }
  function hide() {
    clearTimeout(timer);
    if (tip) tip.style.display = 'none';
    if (on) {
      if (held !== null) on.setAttribute('title', held);
      on.removeAttribute('aria-describedby');
    }
    on = null; held = null;
  }
  document.addEventListener('mouseover', (e) => {
    const el = candidate(e.target);
    if (el === on) return;
    hide();
    if (el) timer = setTimeout(() => show(el), 400);
  });
  document.addEventListener('mouseout', (e) => { if (!on || !e.relatedTarget || !on.contains(e.relatedTarget)) hide(); });
  document.addEventListener('focusin', (e) => { const el = candidate(e.target); if (el) show(el); });
  document.addEventListener('focusout', hide);
  document.addEventListener('keydown', (e) => { if (e.key === 'Escape' && on) hide(); }, true);
  document.addEventListener('mousedown', hide, true);
  document.addEventListener('htmx:beforeSwap', hide);
})();
