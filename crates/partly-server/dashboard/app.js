// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

"use strict";
const $ = id => document.getElementById(id);
const notice = $('notice');
let noticeReset;
const route = location.pathname.match(/^\/environments\/(development|production)\/(flags|apps|audits|docs|users)\/?$/);
const environment = route?.[1] || 'development';
const currentScreen = route?.[2] || 'flags';
const trackers = new Map();
let setupRequired = false;
let currentUser;
let canEdit = false;
let canManage = false;
let unloading = false;
let modalEditor;
let nextId = 0;
let docsSource = '';
const docsExamples = {
  python: { name: 'Python', file: 'quickstart.py', url: '/docs/python.py' },
  rust: { name: 'Rust', file: 'quickstart.rs', url: '/docs/rust.rs' },
  go: { name: 'Go', file: 'main.go', url: '/docs/go.go' },
  node: { name: 'Node.js', file: 'quickstart.js', url: '/docs/node.js' },
};
let availableFlags = [];
const flagUsage = new Map();
let keyApplication;
let appSelection;
const ops = {
  le: ['≤', 'number'], lt: ['<', 'number'], ge: ['≥', 'number'], gt: ['>', 'number'], eq: ['=', 'number'],
  prefix: ['Starts with', 'string'], suffix: ['Ends with', 'string'], contains: ['Contains', 'string'],
  matches: ['Matches', 'string'], regex: ['Regex', 'string'], is: ['Is', 'boolean'],
};

function node(tag, text, className) {
  const element = document.createElement(tag);
  if (text) element.textContent = text;
  if (className) element.className = className;
  return element;
}
function button(text, className, handler) {
  const element = node('button', text, className); element.type = 'button';
  if (handler) element.addEventListener('click', handler);
  return element;
}
function label(text, input) {
  const element = node('label');
  const control = input.matches('[role=combobox]') ? node('span', '', 'select-control') : input;
  if (control !== input) control.append(input);
  element.append(node('span', text), control);
  return element;
}
function input(name, value = '', type = 'text') {
  const element = node('input'); element.name = name; element.type = type; element.value = value; element.required = true; return element;
}
function numberValue(value) {
  const result = Number(value);
  if (value === '' || !Number.isFinite(result)) throw new Error('Enter a valid, finite number.');
  if (Number.isInteger(result) && !Number.isSafeInteger(result)) throw new Error('This number is too large to edit accurately in the dashboard. Use a number between −9007199254740991 and 9007199254740991.');
  return result;
}
async function api(path, method = 'GET', body, headers = {}) {
  const response = await fetch(`/api${path}`, {
    method, credentials: 'same-origin',
    headers: { ...headers, 'X-Partly-Request': 'dashboard', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    let message = await response.text();
    try { message = JSON.parse(message).error || message; } catch { /* Validation may return plain text. */ }
    if (response.status === 401) message = path === '/login'
      ? 'Username or password is incorrect.'
      : 'Your session has ended. Save a copy of your edits, then reload to sign in.';
    throw new Error(message || `Request failed (${response.status})`);
  }
  return response.status === 204 ? undefined : response.json();
}
async function action(trigger, operation, errorArea) {
  closeDropdown();
  const keyRow = trigger?.closest('.key-row');
  const form = trigger?.closest('form') || keyRow?.querySelector('form');
  const state = trackers.get(form);
  if (state?.pending) return;
  const scope = keyRow || form;
  errorArea ||= scope?.querySelector('.item-error') || $('error');
  errorArea.hidden = true;
  clearTimeout(noticeReset); notice.textContent = '';
  const focused = document.activeElement;
  const controls = scope ? [...scope.querySelectorAll('input, button')].map(element => [element, element.disabled]) : [];
  if (form === $('new-flag')) controls.push([$('close-flag'), $('close-flag').disabled]);
  if (form === $('new-app')) controls.push([$('close-app'), $('close-app').disabled]);
  if (form === $('new-key')) controls.push([$('close-key'), $('close-key').disabled]);
  if (form === $('user-form')) controls.push([$('close-user'), $('close-user').disabled]);
  if (state) state.pending = true;
  if (scope) scope.setAttribute('aria-busy', 'true');
  controls.forEach(([element]) => { element.disabled = true; });
  if (trigger) trigger.disabled = true;
  try { await operation(); }
  catch (error) {
    errorArea.textContent = error.message;
    errorArea.hidden = false;
    errorArea.scrollIntoView({ block: 'nearest' });
  }
  finally {
    controls.forEach(([element, disabled]) => { element.disabled = disabled; });
    if (scope) scope.removeAttribute('aria-busy');
    if (state) state.pending = false;
    if (trigger && !form) trigger.disabled = false;
    updateDirty();
    const active = document.activeElement;
    const closedDialog = active.closest('dialog:not([open])');
    if ((active === document.body || closedDialog || (scope?.contains(active) && active.disabled)) && scope?.isConnected) {
      const preferred = closedDialog ? trigger : focused;
      const target = preferred !== document.body && preferred?.isConnected && !preferred.disabled
        ? preferred
        : scope.querySelector('input:not([disabled]), button:not([disabled])');
      target?.focus();
    }
  }
}
function showNotice(message, container) {
  clearTimeout(noticeReset);
  container.classList.add('feedback-anchor');
  container.append(notice);
  notice.textContent = message;
  noticeReset = setTimeout(() => { notice.textContent = ''; }, 3000);
}
function beforeUnload(event) {
  if (!unloading && isDirty()) { event.preventDefault(); event.returnValue = ''; }
}
function isDirty(element) {
  if (element) {
    const state = trackers.get(element);
    return !!state && state.baseline !== JSON.stringify(state.read());
  }
  return [...trackers.values()].some(state => state.baseline !== JSON.stringify(state.read()));
}
function updateDirty() {
  let anyDirty = false;
  for (const [element, state] of trackers) {
    const dirty = state.baseline !== JSON.stringify(state.read());
    anyDirty ||= dirty;
    element.dataset.dirty = String(dirty);
    if (state.save && !state.pending) state.save.disabled = !dirty;
    if (state.indicator) state.indicator.style.visibility = dirty ? 'visible' : 'hidden';
  }
  window.removeEventListener('beforeunload', beforeUnload);
  if (anyDirty && !unloading) window.addEventListener('beforeunload', beforeUnload);
}
function track(element, read, save, indicator) {
  trackers.set(element, { read, baseline: JSON.stringify(read()), save, indicator });
  element.dataset.dirty = 'false';
  if (save) save.disabled = true;
  if (indicator) indicator.style.visibility = 'hidden';
}
function untrack(element) { trackers.delete(element); updateDirty(); }
function markSaved(element) {
  const state = trackers.get(element); state.baseline = JSON.stringify(state.read()); updateDirty();
}
document.addEventListener('input', updateDirty);
document.addEventListener('change', updateDirty);
function confirmDialog(dialog, cancel, submit) {
  dialog.showModal(); cancel.focus();
  return new Promise(resolve => {
    const finish = answer => { dialog.close(); cancel.onclick = null; submit.onclick = null; dialog.oncancel = null; resolve(answer); };
    cancel.onclick = () => finish(false);
    submit.onclick = () => finish(true);
    dialog.oncancel = event => { event.preventDefault(); finish(false); };
  });
}
function confirmDiscard() {
  return confirmDialog($('discard-dialog'), $('keep-editing'), $('discard-changes'));
}
async function navigate(path, focusTab = false) {
  if (isDirty() && !await confirmDiscard()) return false;
  unloading = true; window.removeEventListener('beforeunload', beforeUnload);
  if (focusTab) sessionStorage.setItem('partly-focus-tab', 'yes');
  location.assign(path); return true;
}

function createEditor(flag, name) {
  const area = node('div', '', 'rollout-editor');
  const mode = createDropdown([
    ['enabled', 'Enabled for everyone'], ['percentage', 'Percentage of users'], ['custom', 'Custom rules'], ['disabled', 'Disabled for everyone'],
  ], flag.mode, 'mode'); mode.setAttribute('aria-label', `Rollout mode for ${name}`);
  const modeLabel = label('Rollout mode', mode); modeLabel.className = 'mode-label'; area.append(modeLabel);
  const percentageArea = node('div', '', 'percentage-editor');
  const percentage = input('percentage', flag.percentage ?? 50, 'number'); percentage.min = '0'; percentage.max = '100'; percentage.step = '1'; percentage.setAttribute('aria-label', `Percentage for ${name}`);
  percentageArea.append(label('Rollout', percentage), node('span', '%'), node('p', 'Stable selection using context.id.')); area.append(percentageArea);
  const custom = node('div', '', 'custom-editor');
  const customHeading = node('div', '', 'custom-heading');
  const explanation = node('div'); explanation.append(node('h2', 'Match your context'), node('p', 'Match any group. All conditions in a group must match.'));
  const matchAction = createDropdown([['enable', 'Enable matches; others off'], ['disable', 'Disable matches; others on']], flag.action || 'enable', 'action');
  matchAction.setAttribute('aria-label', `Match action for ${name}`);
  const actionLabel = label('When a group matches', matchAction); actionLabel.className = 'match-action'; customHeading.append(explanation, actionLabel); custom.append(customHeading);
  const groupsArea = node('div', '', 'rule-groups'); custom.append(groupsArea);
  const groups = [];
  function updateGroupLabels() {
    groups.forEach((group, index) => {
      group.title.textContent = `Group ${index + 1} · Match all`;
      group.join.hidden = index === 0;
      group.remove.disabled = groups.length === 1;
      group.conditions.forEach(condition => { condition.remove.disabled = group.conditions.length === 1; });
    });
    if (area.isConnected) updateDirty();
  }
  function addGroup(conditions = [{ k: '', op: 'matches', v: '' }]) {
    const join = node('div', 'OR', 'group-join');
    const box = node('div', '', 'rule-group');
    const heading = node('div', '', 'group-heading'); const title = node('h3');
    title.id = `rule-group-${++nextId}`;
    box.setAttribute('role', 'group');
    box.setAttribute('aria-labelledby', title.id);
    const group = { box, title, join, conditions: [] };
    const remove = button('Remove group', 'quiet', () => {
      const index = groups.indexOf(group);
      groups.splice(index, 1);
      box.remove();
      join.remove();
      updateGroupLabels();
      groups[Math.min(index, groups.length - 1)].conditions[0].key.focus();
    });
    group.remove = remove; heading.append(title, remove); box.append(heading);
    const rows = node('div'); box.append(rows);
    function addCondition(condition = { k: '', op: 'matches', v: '' }) {
      const row = node('div', '', 'condition');
      const key = input('k', condition.k); key.maxLength = 64; key.placeholder = 'e.g. plan'; key.setAttribute('aria-label', 'Context key');
      const operator = createDropdown(Object.entries(ops).map(([key, [text, type]]) => [key, `${text} (${type[0].toUpperCase()}${type.slice(1)})`]), condition.op, 'op');
      operator.setAttribute('aria-label', 'Operator');
      const valueArea = node('div', '', 'rule-value');
      let valueInput; let booleanInputs; let currentType = ops[condition.op][1];
      const helpId = `value-help-${++nextId}`;
      function setValue(type, value) {
        valueArea.replaceChildren(); booleanInputs = null;
        if (type === 'boolean') {
          const choices = node('div', '', 'bool-values'); choices.setAttribute('role', 'group'); choices.setAttribute('aria-label', 'Comparison value');
          choices.setAttribute('aria-describedby', helpId);
          const radioName = `bool-${++nextId}`;
          booleanInputs = [true, false].map(choice => {
            const radio = input(radioName, String(choice), 'radio'); radio.checked = value === choice;
            choices.append(label(choice ? 'True' : 'False', radio)); return radio;
          });
          valueArea.append(node('span', 'Value', 'small'), choices);
        } else {
          valueInput = input('v', value ?? '', type === 'number' ? 'number' : 'text');
          if (type === 'number') {
            valueInput.step = 'any';
            valueInput.addEventListener('input', () => {
              valueInput.setCustomValidity('');
              try { numberValue(valueInput.value); } catch (error) { valueInput.setCustomValidity(error.message); }
            });
          }
          // Empty strings are valid comparisons; numbers must always have a value.
          valueInput.required = type === 'number';
          valueInput.setAttribute('aria-label', 'Comparison value'); valueInput.setAttribute('aria-describedby', helpId);
          valueArea.append(label('Value', valueInput));
        }
        const help = node('p', '', 'value-help'); help.id = helpId; valueArea.append(help); updateHelp();
      }
      function readValue() { return booleanInputs ? (booleanInputs.find(item => item.checked)?.value ?? '') : valueInput.value; }
      function updateHelp() {
        const meaning = operator.value === 'matches' ? 'Exact equality. ' : operator.value === 'regex' ? 'Rust regex pattern. ' : '';
        valueArea.querySelector('.value-help').textContent = `${meaning}Expects a ${currentType}. A different context type raises ContextTypeError.${currentType === 'string' ? ' Case-sensitive.' : ''}`;
      }
      operator.addEventListener('change', () => {
        const type = ops[operator.value][1];
        if (type !== currentType) { currentType = type; setValue(type, undefined); }
        else updateHelp();
        updateDirty();
      });
      setValue(currentType, condition.v);
      const item = { row, key, operator, readValue, remove: null };
      const removeCondition = button('×', 'quiet remove-condition', () => {
        const index = group.conditions.indexOf(item);
        group.conditions.splice(index, 1);
        row.remove();
        updateGroupLabels();
        group.conditions[Math.min(index, group.conditions.length - 1)].key.focus();
      });
      removeCondition.setAttribute('aria-label', 'Remove condition'); item.remove = removeCondition;
      row.append(label('Context key', key), label('Operator', operator), valueArea, removeCondition);
      group.conditions.push(item); rows.append(row); updateGroupLabels();
      return key;
    }
    const footer = node('div', '', 'group-footer'); footer.append(button('+ AND condition', '', () => addCondition().focus())); box.append(footer);
    groups.push(group); groupsArea.append(join, box); conditions.forEach(addCondition); updateGroupLabels();
    return group.conditions[0].key;
  }
  (flag.groups || [[{ k: '', op: 'matches', v: '' }]]).forEach(addGroup);
  custom.append(button('+ OR group', 'add-group', () => addGroup().focus()));
  area.append(custom);
  function updateMode() {
    percentageArea.hidden = mode.value !== 'percentage'; custom.hidden = mode.value !== 'custom';
    percentage.disabled = percentageArea.hidden;
    custom.querySelectorAll('input, [role=combobox]').forEach(element => { element.disabled = custom.hidden; });
    if (area.isConnected) updateDirty();
  }
  mode.addEventListener('change', updateMode); updateMode();
  function read() {
    if (mode.value === 'percentage') return { mode: mode.value, percentage: percentage.value };
    if (mode.value === 'custom') return { mode: mode.value, action: matchAction.value, groups: groups.map(group => group.conditions.map(item => ({ k: item.key.value, op: item.operator.value, v: item.readValue() }))) };
    return { mode: mode.value };
  }
  function value() {
    const draft = read();
    if (draft.mode === 'percentage') draft.percentage = Number(draft.percentage);
    if (draft.mode === 'custom') for (const group of draft.groups) for (const rule of group) {
      const type = ops[rule.op][1];
      if (type === 'number') rule.v = numberValue(rule.v);
      if (type === 'boolean') { if (rule.v === '') throw new Error('Choose True or False for every boolean comparison.'); rule.v = rule.v === 'true'; }
    }
    return draft;
  }
  return { area, read, value };
}
function updateFlagCount() {
  const count = $('flags').querySelectorAll('.flag-row').length; $('flag-count').textContent = count;
  $('flags').querySelector('.empty')?.remove();
  if (!count) {
    const empty = node('div', '', 'empty'); empty.append(node('h3', 'No flags in this environment'), node('p', 'Create your first flag to start controlling a rollout.')); $('flags').append(empty);
  }
}
function dateTime(value) {
  const time = node('time', new Date(value).toLocaleString()); time.dateTime = value; return time;
}
function usageDetails(key) {
  const entries = flagUsage.get(key) || [];
  if (!entries.length) return node('p', 'No usage reported', 'flag-usage usage-empty');
  const latest = entries.reduce((value, entry) => entry.last_used_at > value ? entry.last_used_at : value, '');
  const details = node('details', '', 'flag-usage');
  const summary = node('summary'); summary.append(node('span', 'Last reported use', 'usage-label'), dateTime(latest)); details.append(summary);
  const table = node('table'); table.setAttribute('aria-label', `Usage for ${key}`); table.setAttribute('role', 'table');
  const head = node('thead'); const headings = node('tr');
  head.setAttribute('role', 'rowgroup'); headings.setAttribute('role', 'row');
  for (const text of ['Application', 'API Key', 'Last reported use']) { const cell = node('th', text); cell.scope = 'col'; headings.append(cell); }
  head.append(headings); const body = node('tbody'); body.setAttribute('role', 'rowgroup');
  for (const entry of entries) {
    const row = node('tr'); row.setAttribute('role', 'row');
    for (const [title, value] of [['Application', entry.application_name], ['API Key', entry.key_name], ['Last reported use', new Date(entry.last_used_at).toLocaleString()]]) {
      const cell = node('td'); cell.setAttribute('role', 'cell');
      const label = node('span', title, 'usage-field-label'); label.setAttribute('aria-hidden', 'true');
      cell.append(label, node('span', value)); row.append(cell);
    }
    body.append(row);
  }
  table.append(head, body); details.append(table);
  return details;
}
function flagRow(key, flag) {
  const form = node('form', '', 'flag-row'); form.dataset.key = key;
  const heading = node('div', '', 'flag-heading'); const identity = node('div', '', 'flag-identity');
  const indicator = node('span', 'Unsaved', 'draft-indicator');
  const title = node('h2', '', 'flag-title'); title.id = `flag-title-${++nextId}`; title.append(node('code', key, 'flag-name'));
  form.setAttribute('aria-labelledby', title.id); identity.append(title, indicator);
  const actions = node('div', '', 'row-actions');
  const save = node('button', 'Save'); save.type = 'submit'; save.setAttribute('aria-label', `Save ${key}`);
  const remove = button('Delete', 'quiet danger', () => {
    action(remove, async () => {
      const confirmation = await confirmFlag(key, true);
      if (!confirmation) return;
      await api(`/environments/${environment}/flags/${key}`, 'DELETE', undefined, confirmation); flagUsage.delete(key); untrack(form); form.remove(); updateFlagCount(); showNotice('Deleted', $('flags'));
    });
  }); remove.setAttribute('aria-label', `Delete ${key}`);
  actions.append(save, remove); heading.append(identity, actions); form.append(heading);
  const editor = createEditor(flag, key); form.append(editor.area, usageDetails(key));
  const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert'); form.append(error);
  if (!canEdit) {
    actions.hidden = true; indicator.hidden = true;
    editor.area.querySelectorAll('input, button').forEach(control => { control.disabled = true; });
  } else track(form, editor.read, save, indicator);
  form.addEventListener('submit', event => {
    event.preventDefault();
    action(save, async () => {
      const draft = editor.value();
      const confirmation = await confirmFlag(key, false);
      if (!confirmation) return;
      const saved = await api(`/environments/${environment}/flags/${key}`, 'PUT', draft, confirmation);
      const replacement = flagRow(saved.key, saved);
      untrack(form);
      form.replaceWith(replacement);
      requestAnimationFrame(() => {
        const active = document.activeElement;
        if (active === document.body || active.closest('dialog:not([open])')) replacement.querySelector('[name=mode]').focus();
      });
      showNotice('Saved', replacement);
    });
  });
  return form;
}
async function confirmFlag(key, deleting) {
  const impact = await api(`/environments/${environment}/flags/${key}/impact`);
  const expected = `${environment}/${key}`;
  const dialog = $('confirm-flag-dialog');
  $('confirm-flag-title').textContent = deleting ? `Delete ${key}?` : `Update ${key}?`;
  $('flag-impact-description').textContent = impact.applications.length
    ? `This change affects the following applications in ${environment} after their next successful refresh.${deleting ? ' Evaluating this flag will then return an unknown-flag error.' : ''}`
    : 'No applications use this flag.';
  $('flag-impact-apps').replaceChildren(...impact.applications.map(app => node('li', app.name)));
  $('confirm-flag-label').replaceChildren('Type ', node('code', expected, 'confirmation-target'), ' to confirm');
  const field = $('confirm-flag-input'); field.value = '';
  const submit = $('confirm-flag-submit'); submit.textContent = deleting ? 'Delete flag' : 'Save changes'; submit.disabled = true;
  $('confirm-flag-error').hidden = true;
  field.oninput = () => { submit.disabled = field.value !== expected; };
  dialog.showModal(); field.focus();
  return new Promise(resolve => {
    const finish = confirmed => {
      dialog.close(); field.oninput = null; $('confirm-flag-form').onsubmit = null; dialog.oncancel = null; $('cancel-flag-confirmation').onclick = null;
      resolve(confirmed ? { 'X-Partly-Confirm': expected, 'X-Partly-Impact': impact.token } : null);
    };
    $('confirm-flag-form').onsubmit = event => { event.preventDefault(); if (field.value === expected) finish(true); };
    $('cancel-flag-confirmation').onclick = () => finish(false);
    dialog.oncancel = event => { event.preventDefault(); finish(false); };
  });
}

function flagSelection(selected, name) {
  const area = node('fieldset', '', 'flag-selection'); area.append(node('legend', 'Selected flags'));
  const search = input('search', '', 'search'); search.required = false; search.placeholder = 'Find a flag'; search.setAttribute('aria-label', `Find flags for ${name}`);
  const list = node('div', '', 'flag-choices');
  const choices = availableFlags.map(key => {
    const checkbox = input('flags', key, 'checkbox'); checkbox.required = false; checkbox.checked = selected.includes(key);
    const row = label(key, checkbox); list.append(row); return { key, checkbox, row };
  });
  const empty = node('p', availableFlags.length ? 'No matching flags.' : 'No flags in this environment.', 'small muted');
  empty.hidden = availableFlags.length > 0;
  search.addEventListener('input', () => {
    for (const choice of choices) choice.row.hidden = !choice.key.includes(search.value);
    empty.hidden = choices.some(choice => !choice.row.hidden);
  });
  area.append(search, list, empty);
  return { area, read: () => choices.filter(choice => choice.checkbox.checked).map(choice => choice.key) };
}

function addKey(key, application) {
  const { id, masked_secret: masked, revoked_at: revoked, created_at: created } = key;
  let { name, secret } = key;
  const row = node('div', '', 'key-row');
  const metadata = node('dl', '', 'key-metadata');
  for (const [label, value] of [['Created', created], ...(revoked ? [['Revoked', revoked]] : []), ['Last reported use', key.last_used_at]]) {
    const property = node('div'); const detail = node('dd');
    detail.append(value ? dateTime(value) : node('span', 'No usage reported', 'muted'));
    property.append(node('dt', label), detail); metadata.append(property);
  }
  if (revoked || !canManage) {
    row.classList.add('revoked-key');
    row.append(node('strong', name), node('code', masked), metadata);
    return row;
  }
  const rename = node('form', '', 'key-name-form');
  const nameInput = input('name', name); nameInput.maxLength = 64;
  const saveName = node('button', 'Save name'); saveName.type = 'submit';
  const value = input('secret', masked); value.readOnly = true; value.autocomplete = 'off';
  const copy = button('Copy');
  function updateNames() {
    nameInput.setAttribute('aria-label', `Name for ${name}`); saveName.setAttribute('aria-label', `Save name for ${name}`);
    value.setAttribute('aria-label', `API Key for ${name}`); copy.setAttribute('aria-label', `Copy API Key for ${name}`);
  }
  updateNames(); rename.append(label('Key name', nameInput), saveName); track(rename, () => nameInput.value, saveName);
  rename.addEventListener('submit', event => {
    event.preventDefault(); action(saveName, async () => {
      await api(`/keys/${id}`, 'PUT', { name: nameInput.value }); name = nameInput.value.trim(); key.name = name; nameInput.value = name;
      updateNames(); markSaved(rename); showNotice('Saved', row);
    });
  });
  const controls = node('div', '', 'key-controls'); const secretLabel = label('API Key', value); secretLabel.className = 'key-secret';
  let visible = false;
  const reveal = button('Reveal', '', () => { visible = !visible; value.value = visible ? secret : masked; reveal.textContent = visible ? 'Hide' : 'Reveal'; reveal.setAttribute('aria-pressed', String(visible)); }); reveal.setAttribute('aria-pressed', 'false');
  copy.addEventListener('click', () => action(copy, async () => {
    try { await navigator.clipboard.writeText(secret); } catch { throw new Error('Could not copy. Allow clipboard access or use Reveal to copy the key manually.'); }
    showNotice('Copied', row);
  }));
  const revoke = button('Revoke', 'quiet danger', () => {
    action(revoke, async () => {
      $('revoke-key-context').replaceChildren('Revoke ', node('strong', name), ' for ', node('strong', application.name), ' in ', node('strong', environment), '.');
      if (!await confirmDialog($('revoke-key-dialog'), $('cancel-key-revocation'), $('confirm-key-revocation'))) return;
      await api(`/keys/${id}`, 'DELETE'); untrack(rename);
      secret = null; key.secret = null; key.revoked_at = new Date().toISOString();
      const replacement = addKey(key, application); row.remove(); application.revoked.append(replacement); application.updateRevoked();
      showNotice('Revoked', application.heading.querySelector('button'));
      application.heading.querySelector('button').focus();
    });
  });
  const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert');
  controls.append(secretLabel, copy, reveal, revoke); row.append(rename, controls, metadata, error);
  return row;
}

function appRow(app) {
  const row = node('article', '', 'app-row'); row.dataset.appId = app.id;
  const heading = node('div', '', 'app-heading'); const title = node('h2', app.name); title.id = `${app.id}-title`; row.setAttribute('aria-labelledby', title.id);
  const identity = node('div', '', 'app-identity'); identity.append(node('code', app.id));
  const copyId = button('Copy ID', 'quiet', () => action(copyId, async () => { await navigator.clipboard.writeText(app.id); showNotice('Copied', copyId); }));
  copyId.setAttribute('aria-label', 'Copy ID');
  identity.append(copyId); heading.append(title, identity); row.append(heading);
  const settingsLabel = () => `App settings · ${app.flags.length} selected ${app.flags.length === 1 ? 'flag' : 'flags'}`;
  const settings = node('details', '', 'app-settings'); settings.append(node('summary', settingsLabel()));
  const form = node('form'); const name = input('name', app.name); name.maxLength = 64; name.setAttribute('aria-label', `Application name for ${app.name}`);
  const selection = flagSelection(app.flags, app.name); const save = node('button', 'Save app', 'primary'); save.type = 'submit'; save.setAttribute('aria-label', 'Save app');
  const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert');
  form.append(label('Application name', name), selection.area, save, error); settings.append(form); row.append(settings);
  if (canManage) track(form, () => ({ name: name.value, flags: selection.read() }), save);
  else { name.readOnly = true; selection.area.disabled = true; save.hidden = true; }
  form.addEventListener('submit', event => {
    event.preventDefault(); action(save, async () => {
      const saved = await api(`/environments/${environment}/apps/${app.id}`, 'PUT', { name: name.value, flags: selection.read() });
      app.name = saved.name; application.name = saved.name; app.flags = saved.flags; name.value = saved.name; title.textContent = saved.name; name.setAttribute('aria-label', `Application name for ${saved.name}`);
      settings.querySelector('summary').textContent = settingsLabel();
      keys.setAttribute('aria-label', `API Keys for ${saved.name}`);
      markSaved(form); showNotice('Saved', save);
    });
  });
  const keyHeading = node('div', '', 'list-heading'); keyHeading.append(node('h3', 'API Keys'));
  const active = node('div', '', 'active-keys'); const revoked = node('details', '', 'revoked-keys'); const summary = node('summary'); revoked.append(summary);
  const application = { ...app, row, heading: keyHeading, active, revoked, updateRevoked: () => { const count = revoked.querySelectorAll('.key-row').length; summary.textContent = `Revoked keys (${count})`; revoked.hidden = count === 0; } };
  const createKey = button('Create key', '', () => openKey(application)); createKey.setAttribute('aria-label', 'Create key');
  createKey.hidden = !canManage; keyHeading.append(createKey);
  const keys = node('section', '', 'app-keys'); keys.setAttribute('aria-label', `API Keys for ${app.name}`); keys.append(keyHeading, active, revoked); row.append(keys);
  for (const key of app.keys) (key.revoked_at ? revoked : active).append(addKey(key, application));
  application.updateRevoked();
  return row;
}

function openKey(application) {
  keyApplication = application; $('new-key').reset(); $('key-dialog-error').hidden = true;
  $('key-dialog-context').textContent = `Generate API Key for ${application.name} in ${environment}.`;
  track($('new-key'), () => $('key-name').value); $('key-dialog').showModal(); $('key-name').focus();
}

async function closeApp() {
  if (trackers.get($('new-app'))?.pending) return;
  if (isDirty($('new-app')) && !await confirmDiscard()) return;
  untrack($('new-app')); $('app-dialog').close();
}
$('create-app').addEventListener('click', () => {
  $('new-app').reset(); $('app-dialog-error').hidden = true; $('app-dialog-context').textContent = `Create app in ${environment}.`;
  appSelection = flagSelection([], 'new app'); $('app-flags').replaceChildren(appSelection.area);
  track($('new-app'), () => ({ name: $('app-name').value, flags: appSelection.read() })); $('app-dialog').showModal(); $('app-name').focus();
});
$('close-app').addEventListener('click', closeApp); $('cancel-app').addEventListener('click', closeApp);
$('app-dialog').addEventListener('cancel', event => { event.preventDefault(); closeApp(); });
$('new-app').addEventListener('submit', event => {
  event.preventDefault(); action(event.submitter, async () => {
    const app = await api(`/environments/${environment}/apps`, 'POST', { name: $('app-name').value, flags: appSelection.read() });
    const row = appRow(app); $('apps').querySelector('.empty')?.remove(); $('apps').append(row);
    untrack($('new-app')); $('app-dialog').close(); showNotice('Created', row.querySelector('.app-identity button'));
  });
});

let editingUser;
function userDraft() {
  const permissions = {};
  if (!$('user-admin').checked) {
    $('user-permissions').querySelectorAll('[role=combobox]').forEach(control => { if (control.value) permissions[control.name] = control.value; });
  }
  return { username: $('user-name').value, enabled: $('user-enabled').checked, is_admin: $('user-admin').checked, permissions, ...($('user-password').value ? { password: $('user-password').value } : {}) };
}
function userRow(user) {
  const row = node('article', '', 'user-row'); row.dataset.userId = user.id;
  const identity = node('div'); identity.append(node('h2', user.username));
  identity.append(node('p', user.is_admin ? 'Instance administrator' : Object.entries(user.permissions).map(([env, role]) => `${env}: ${role}`).join(' · ') || 'No environment access', 'small muted'));
  const controls = node('div', '', 'row-actions');
  if (!user.enabled) controls.append(node('span', 'Disabled', 'type-pill'));
  const edit = button('Edit', '', () => openUser(user)); edit.setAttribute('aria-label', `Edit ${user.username}`);
  controls.append(edit); row.append(identity, controls); return row;
}
function openUser(user) {
  editingUser = user;
  $('user-form').reset(); $('user-dialog-error').hidden = true;
  $('user-dialog-title').textContent = user ? `Edit ${user.username}` : 'Create user';
  $('user-dialog-context').textContent = user ? 'Saving changes signs this user out of all sessions.' : 'Assign access separately for each environment.';
  $('user-name').value = user?.username || ''; $('user-name').readOnly = !!user;
  $('user-password').required = !user; $('user-password').placeholder = user ? 'Leave blank to keep the password' : 'At least 12 characters';
  $('user-enabled').checked = user?.enabled ?? true; $('user-admin').checked = user?.is_admin || false;
  $('user-enabled').disabled = user?.id === 1; $('user-admin').disabled = user?.id === 1;
  $('user-recovery-note').hidden = user?.id !== 1;
  $('user-permissions').replaceChildren(...['development', 'production'].map(env => {
    const control = createDropdown([['', 'No access'], ['viewer', 'Viewer'], ['editor', 'Editor'], ['manager', 'Manager']], user?.permissions[env] || '', env);
    control.setAttribute('aria-label', `${env} access`); control.disabled = $('user-admin').checked;
    return label(env[0].toUpperCase() + env.slice(1), control);
  }));
  $('save-user').textContent = user ? 'Save changes' : 'Create user';
  $('save-user').disabled = false;
  track($('user-form'), userDraft, user ? $('save-user') : undefined);
  $('user-dialog').showModal(); (user ? $('user-password') : $('user-name')).focus();
}
$('user-admin').addEventListener('change', () => {
  $('user-permissions').querySelectorAll('[role=combobox]').forEach(control => { control.disabled = $('user-admin').checked; });
});
async function closeUser() {
  if (trackers.get($('user-form'))?.pending) return;
  if (isDirty($('user-form')) && !await confirmDiscard()) return;
  untrack($('user-form')); $('user-password').value = ''; $('user-dialog').close();
}
$('create-user').addEventListener('click', () => openUser(null));
$('close-user').addEventListener('click', closeUser); $('cancel-user').addEventListener('click', closeUser);
$('user-dialog').addEventListener('cancel', event => { event.preventDefault(); closeUser(); });
$('user-form').addEventListener('submit', event => {
  event.preventDefault(); action(event.submitter, async () => {
    const saved = await api(editingUser ? `/users/${editingUser.id}` : '/users', editingUser ? 'PUT' : 'POST', userDraft());
    const row = userRow(saved);
    if (editingUser) $('users').querySelector(`[data-user-id="${saved.id}"]`).replaceWith(row);
    else $('users').append(row);
    untrack($('user-form')); $('user-password').value = ''; $('user-dialog').close();
    row.querySelector('button').focus();
    showNotice(editingUser ? 'Saved' : 'Created', row.querySelector('button'));
    if (saved.id === currentUser.id) location.reload();
  }, $('user-dialog-error'));
});

function auditState(type, value, title) {
  const state = node('div', '', 'audit-state');
  state.dataset.phase = title.toLowerCase();
  state.append(node('h3', title, 'audit-label'));
  if (type === 'flag') {
    const modes = { enabled: 'Enabled for everyone', disabled: 'Disabled for everyone', percentage: `${value.percentage}% of users`, custom: 'Custom rules' };
    state.append(node('p', modes[value.mode], 'audit-value'));
    if (value.mode === 'custom') {
      state.append(node('p', value.action === 'enable' ? 'Enable when a group matches' : 'Disable when a group matches'));
      value.groups.forEach((group, index) => {
        if (index) state.append(node('p', 'OR', 'audit-join'));
        const conditions = node('div', '', 'audit-rule-group');
        group.forEach((condition, index) => {
          if (index) conditions.append(node('p', 'AND', 'audit-join'));
          const rule = node('p', '', 'audit-condition');
          const value = typeof condition.v === 'string' ? `“${condition.v}”` : String(condition.v);
          rule.append(node('code', condition.k), ` ${ops[condition.op][0].toLowerCase()} `, node('span', value));
          conditions.append(rule);
        });
        state.append(conditions);
      });
      state.append(node('p', value.action === 'enable' ? 'Otherwise disabled.' : 'Otherwise enabled.', 'muted small'));
    }
  } else {
    state.append(node('p', value.name || value.status, 'audit-value'));
    if (type === 'user') {
      state.append(node('p', value.enabled ? 'Account enabled' : 'Account disabled'));
      state.append(node('p', value.is_admin ? 'Instance administrator' : `${value.role || 'No access'} in ${environment}`));
    }
    if (type === 'app') {
      state.append(node('p', 'Selected flags', 'audit-label'));
      if (value.flags.length) {
        const flags = node('ul', '', 'audit-flags');
        value.flags.forEach(key => { const item = node('li'); item.append(node('code', key)); flags.append(item); });
        state.append(flags);
      } else state.append(node('p', 'No flags selected', 'muted'));
    }
  }
  return state;
}

function auditChange(entry) {
  const change = node('div', '', 'audit-change');
  const { before, after, masked_secret: maskedSecret } = entry.details;
  if (entry.action === 'created') {
    const state = auditState(entry.item_type, after || { status: 'Active' }, 'Created');
    if (maskedSecret) state.append(node('p', 'Key identifier', 'audit-label'), node('code', maskedSecret));
    change.append(state);
  } else if (entry.action === 'deleted') {
    change.append(auditState(entry.item_type, before, 'Deleted'));
  } else {
    change.classList.add('audit-transition');
    const arrow = node('span', '→', 'audit-arrow'); arrow.setAttribute('aria-hidden', 'true');
    change.append(
      auditState(entry.item_type, before || { status: 'Active' }, 'Before'), arrow,
      auditState(entry.item_type, after || { status: 'Revoked' }, 'After'),
    );
  }
  return change;
}

async function setupAudits() {
  const { applications } = await api(`/environments/${environment}/apps`);
  const actionFilter = createDropdown([['', 'All actions'], ...['created', 'updated', 'deleted', 'renamed', 'revoked'].map(value => [value, value[0].toUpperCase() + value.slice(1)])], '', 'action');
  const appFilter = createDropdown([['', 'All applications'], ...applications.map(app => [app.id, app.name])], '', 'application_id');
  actionFilter.setAttribute('aria-label', 'Filter by action'); appFilter.setAttribute('aria-label', 'Filter by application');
  $('audit-filters').append(label('Action', actionFilter), label('Application', appFilter));
  let cursor;
  async function load(reset) {
    const params = new URLSearchParams();
    if (!reset && cursor) params.set('before', cursor);
    if (actionFilter.value) params.set('action', actionFilter.value);
    if (appFilter.value) params.set('application_id', appFilter.value);
    actionFilter.disabled = true; appFilter.disabled = true;
    try {
      const { audits } = await api(`/environments/${environment}/audits?${params}`);
      if (reset) {
        const heading = node('div', '', 'audit-summary audit-heading'); heading.setAttribute('aria-hidden', 'true');
        heading.append(...['Item', 'Action', 'Who', 'Time'].map(text => node('span', text)));
        $('audits').replaceChildren(heading);
      }
      for (const entry of audits) {
        const row = node('details', '', 'audit-row'); const summary = node('summary', '', 'audit-summary');
        const time = node('time', new Date(entry.created_at).toLocaleString()); time.dateTime = entry.created_at;
        const item = node('span', '', 'audit-item'); item.append(node('span', { flag: 'Flag', app: 'App', api_key: 'API Key', user: 'User' }[entry.item_type], 'type-pill'), node('span', entry.item_name));
        const actor = node('span', '', 'audit-actor'); actor.append(node('span', 'by ', 'audit-by'), node('span', entry.actor));
        const action = node('span', entry.action[0].toUpperCase() + entry.action.slice(1), 'audit-action'); action.dataset.action = entry.action;
        summary.append(item, action, actor, time);
        const detail = node('div', '', 'audit-detail');
        detail.append(auditChange(entry));
        if (entry.item_type === 'user' && entry.action === 'updated' && entry.details.password_changed) detail.append(node('p', 'Password reset', 'audit-context'));
        if (entry.item_type === 'api_key') detail.append(node('p', `Application: ${applications.find(app => app.id === entry.details.application_id)?.name || entry.details.application_id}`, 'audit-context'));
        if (entry.details.applications) detail.append(node('p', entry.details.applications.length ? `Affected applications: ${entry.details.applications.map(app => app.name).join(', ')}` : 'No applications used this flag.', 'audit-context'));
        row.append(summary, detail); $('audits').append(row);
      }
      if (!$('audits').querySelector('.audit-row')) $('audits').append(node('p', 'No audit entries match.', 'empty'));
      cursor = audits.at(-1)?.id; $('more-audits').hidden = audits.length < 100;
    } finally { actionFilter.disabled = false; appFilter.disabled = false; }
  }
  const refresh = () => action($('more-audits'), () => load(true));
  actionFilter.addEventListener('change', refresh); appFilter.addEventListener('change', refresh);
  $('more-audits').addEventListener('click', () => action($('more-audits'), () => load(false)));
  await load(true);
}

async function closeFlag() {
  if (trackers.get($('new-flag'))?.pending) return;
  if (isDirty($('new-flag')) && !await confirmDiscard()) return;
  untrack($('new-flag')); $('flag-dialog').close();
}
$('create-flag').addEventListener('click', () => {
  $('new-flag').reset(); $('flag-dialog-error').hidden = true;
  modalEditor = createEditor({ mode: 'disabled' }, 'new flag'); $('new-flag-editor').replaceChildren(modalEditor.area);
  $('flag-dialog-context').textContent = `Create flag in ${environment}.`;
  track($('new-flag'), () => ({ key: $('flag-key').value, ...modalEditor.read() })); $('flag-dialog').showModal(); $('flag-key').focus();
});
$('close-flag').addEventListener('click', closeFlag); $('cancel-flag').addEventListener('click', closeFlag);
$('flag-dialog').addEventListener('cancel', event => { event.preventDefault(); closeFlag(); });
$('new-flag').addEventListener('submit', event => {
  event.preventDefault(); action(event.submitter, async () => {
    const saved = await api(`/environments/${environment}/flags`, 'POST', { key: $('flag-key').value, ...modalEditor.value() });
    const row = flagRow(saved.key, saved);
    $('flags').querySelector('.empty')?.remove(); $('flags').append(row); updateFlagCount();
    untrack($('new-flag')); $('flag-dialog').close(); showNotice('Created', row);
  }, $('flag-dialog-error'));
});
$('new-key').addEventListener('submit', event => {
  event.preventDefault(); action(event.submitter, async () => {
    const key = await api(`/environments/${environment}/apps/${keyApplication.id}/keys`, 'POST', { name: $('key-name').value });
    const row = addKey(key, keyApplication); keyApplication.active.append(row);
    untrack($('new-key'));
    $('key-dialog').close();
    showNotice('Created', row);
  });
});
async function closeKey() {
  if (trackers.get($('new-key'))?.pending) return;
  if (isDirty($('new-key')) && !await confirmDiscard()) return;
  untrack($('new-key'));
  $('key-dialog').close();
}
$('close-key').addEventListener('click', closeKey);
$('cancel-key').addEventListener('click', closeKey);
$('key-dialog').addEventListener('cancel', event => { event.preventDefault(); closeKey(); });
for (const [id, options, value, label] of [
  ['docs-language', Object.entries(docsExamples).map(([value, example]) => [value, example.name]), 'python', 'Language'],
]) {
  const control = createDropdown(options, value, id);
  control.id = id; control.setAttribute('aria-label', label);
  $(id).replaceWith(control);
}
document.querySelector('.brand').href = `/environments/${environment}/flags`;
const tabs = [...$('screens').querySelectorAll('[role=tab]')];
for (const tab of tabs) {
  const active = tab.id === `${currentScreen}-tab`; tab.setAttribute('aria-selected', String(active)); tab.tabIndex = active ? 0 : -1; $(tab.getAttribute('aria-controls')).hidden = !active;
  const go = focus => { if (!active) return navigate(`/environments/${environment}/${tab.id.split('-')[0]}`, focus); };
  tab.addEventListener('click', () => go(false));
  tab.addEventListener('keydown', event => {
    const visibleTabs = tabs.filter(tab => !tab.hidden);
    const index = visibleTabs.indexOf(tab);
    let next;
    if (event.key === 'ArrowRight') next = (index + 1) % visibleTabs.length;
    else if (event.key === 'ArrowLeft') next = (index + visibleTabs.length - 1) % visibleTabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = visibleTabs.length - 1;
    else return;
    event.preventDefault();
    if (next === index) tab.focus(); else navigate(`/environments/${environment}/${visibleTabs[next].id.split('-')[0]}`, true);
  });
}
$('copy-example').addEventListener('click', () => action($('copy-example'), async () => {
  try { await navigator.clipboard.writeText(docsSource); } catch { throw new Error('Could not copy. Select the example and copy it manually.'); }
  showNotice('Copied', $('copy-example'));
}));
function highlightCode(source, language) {
  const keywords = {
    python: 'async|await|def|from|import|as|try|except|finally|if|elif|else|raise|return|with|for|in|not|and|or|is|None|True|False',
    rust: 'use|const|fn|pub|async|await|let|mut|match|if|else|return|for|in|true|false|impl|struct|enum|Self|self|where|move|dyn|type|as',
    go: 'package|import|const|var|func|defer|if|else|for|range|return|nil|true|false|map|type|struct|interface|go|select|case',
    node: 'const|let|var|function|async|await|try|catch|finally|throw|if|else|return|for|of|in|new|true|false|null|undefined',
  };
  const literals = language === 'python'
    ? /(?<comment>#[^\r\n]*)|(?<string>(?:\b(?:fr|rf|f|r|b))?(?:"""[\s\S]*?"""|'''[\s\S]*?'''|"(?:\\.|[^"\\\r\n])*"|'(?:\\.|[^'\\\r\n])*'))/.source
    : /(?<comment>\/\/[^\r\n]*|\/\*[\s\S]*?\*\/)|(?<string>"(?:\\.|[^"\\\r\n])*"|'(?:\\.|[^'\\\r\n])*'|`(?:\\[\s\S]|[^`\\])*`)/.source;
  const tokens = new RegExp(`${literals}|(?<keyword>\\b(?:${keywords[language]})\\b)|(?<number>\\b\\d+(?:\\.\\d+)?\\b)`, 'g');
  const fragment = document.createDocumentFragment();
  let cursor = 0;
  for (const match of source.matchAll(tokens)) {
    fragment.append(document.createTextNode(source.slice(cursor, match.index)));
    const type = Object.keys(match.groups).find(key => match.groups[key] !== undefined);
    fragment.append(node('span', match[0], 'token-' + type));
    cursor = match.index + match[0].length;
  }
  fragment.append(document.createTextNode(source.slice(cursor)));
  $('docs-code').replaceChildren(fragment);
}
async function loadExample() {
  const focused = document.activeElement === $('docs-language');
  const language = $('docs-language').value;
  const example = docsExamples[language];
  $('docs-language').disabled = true; $('copy-example').disabled = true; docsSource = '';
  $('docs-filename').textContent = example.file;
  $('docs-code').parentElement.setAttribute('aria-label', `Complete ${example.name} example`);
  $('docs-code').textContent = 'Loading example…';
  try {
    const response = await fetch(example.url);
    if (!response.ok) throw new Error('Example request failed');
    const source = await response.text();
    highlightCode(source, language);
    docsSource = source; $('copy-example').disabled = false;
  } catch {
    $('docs-code').textContent = 'Example unavailable.';
    throw new Error(`Could not load the ${example.name} example. Select another language or reload to try again.`);
  } finally {
    $('docs-language').disabled = false;
    if (focused && document.activeElement === document.body) $('docs-language').focus({ preventScroll: true });
  }
}
$('docs-language').addEventListener('change', () => action(null, loadExample));
$('logout').addEventListener('click', async () => {
  if (isDirty() && !await confirmDiscard()) return;
  action($('logout'), async () => { await api('/logout', 'POST'); unloading = true; window.removeEventListener('beforeunload', beforeUnload); location.reload(); });
});
$('auth-form').addEventListener('submit', event => {
  event.preventDefault(); action($('auth-submit'), async () => {
    await api(setupRequired ? '/setup' : '/login', 'POST', { username: $('username').value, password: $('password').value });
    $('password').value = ''; location.replace(route ? `/environments/${environment}/${currentScreen}` : '/');
  }, $('auth-error'));
});
async function boot() {
  if (location.pathname !== '/' && !route) throw new Error('Unknown environment or screen. Open /environments/development/flags or /environments/production/flags.');
  const status = await api('/status'); setupRequired = status.setup_required; $('loading').hidden = true;
  $('auth').hidden = status.authenticated; $('workspace').hidden = !status.authenticated; $('session').hidden = !status.authenticated; $('screens').hidden = !status.authenticated;
  if (!status.authenticated) {
    $('auth-title').textContent = setupRequired ? 'Create admin account' : 'Sign in';
    $('auth-description').textContent = setupRequired ? 'Set up your account to manage feature flags.' : 'Enter your account credentials.';
    $('auth-submit').textContent = setupRequired ? 'Create admin account' : 'Sign in';
    $('password-help').textContent = setupRequired ? 'Use at least 12 characters.' : '';
    $('password').minLength = setupRequired ? 12 : 1; $('password').autocomplete = setupRequired ? 'new-password' : 'current-password'; $('setup-note').hidden = !setupRequired;
    $('username').focus(); return;
  }
  const access = await api('/environments');
  currentUser = access.user;
  $('users-tab').hidden = !currentUser.is_admin;
  const allowed = access.environments;
  if (!allowed.length || (route && !allowed.includes(environment))) {
    $('workspace').hidden = true; $('screens').hidden = true; $('environment').closest('label').hidden = true; document.querySelector('.brand').href = '/';
    throw new Error(allowed.length ? 'You do not have access to this environment. Open the home page to choose an available environment.' : 'You do not have access to any environment. Ask an administrator to assign access.');
  }
  if (!route) { location.replace(`/environments/${allowed[0]}/flags`); return; }
  const selector = createDropdown(allowed.map(env => [env, env[0].toUpperCase() + env.slice(1)]), environment, 'environment');
  selector.id = 'environment'; selector.setAttribute('aria-label', 'Environment'); $('environment').replaceWith(selector);
  selector.addEventListener('change', async () => {
    const target = selector.value; selector.setValue(environment);
    if (target !== environment) await navigate(`/environments/${target}/${currentScreen}`);
  });
  const role = currentUser.permissions[environment];
  canEdit = currentUser.is_admin || ['editor', 'manager'].includes(role);
  canManage = currentUser.is_admin || role === 'manager';
  $('create-flag').hidden = currentScreen !== 'flags' || !canEdit;
  $('create-app').hidden = currentScreen !== 'apps' || !canManage;
  $('create-user').hidden = currentScreen !== 'users' || !currentUser.is_admin;
  if (currentScreen === 'users' && !currentUser.is_admin) {
    $('workspace').hidden = true;
    throw new Error('Only instance administrators can manage users.');
  }
  $('screen-title').textContent = $(`${currentScreen}-tab`).textContent; document.title = `Partly · ${$('screen-title').textContent} · ${environment}`;
  if (currentScreen === 'flags') {
    const [flags, result] = await Promise.all([api(`/environments/${environment}/flags`), api(`/environments/${environment}/usage`)]);
    for (const entry of result.usage) {
      if (!flagUsage.has(entry.flag_key)) flagUsage.set(entry.flag_key, []);
      flagUsage.get(entry.flag_key).push(entry);
    }
    for (const [key, flag] of Object.entries(flags)) $('flags').append(flagRow(key, flag)); updateFlagCount();
  } else if (currentScreen === 'apps') {
    const [flags, result] = await Promise.all([api(`/environments/${environment}/flags`), api(`/environments/${environment}/apps`)]);
    availableFlags = Object.keys(flags); result.applications.forEach(app => $('apps').append(appRow(app)));
    if (!result.applications.length) $('apps').append(node('p', 'No applications in this environment.', 'empty'));
  } else if (currentScreen === 'users') {
    const { users } = await api('/users');
    users.forEach(user => $('users').append(userRow(user)));
  } else if (currentScreen === 'audits') {
    await setupAudits();
  } else {
    await loadExample();
  }
  $(`${currentScreen}-tab`).scrollIntoView({ block: 'nearest', inline: 'nearest' });
  if (sessionStorage.getItem('partly-focus-tab')) { sessionStorage.removeItem('partly-focus-tab'); $(`${currentScreen}-tab`).focus(); }
}
action(null, boot).finally(() => { $('loading').hidden = true; });
