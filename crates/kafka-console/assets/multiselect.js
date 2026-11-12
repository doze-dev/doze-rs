// MultiSelect, an Alpine component (the CSP build: named methods only; no string is evaluated). Markup: src/ui/components/multiselect.rs.
// It is loaded after components.js and registers on `alpine:init`. Contract: docs/port/console/components.md.
//
// The state lives in real checkboxes (inside a <details> that the script hides): a form submits one `name=value` per chosen option,
// with or without this file. The popup's options are the server's elements and are never built from strings; the chips of the
// chosen ones are made with createElement and textContent. A change submits the form (data-autosubmit) a moment after the last one,
// and at once when the popup closes, so a person ticking three topics asks the server once.
document.addEventListener('alpine:init', () => {
  const A = window.Alpine;
  const SETTLE = 450;
  const words = (placeholder, noun, labels) => {
    if (!labels.length) return placeholder;
    if (labels.length <= 2) return labels.join(', ');
    return labels.length + ' ' + noun;
  };

  A.data('dzMulti', () => {
    let root, trigger, pop, search, list, chips, status, clearBtn, empty, details;
    let checks = [], opts = [];
    let at = -1, timer = 0, dirty = false;
    const label = (o) => (o.getAttribute('data-label') || o.textContent).trim();
    const visible = () => opts.filter((o) => !o.hidden);
    const idx = (o) => Number(o.getAttribute('data-i'));
    const on = (o) => !!checks[idx(o)] && checks[idx(o)].checked;
    const chosen = () => opts.filter(on);
    return {
      open: false,
      has: false,
      init() {
        root = this.$el; trigger = this.$refs.trigger; pop = this.$refs.pop; search = this.$refs.search; list = this.$refs.list;
        chips = this.$refs.chips; status = this.$refs.status; clearBtn = this.$refs.clear;
        details = root.querySelector('.dz-native');
        checks = [...details.querySelectorAll('input[type=checkbox]')];
        opts = [...list.querySelectorAll('[role=option]')];
        empty = list.querySelector('.dz-empty');
        checks.forEach((c) => { c.tabIndex = -1; });
        details.setAttribute('aria-hidden', 'true');
        details.tabIndex = -1;
        root.classList.add('is-on');
        trigger.hidden = false;
        this.paint();
      },
      // What the person chose, in every place it is shown: the options, the button, the chips, the count.
      paint() {
        const noun = root.getAttribute('data-noun') || 'items';
        const ph = root.getAttribute('data-placeholder') || '';
        const ls = chosen().map(label);
        opts.forEach((o) => o.setAttribute('aria-selected', String(on(o))));
        const v = trigger.querySelector('.dz-value');
        if (v) { v.textContent = words(ph, noun, ls); v.classList.toggle('muted', !ls.length); }
        trigger.title = ls.join(', ');
        trigger.setAttribute('aria-label', (root.getAttribute('data-label') || pop.getAttribute('aria-label') || '') + ': ' + words(ph, noun, ls));
        trigger.classList.toggle('has', ls.length > 0);
        root.classList.toggle('on', ls.length > 0);
        this.has = ls.length > 0;
        status.textContent = ls.length + ' of ' + opts.length + ' selected';
        this.paintChips();
      },
      paintChips() {
        chips.textContent = '';
        for (const o of chosen()) {
          const c = document.createElement('span');
          c.className = 'dz-tag';
          c.setAttribute('data-i', String(idx(o)));
          const t = document.createElement('span');
          t.className = 'dz-tag-t';
          t.textContent = label(o);
          const x = document.createElement('button');
          x.type = 'button'; x.tabIndex = -1; x.className = 'dz-tag-x';
          x.setAttribute('aria-label', 'Remove ' + label(o));
          x.setAttribute('data-remove', String(idx(o)));
          x.textContent = '×';
          c.append(t, x);
          chips.append(c);
        }
        const all = pop.querySelector('[data-all]'), none = pop.querySelector('[data-none]');
        if (all) all.disabled = visible().every(on);
        if (none) none.disabled = !chosen().length;
      },
      // The selected options first (as the popup opens: never while it is open, so nothing moves under the pointer).
      order() {
        const sorted = [...opts].sort((a, b) => (on(a) ? 0 : 1) - (on(b) ? 0 : 1) || idx(a) - idx(b));
        sorted.forEach((o) => list.insertBefore(o, empty));
      },
      show(seed) {
        if (this.open || trigger.disabled) return;
        this.open = true;
        this.order();
        search.value = seed || '';
        this.filter();
        pop.style.minWidth = Math.max(trigger.offsetWidth, 240) + 'px';
        this.$nextTick(() => {
          search.focus();
          const first = visible().findIndex(on);
          this.move(first < 0 ? 0 : first);
        });
      },
      hide(refocus) {
        if (!this.open) return;
        this.open = false;
        opts.forEach((o) => o.removeAttribute('data-active'));
        search.removeAttribute('aria-activedescendant');
        at = -1;
        this.flush();
        if (refocus) trigger.focus();
      },
      toggleOpen() { if (this.open) this.hide(true); else this.show(''); },
      filter() {
        const q = search.value.trim().toLowerCase();
        let shown = 0;
        for (const o of opts) { const hit = !q || label(o).toLowerCase().includes(q); o.hidden = !hit; if (hit) shown++; }
        if (empty) empty.hidden = shown > 0;
        opts.forEach((o) => o.removeAttribute('data-active'));
        at = -1;
        this.move(0);
        this.paintChips();
      },
      move(i) {
        const all = visible();
        opts.forEach((o) => o.removeAttribute('data-active'));
        if (!all.length) { at = -1; search.removeAttribute('aria-activedescendant'); return; }
        at = Math.max(0, Math.min(all.length - 1, i));
        const o = all[at];
        o.setAttribute('data-active', '');
        search.setAttribute('aria-activedescendant', o.id);
        if (o.scrollIntoView) o.scrollIntoView({ block: 'nearest' });
      },
      // One option on or off (or the others, when `to` says).
      set(o, to) {
        const c = checks[idx(o)];
        if (!c || c.checked === to) return;
        c.checked = to;
        this.changed();
      },
      toggle(o) { if (o) this.set(o, !on(o)); },
      changed() {
        this.paint();
        dirty = true;
        clearTimeout(timer);
        timer = setTimeout(() => this.flush(), SETTLE);
      },
      // Ask the server (the form's change handler submits it) once, when the ticking has settled or the popup closes.
      flush() {
        clearTimeout(timer);
        if (!dirty) return;
        dirty = false;
        const c = checks[0];
        if (c) c.dispatchEvent(new Event('change', { bubbles: true }));
      },
      all() { visible().forEach((o) => { const c = checks[idx(o)]; if (c) c.checked = true; }); this.changed(); },
      none() { checks.forEach((c) => { c.checked = false; }); this.changed(); },
      removeLast() { const c = chosen(); if (c.length) this.set(c[c.length - 1], false); },
      click(e) {
        const t = e.target;
        if (!t.closest) return;
        const rm = t.closest('[data-remove]');
        if (rm && pop.contains(rm)) { const o = opts.find((x) => idx(x) === Number(rm.getAttribute('data-remove'))); this.set(o, false); search.focus(); return; }
        if (t.closest('[data-clear]')) { this.none(); this.flush(); trigger.focus(); return; }
        if (t.closest('[data-all]')) { this.all(); search.focus(); return; }
        if (t.closest('[data-none]')) { this.none(); search.focus(); return; }
        const o = t.closest('[role=option]');
        if (o && list.contains(o)) { this.toggle(o); this.move(visible().indexOf(o)); search.focus(); return; }
        if (trigger.contains(t)) this.toggleOpen();
      },
      // A press inside the popup keeps focus where it is (in the search field), so that the popup does not see focus leave.
      down(e) {
        const t = e.target;
        if (this.open && t.closest && pop.contains(t) && t !== search) e.preventDefault();
      },
      hover(e) {
        const o = e.target.closest && e.target.closest('[role=option]');
        if (o && list.contains(o) && o.getAttribute('data-active') === null) this.move(visible().indexOf(o));
      },
      away() { this.hide(false); },
      focusout(e) { if (this.open && !root.contains(e.relatedTarget)) this.hide(false); },
      key(e) {
        const k = e.key;
        if (!this.open) {
          if (e.target === trigger) {
            if (k === 'Enter' || k === ' ' || k === 'ArrowDown' || k === 'ArrowUp') { e.preventDefault(); this.show(''); }
            else if (k.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) { e.preventDefault(); this.show(k); }
          }
          return;
        }
        if (k === 'Escape') { e.preventDefault(); e.stopPropagation(); this.hide(true); return; }
        const inSearch = e.target === search;
        if (k === 'ArrowDown') { e.preventDefault(); this.move(at + 1); }
        else if (k === 'ArrowUp') { e.preventDefault(); this.move(at - 1); }
        else if (k === 'Home') { e.preventDefault(); this.move(0); }
        else if (k === 'End') { e.preventDefault(); this.move(visible().length - 1); }
        else if (inSearch && k === 'Enter') { e.preventDefault(); this.toggle(visible()[at]); }
        else if (inSearch && k === ' ' && search.value === '') { e.preventDefault(); this.toggle(visible()[at]); }
        else if (inSearch && k === 'Backspace' && search.value === '') { e.preventDefault(); this.removeLast(); }
      },
    };
  });
});
