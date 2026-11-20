'use strict';

// SECURITY: window titles and app names come from other programs (any web page
// can set its own title). Never use innerHTML / insertAdjacentHTML here; build
// DOM nodes and set text with textContent only.

const invoke = window.__TAURI__.core.invoke;

const CATEGORY_COLORS = {
  'Coding': '#4f7cff',
  'Design': '#a368f5',
  'Writing': '#25a99b',
  'Research': '#3fb68b',
  'Meetings': '#f0a03c',
  'Communication': '#e2b93b',
  'Browsing': '#8d95a7',
  'Utilities': '#6f7890',
  'Other': '#b0b6c3',
  'Entertainment': '#e5576a',
  'Social Media': '#ef7fab',
};
const catColor = (c) => CATEGORY_COLORS[c] || '#999';

// ---------- helpers ----------

const $ = (id) => document.getElementById(id);

function el(tag, opts = {}, ...children) {
  const e = document.createElement(tag);
  if (opts.class) e.className = opts.class;
  if (opts.text != null) e.textContent = String(opts.text);
  if (opts.title != null) e.title = String(opts.title);
  if (opts.attrs) for (const [k, v] of Object.entries(opts.attrs)) e.setAttribute(k, String(v));
  if (opts.style) for (const [k, v] of Object.entries(opts.style)) e.style.setProperty(k, String(v));
  if (opts.on) for (const [k, v] of Object.entries(opts.on)) e.addEventListener(k, v);
  for (const c of children) if (c != null) e.append(c);
  return e;
}

function clear(node) { node.replaceChildren(); }

function fmtDur(secs) {
  secs = Math.max(0, Math.round(secs));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  if (h > 0) return `${h}h ${String(m).padStart(2, '0')}m`;
  if (m > 0 || secs === 0) return `${m}m`;
  return `${secs}s`;
}

function fmtClock(secs) {
  secs = Math.max(0, Math.floor(secs));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(s).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

const timeFmt = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });
const hourFmt = new Intl.DateTimeFormat(undefined, { hour: 'numeric' });
const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: 'long', month: 'long', day: 'numeric' });
const shortDayFmt = new Intl.DateTimeFormat(undefined, { weekday: 'short' });
const dateFmt = new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric' });

const fmtTime = (ts) => timeFmt.format(new Date(ts * 1000));
const pct = (a, b) => (b > 0 ? Math.round((a / b) * 100) : 0);

function startOfDay(d) { return new Date(d.getFullYear(), d.getMonth(), d.getDate()); }
function addDays(d, n) { return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n); }
function isSameDay(a, b) { return startOfDay(a).getTime() === startOfDay(b).getTime(); }
const toUnix = (d) => Math.floor(d.getTime() / 1000);
// Monday-based week start.
function startOfWeek(d) { const s = startOfDay(d); return addDays(s, -((s.getDay() + 6) % 7)); }

let toastTimer;
function toast(msg, isError = false) {
  const t = $('toast');
  t.textContent = msg;
  t.classList.toggle('error', isError);
  t.classList.remove('hidden');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.classList.add('hidden'), isError ? 6000 : 3500);
}

async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(typeof e === 'string' ? e : 'Something went wrong', true);
    throw e;
  }
}

/** Two-click confirmation for destructive buttons (window.confirm is unavailable in Tauri). */
function confirmButton(btn, action) {
  const original = btn.textContent;
  let armed = false;
  let timer;
  btn.addEventListener('click', async () => {
    if (!armed) {
      armed = true;
      btn.classList.add('confirm');
      btn.textContent = 'Click again to confirm';
      timer = setTimeout(() => { armed = false; btn.classList.remove('confirm'); btn.textContent = original; }, 4000);
      return;
    }
    clearTimeout(timer);
    armed = false;
    btn.classList.remove('confirm');
    btn.textContent = original;
    await action();
  });
}

// ---------- state ----------

const state = {
  view: 'today',
  day: startOfDay(new Date()),
  weekStart: startOfWeek(new Date()),
  categories: [],
  status: null,
  focusTarget: 50,
};

// ---------- rendering pieces ----------

function renderLegend() {
  const lg = $('prod-legend');
  clear(lg);
  for (const [cls, label] of [['productive', 'Productive'], ['neutral', 'Neutral'], ['distracting', 'Distracting']]) {
    lg.append(el('span', {}, el('i', { class: `sw sw-${cls}` }), label));
  }
}

function categorySelect(app, current) {
  const sel = el('select', { class: 'cat-select', attrs: { 'aria-label': `Category for ${app}` } });
  for (const c of state.categories) {
    const o = el('option', { text: c.name });
    o.value = c.name;
    if (c.name === current) o.selected = true;
    sel.append(o);
  }
  sel.addEventListener('change', async () => {
    await call('set_app_category', { app, category: sel.value });
    toast(`${app} is now “${sel.value}”`);
    refresh();
  });
  return sel;
}

function renderApps(container, apps, total, editable = true) {
  clear(container);
  if (!apps.length) {
    container.append(el('div', { class: 'empty', text: 'No activity recorded yet.' }));
    return;
  }
  const max = apps[0].secs;
  for (const a of apps.slice(0, 15)) {
    const row = el('div', { class: 'app-row' },
      el('div', { class: 'app-name', text: a.app, title: a.app }),
      editable ? categorySelect(a.app, a.category) : el('span', { class: 'muted small', text: a.category }),
      el('div', { class: 'app-time', text: fmtDur(a.secs), title: `${pct(a.secs, total)}% of tracked time` }),
      el('div', { class: 'app-bar' }, el('i', { style: { width: `${(a.secs / max) * 100}%`, background: catColor(a.category) } })),
    );
    if (a.topTitles && a.topTitles.length) {
      const ul = el('ul');
      for (const t of a.topTitles) ul.append(el('li', {}, el('span', { text: t.title, title: t.title }), el('span', { text: fmtDur(t.secs) })));
      row.append(el('details', { class: 'titles' }, el('summary', { text: 'Top windows' }), ul));
    }
    container.append(row);
  }
  if (apps.length > 15) {
    const rest = apps.slice(15).reduce((s, a) => s + a.secs, 0);
    container.append(el('div', { class: 'muted small', text: `+ ${apps.length - 15} more apps · ${fmtDur(rest)}` }));
  }
}

function renderCategories(barEl, listEl, cats, total) {
  clear(barEl);
  clear(listEl);
  if (!cats.length) { listEl.append(el('div', { class: 'empty', text: 'Nothing yet.' })); return; }
  for (const c of cats) {
    barEl.append(el('i', { title: `${c.category}: ${fmtDur(c.secs)}`, style: { width: `${(c.secs / total) * 100}%`, background: catColor(c.category) } }));
    listEl.append(el('div', { class: 'cat-item' },
      el('i', { class: 'sw', style: { background: catColor(c.category) } }),
      el('span', { text: c.category }),
      el('span', { class: 'dur', text: fmtDur(c.secs) }),
      el('span', { class: 'pct', text: `${pct(c.secs, total)}%` }),
    ));
  }
}

function sessionRow(kind, title, start, end, extra) {
  return el('div', { class: 'session' },
    el('span', { class: `badge ${kind === 'Timer' ? 'manual' : ''}`, text: kind }),
    el('div', { class: 'meta' },
      el('div', { text: title }),
      el('div', { text: `${fmtTime(start)} – ${end ? fmtTime(end) : 'now'}${extra ? ` · ${extra}` : ''}` }),
    ),
    el('span', { class: 'len', text: fmtDur((end || Date.now() / 1000) - start) }),
  );
}

function renderSessions(container, summary) {
  clear(container);
  const items = [];
  for (const b of summary.focusBlocks) {
    items.push({ start: b.start, node: sessionRow('Auto', b.topApp || 'Focused work', b.start, b.end, `${fmtDur(b.productiveSecs)} productive`) });
  }
  for (const m of summary.manualSessions) {
    const extra = `target ${m.targetMinutes}m`;
    items.push({ start: m.start, node: sessionRow('Timer', m.label || 'Focus session', m.start, m.end, extra) });
  }
  items.sort((a, b) => a.start - b.start);
  if (!items.length) {
    container.append(el('div', { class: 'empty', text: 'No focus sessions yet. Work in productive apps for a while, or start the focus timer.' }));
    return;
  }
  for (const i of items) container.append(i.node);
}

function renderTimeline(summary, dayStart) {
  const tl = $('timeline');
  const strip = $('focus-strip');
  const axis = $('timeline-axis');
  const hourly = $('hourly');
  clear(tl); clear(strip); clear(axis); clear(hourly);

  const dayStartTs = toUnix(dayStart);
  const dayEndTs = toUnix(addDays(dayStart, 1));
  // Window: whole hours around the activity, at least 8 hours wide.
  let fromH = 8, toH = 18;
  if (summary.firstActivity != null) {
    fromH = Math.floor((summary.firstActivity - dayStartTs) / 3600);
    toH = Math.ceil((summary.lastActivity - dayStartTs) / 3600);
    while (toH - fromH < 8) { if (fromH > 0) fromH--; if (toH - fromH < 8 && toH < 24) toH++; if (fromH === 0 && toH === 24) break; }
  }
  fromH = Math.max(0, fromH); toH = Math.min(24, Math.max(toH, fromH + 1));
  const winStart = dayStartTs + fromH * 3600;
  const winLen = Math.min(dayEndTs, dayStartTs + toH * 3600) - winStart;
  const x = (ts) => ((ts - winStart) / winLen) * 100;

  for (const s of summary.timeline) {
    tl.append(el('div', {
      class: 'seg',
      title: `${s.app} · ${s.category}\n${fmtTime(s.start)} – ${fmtTime(s.end)} (${fmtDur(s.end - s.start)})`,
      style: { left: `${x(s.start)}%`, width: `${x(s.end) - x(s.start)}%`, background: catColor(s.category) },
    }));
  }
  for (const b of summary.focusBlocks) {
    strip.append(el('i', {
      title: `Focus session · ${fmtTime(b.start)} – ${fmtTime(b.end)} (${fmtDur(b.end - b.start)})`,
      style: { left: `${x(b.start)}%`, width: `${x(b.end) - x(b.start)}%` },
    }));
  }
  const step = toH - fromH > 12 ? 2 : 1;
  for (let h = fromH; h <= toH; h += step) {
    const ts = dayStartTs + h * 3600;
    const label = hourFmt.format(new Date(ts * 1000));
    axis.append(el('span', { text: label, style: { left: `${x(ts)}%` } }));
  }

  // Hourly stacked bars (split spans across hour boundaries).
  const hours = [];
  for (let h = fromH; h < toH; h++) hours.push({ productive: 0, neutral: 0, distracting: 0 });
  for (const s of summary.timeline) {
    for (let h = fromH; h < toH; h++) {
      const a = dayStartTs + h * 3600, b = a + 3600;
      const o = Math.min(b, s.end) - Math.max(a, s.start);
      if (o > 0) hours[h - fromH][s.productivity] += o;
    }
  }
  hours.forEach((hr, i) => {
    const total = hr.productive + hr.neutral + hr.distracting;
    const label = hourFmt.format(new Date((dayStartTs + (fromH + i) * 3600) * 1000));
    const col = el('div', { class: 'hour', title: `${label}: ${fmtDur(total)} (${fmtDur(hr.productive)} productive)` });
    for (const k of ['productive', 'neutral', 'distracting']) {
      if (hr[k] > 0) col.append(el('i', { class: `p-${k}`, style: { height: `${(hr[k] / 3600) * 100}%` } }));
    }
    hourly.append(col);
  });
}

// ---------- views ----------

async function renderToday() {
  const from = toUnix(state.day);
  const to = toUnix(addDays(state.day, 1));
  const s = await call('get_summary', { from, to });
  const isToday = isSameDay(state.day, new Date());

  $('day-title').textContent = isToday ? 'Today' : dayFmt.format(state.day);
  $('day-sub').textContent = isToday ? dayFmt.format(state.day) : '';
  $('day-next').disabled = isToday;
  $('day-today').disabled = isToday;

  $('k-work').textContent = fmtDur(s.totalSecs);
  $('k-work-sub').textContent = s.firstActivity != null ? `${fmtTime(s.firstActivity)} – ${fmtTime(s.lastActivity)}` : 'No activity';
  $('k-focus').textContent = fmtDur(s.focusSecs);
  const n = s.focusBlocks.length;
  $('k-focus-sub').textContent = `${n} session${n === 1 ? '' : 's'} · ${pct(s.focusSecs, s.totalSecs)}% of work`;
  $('k-longest').textContent = s.longestFocusSecs ? fmtDur(s.longestFocusSecs) : '–';
  const longest = s.focusBlocks.reduce((m, b) => (!m || b.end - b.start > m.end - m.start ? b : m), null);
  $('k-longest-sub').textContent = longest ? `${fmtTime(longest.start)} – ${fmtTime(longest.end)}` : '';
  $('k-score').textContent = s.totalSecs ? `${pct(s.productiveSecs, s.totalSecs)}%` : '–';
  $('k-score-sub').textContent = s.totalSecs ? `${fmtDur(s.distractingSecs)} distracting · ${s.contextSwitches} app switches` : '';

  renderTimeline(s, state.day);
  renderApps($('apps'), s.apps, s.totalSecs);
  renderCategories($('cat-bar'), $('cat-list'), s.categories, s.totalSecs);
  renderSessions($('sessions'), s);
}

async function renderWeek() {
  const start = state.weekStart;
  const bounds = [];
  for (let i = 0; i <= 7; i++) bounds.push(toUnix(addDays(start, i)));
  const [days, s] = await Promise.all([
    call('get_interval_totals', { boundaries: bounds }),
    call('get_summary', { from: bounds[0], to: bounds[7] }),
  ]);
  const thisWeek = isSameDay(start, startOfWeek(new Date()));
  $('week-title').textContent = thisWeek ? 'This week' : `Week of ${dateFmt.format(start)}`;
  $('week-sub').textContent = `${dateFmt.format(start)} – ${dateFmt.format(addDays(start, 6))}`;
  $('week-next').disabled = thisWeek;
  $('week-now').disabled = thisWeek;

  const worked = days.filter((d) => d.totalSecs > 0);
  const total = days.reduce((a, d) => a + d.totalSecs, 0);
  const focus = days.reduce((a, d) => a + d.focusSecs, 0);
  const productive = days.reduce((a, d) => a + d.productiveSecs, 0);
  $('w-total').textContent = fmtDur(total);
  $('w-avg').textContent = worked.length ? fmtDur(total / worked.length) : '–';
  $('w-focus').textContent = fmtDur(focus);
  $('w-score').textContent = total ? `${pct(productive, total)}%` : '–';

  const chart = $('week-chart');
  clear(chart);
  const max = Math.max(3600, ...days.map((d) => d.totalSecs));
  const today = new Date();
  days.forEach((d, i) => {
    const date = addDays(start, i);
    const other = d.totalSecs - d.productiveSecs;
    const bar = el('div', { class: 'wbar', title: `${fmtDur(d.totalSecs)} total · ${fmtDur(d.productiveSecs)} productive`, style: { height: `${(d.totalSecs / max) * 100}%` } });
    if (d.totalSecs > 0) {
      bar.append(el('i', { style: { height: `${pct(d.productiveSecs, d.totalSecs)}%`, background: 'var(--productive)' } }));
      bar.append(el('i', { style: { height: `${pct(other, d.totalSecs)}%`, background: 'var(--other)' } }));
    }
    const fbar = el('div', { class: 'wbar focusbar', title: `${fmtDur(d.focusSecs)} focus`, style: { height: `${(d.focusSecs / max) * 100}%`, background: 'var(--focus)' } });
    const col = el('div', { class: `wday${isSameDay(date, today) ? ' today' : ''}` },
      el('div', { class: 'wbars' }, bar, fbar),
      el('div', { class: 'wlabel' }, el('b', { text: d.totalSecs ? fmtDur(d.totalSecs) : '–' }), shortDayFmt.format(date)),
    );
    col.addEventListener('click', () => { state.day = date; switchView('today'); });
    col.style.cursor = 'pointer';
    chart.append(col);
  });

  renderApps($('week-apps'), s.apps, s.totalSecs, false);
  renderCategories($('week-cat-bar'), $('week-cat-list'), s.categories, s.totalSecs);
}

const RING_LEN = 326.73;
function renderFocusTimer() {
  const f = state.status && state.status.activeFocus;
  $('focus-setup').classList.toggle('hidden', !!f);
  $('focus-running').classList.toggle('hidden', !f);
  const ring = $('ring-fg');
  if (!f) {
    $('timer').textContent = fmtClock(state.focusTarget * 60);
    $('timer-sub').textContent = 'Ready';
    ring.style.strokeDashoffset = String(RING_LEN);
    ring.classList.remove('done');
    return;
  }
  const elapsed = Date.now() / 1000 - f.start;
  const target = f.targetMinutes * 60;
  const remaining = target - elapsed;
  const done = remaining <= 0;
  $('timer').textContent = done ? `+${fmtClock(-remaining)}` : fmtClock(remaining);
  $('timer-sub').textContent = done ? 'Target reached 🎉' : `of ${f.targetMinutes} min`;
  ring.style.strokeDashoffset = String(RING_LEN * (1 - Math.min(1, elapsed / target)));
  ring.classList.toggle('done', done);
  $('focus-running-label').textContent = f.label || 'Focus session';
}

async function renderFocusHistory() {
  const from = toUnix(startOfDay(new Date()));
  const s = await call('get_summary', { from, to: toUnix(addDays(startOfDay(new Date()), 1)) });
  renderSessions($('focus-history'), s);
}

async function renderSettings() {
  const [s, autostart] = await Promise.all([call('get_settings'), call('get_autostart')]);
  setSelect($('s-idle'), s.idleThresholdSecs);
  $('s-titles').checked = s.captureTitles;
  $('s-excluded').value = s.excludedApps.join('\n');
  setSelect($('s-focus-min'), s.focusMinMinutes);
  setSelect($('s-focus-tol'), s.focusBreakToleranceMinutes);
  setSelect($('s-retention'), s.retentionDays);
  $('s-autostart').checked = autostart;
  $('settings-msg').textContent = '';
}

/** Select a value, adding an option if a stored value isn't in the preset list. */
function setSelect(sel, value) {
  const v = String(value);
  if (![...sel.options].some((o) => o.value === v)) {
    const o = el('option', { text: v });
    o.value = v;
    sel.append(o);
  }
  sel.value = v;
}

// ---------- status ----------

async function refreshStatus() {
  try {
    state.status = await invoke('get_status');
  } catch { return; }
  const st = state.status;
  const dot = $('status-dot');
  dot.className = 'dot';
  if (st.paused) {
    dot.classList.add('paused');
    $('status-text').textContent = 'Paused';
    $('status-sub').textContent = 'Nothing is being recorded';
  } else if (st.current) {
    dot.classList.add('on');
    $('status-text').textContent = st.current.app;
    $('status-sub').textContent = `for ${fmtDur(st.now - st.current.since)}`;
  } else {
    $('status-text').textContent = 'Away';
    $('status-sub').textContent = 'Waiting for activity';
  }
  $('pause-btn').textContent = st.paused ? 'Resume tracking' : 'Pause tracking';
  if (state.view === 'focus') renderFocusTimer();
}

// ---------- navigation ----------

async function refresh() {
  try {
    if (state.view === 'today') await renderToday();
    else if (state.view === 'week') await renderWeek();
    else if (state.view === 'focus') { renderFocusTimer(); await renderFocusHistory(); }
  } catch (e) {
    console.error(e);
  }
}

function switchView(view) {
  state.view = view;
  for (const b of document.querySelectorAll('.nav-item')) b.classList.toggle('active', b.dataset.view === view);
  for (const v of document.querySelectorAll('.view')) v.classList.toggle('hidden', v.id !== `view-${view}`);
  if (view === 'settings') renderSettings().catch(console.error);
  else refresh();
  document.querySelector('.content').scrollTop = 0;
}

// ---------- wiring ----------

function wire() {
  for (const b of document.querySelectorAll('.nav-item')) b.addEventListener('click', () => switchView(b.dataset.view));

  $('pause-btn').addEventListener('click', async () => {
    await call('set_paused', { paused: !(state.status && state.status.paused) });
    await refreshStatus();
    refresh();
  });

  $('day-prev').addEventListener('click', () => { state.day = addDays(state.day, -1); refresh(); });
  $('day-next').addEventListener('click', () => { state.day = addDays(state.day, 1); refresh(); });
  $('day-today').addEventListener('click', () => { state.day = startOfDay(new Date()); refresh(); });
  $('week-prev').addEventListener('click', () => { state.weekStart = addDays(state.weekStart, -7); refresh(); });
  $('week-next').addEventListener('click', () => { state.weekStart = addDays(state.weekStart, 7); refresh(); });
  $('week-now').addEventListener('click', () => { state.weekStart = startOfWeek(new Date()); refresh(); });

  // Focus timer
  const chips = [...document.querySelectorAll('#focus-chips .chip[data-min]')];
  for (const c of chips) {
    c.addEventListener('click', () => {
      state.focusTarget = Number(c.dataset.min);
      chips.forEach((x) => x.classList.toggle('active', x === c));
      $('focus-custom').value = '';
      renderFocusTimer();
    });
  }
  $('focus-custom').addEventListener('input', (e) => {
    const v = Math.round(Number(e.target.value));
    if (v >= 1 && v <= 480) {
      state.focusTarget = v;
      chips.forEach((x) => x.classList.remove('active'));
      renderFocusTimer();
    }
  });
  $('focus-start').addEventListener('click', async () => {
    await call('start_focus', { label: $('focus-label').value, targetMinutes: state.focusTarget });
    $('focus-label').value = '';
    await refreshStatus();
    refresh();
  });
  $('focus-stop').addEventListener('click', async () => {
    await call('stop_focus');
    await refreshStatus();
    refresh();
    toast('Focus session saved');
  });

  // Settings
  $('settings-form').addEventListener('submit', async (e) => {
    e.preventDefault();
    const settings = {
      idleThresholdSecs: Number($('s-idle').value),
      captureTitles: $('s-titles').checked,
      excludedApps: $('s-excluded').value.split('\n').map((s) => s.trim()).filter(Boolean),
      focusMinMinutes: Number($('s-focus-min').value),
      focusBreakToleranceMinutes: Number($('s-focus-tol').value),
      retentionDays: Number($('s-retention').value),
    };
    await call('save_settings', { settings });
    await call('set_autostart', { enabled: $('s-autostart').checked }).catch(() => {});
    $('settings-msg').textContent = 'Saved';
    toast('Settings saved');
  });
  for (const fmt of ['csv', 'json']) {
    $(`export-${fmt}`).addEventListener('click', async () => {
      const path = await call('export_data', { format: fmt });
      toast(`Exported to ${path}`);
    });
  }
  confirmButton($('clear-titles'), async () => { await call('clear_titles'); toast('All window titles erased'); });
  confirmButton($('delete-all'), async () => { await call('delete_all_data'); toast('All data deleted'); });

  // Re-render when the window comes back, and roll over at midnight.
  document.addEventListener('visibilitychange', () => { if (!document.hidden) { refreshStatus(); refresh(); } });
}

async function main() {
  renderLegend();
  wire();
  state.categories = await call('list_categories');
  await refreshStatus();
  await refresh();

  setInterval(refreshStatus, 5000);
  setInterval(() => { if (state.view === 'focus') renderFocusTimer(); }, 1000);
  setInterval(() => {
    if (document.hidden) return;
    if (state.view === 'today' && isSameDay(state.day, new Date())) refresh();
    else if (state.view === 'week' || state.view === 'focus') refresh();
  }, 30000);
}

main();
