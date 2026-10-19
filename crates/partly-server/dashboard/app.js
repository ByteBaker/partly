"use strict";
const $ = id => document.getElementById(id);
const route = location.pathname.match(/^\/environments\/(development|production)\/(flags|keys|docs)\/?$/);
const environment = route?.[1] || 'development';
const screen = route?.[2] || 'flags';
const trackers = new Map();
let setupRequired = false;
let unloading = false;
let modalEditor;
let nextId = 0;
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
  const element = node('label'); element.append(node('span', text), input); return element;
}
function select(options, value, name) {
  const element = node('select'); element.name = name;
  for (const [key, text] of options) { const option = node('option', text); option.value = key; element.append(option); }
  element.value = value; return element;
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
async function api(path, method = 'GET', body) {
  const response = await fetch(`/api${path}`, {
    method, credentials: 'same-origin',
    headers: { 'X-Partly-Request': 'dashboard', ...(body === undefined ? {} : { 'Content-Type': 'application/json' }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    let message = await response.text();
    try { message = JSON.parse(message).error || message; } catch { /* Validation may return plain text. */ }
    if (response.status === 401) message = 'Your session has ended. Save a copy of your edits, then reload to sign in.';
    throw new Error(message || `Request failed (${response.status})`);
  }
  return response.status === 204 ? undefined : response.json();
}
async function action(trigger, operation, errorArea) {
  const keyRow = trigger?.closest('.key-row');
  const form = trigger?.closest('form') || keyRow?.querySelector('form');
  const state = trackers.get(form);
  if (state?.pending) return;
  const scope = keyRow || form;
  errorArea ||= scope?.querySelector('.item-error') || $('error');
  errorArea.hidden = true;
  $('notice').textContent = '';
  const focused = document.activeElement;
  const controls = scope ? [...scope.querySelectorAll('input, select, button')].map(element => [element, element.disabled]) : [];
  if (form === $('new-flag')) controls.push([$('close-flag'), $('close-flag').disabled]);
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
    if (document.activeElement === document.body && scope?.isConnected) {
      const target = focused?.isConnected && !focused.disabled
        ? focused
        : scope.querySelector('input:not([disabled]), select:not([disabled]), button:not([disabled])');
      target?.focus();
    }
  }
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
    if (state.indicator) state.indicator.hidden = !dirty;
  }
  window.removeEventListener('beforeunload', beforeUnload);
  if (anyDirty && !unloading) window.addEventListener('beforeunload', beforeUnload);
}
function track(element, read, save, indicator) {
  trackers.set(element, { read, baseline: JSON.stringify(read()), save, indicator });
  element.dataset.dirty = 'false';
  if (save) save.disabled = true;
  if (indicator) indicator.hidden = true;
}
function untrack(element) { trackers.delete(element); updateDirty(); }
function markSaved(element) {
  const state = trackers.get(element); state.baseline = JSON.stringify(state.read()); updateDirty();
}
document.addEventListener('input', updateDirty);
document.addEventListener('change', updateDirty);
function confirmDiscard() {
  const dialog = $('discard-dialog'); dialog.showModal();
  return new Promise(resolve => {
    const finish = answer => { dialog.close(); $('keep-editing').onclick = null; $('discard-changes').onclick = null; dialog.oncancel = null; resolve(answer); };
    $('keep-editing').onclick = () => finish(false);
    $('discard-changes').onclick = () => finish(true);
    dialog.oncancel = event => { event.preventDefault(); finish(false); };
  });
}
async function navigate(path, focusTab = false) {
  if (isDirty() && !await confirmDiscard()) return false;
  unloading = true; window.removeEventListener('beforeunload', beforeUnload);
  if (focusTab) sessionStorage.setItem('partly-focus-tab', 'yes');
  location.assign(path); return true;
}

function createEditor(flag, name) {
  const area = node('div', '', 'rollout-editor');
  const mode = select([
    ['enabled', 'Enabled for everyone'], ['percentage', 'Percentage of users'], ['custom', 'Custom rules'], ['disabled', 'Disabled for everyone'],
  ], flag.mode, 'mode'); mode.setAttribute('aria-label', `Rollout mode for ${name}`);
  const modeLabel = label('Rollout mode', mode); modeLabel.className = 'mode-label'; area.append(modeLabel);
  const percentageArea = node('div', '', 'percentage-editor');
  const percentage = input('percentage', flag.percentage ?? 50, 'number'); percentage.min = '0'; percentage.max = '100'; percentage.step = '1'; percentage.setAttribute('aria-label', `Percentage for ${name}`);
  percentageArea.append(label('Rollout', percentage), node('span', '%'), node('p', 'Stable selection using context.id.')); area.append(percentageArea);
  const custom = node('div', '', 'custom-editor');
  const customHeading = node('div', '', 'custom-heading');
  const explanation = node('div'); explanation.append(node('h2', 'Match your context'), node('p', 'Match any group. All conditions in a group must match.'));
  const matchAction = select([['enable', 'Enable matches; disable everyone else'], ['disable', 'Disable matches; enable everyone else']], flag.action || 'enable', 'action');
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
      const operator = select(Object.entries(ops).map(([key, [text, type]]) => [key, `${text} (${type[0].toUpperCase()}${type.slice(1)})`]), condition.op, 'op');
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
  custom.append(node('p', 'Context keys refer to your application data. Values stay in your application; only these rules are downloaded.', 'rule-note')); area.append(custom);
  function updateMode() {
    percentageArea.hidden = mode.value !== 'percentage'; custom.hidden = mode.value !== 'custom';
    percentage.disabled = percentageArea.hidden;
    custom.querySelectorAll('input, select').forEach(element => { element.disabled = custom.hidden; });
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
function flagRow(key, flag) {
  const form = node('form', '', 'flag-row'); form.dataset.key = key;
  const heading = node('div', '', 'flag-heading'); const identity = node('div', '', 'flag-identity');
  const indicator = node('span', 'Unsaved', 'draft-indicator'); indicator.hidden = true;
  identity.append(node('code', key, 'flag-name'), indicator);
  const actions = node('div', '', 'row-actions');
  const save = node('button', 'Save'); save.type = 'submit'; save.setAttribute('aria-label', `Save ${key}`);
  const remove = button('Delete', 'quiet danger', () => {
    if (!confirm(`Delete ${key} from ${environment}? SDK checks for this flag will raise an error after refresh.`)) return;
    action(remove, async () => { await api(`/environments/${environment}/flags/${key}`, 'DELETE'); untrack(form); form.remove(); updateFlagCount(); $('notice').textContent = `Deleted ${key}.`; });
  }); remove.setAttribute('aria-label', `Delete ${key}`);
  actions.append(save, remove); heading.append(identity, actions); form.append(heading);
  const editor = createEditor(flag, key); form.append(editor.area);
  const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert'); form.append(error);
  track(form, editor.read, save, indicator);
  form.addEventListener('submit', event => {
    event.preventDefault();
    action(save, async () => {
      const saved = await api(`/environments/${environment}/flags/${key}`, 'PUT', editor.value());
      const replacement = flagRow(saved.key, saved);
      untrack(form);
      form.replaceWith(replacement);
      if (document.activeElement === document.body) replacement.querySelector('[name=mode]').focus();
      $('notice').textContent = `Saved ${saved.key} in ${environment}.`;
    });
  });
  return form;
}
function addKey({ id, name, secret }) {
  $('keys').querySelector('.empty')?.remove();
  const row = node('div', '', 'key-row');
  const rename = node('form', '', 'key-name-form');
  const nameInput = input('name', name); nameInput.maxLength = 64;
  const saveName = node('button', 'Save name'); saveName.type = 'submit';
  const value = input('secret', secret, 'password'); value.readOnly = true; value.autocomplete = 'off';
  const copy = button('Copy');
  function updateNames() {
    nameInput.setAttribute('aria-label', `Name for ${name}`); saveName.setAttribute('aria-label', `Save name for ${name}`);
    value.setAttribute('aria-label', `SDK key for ${name}`); copy.setAttribute('aria-label', `Copy SDK key for ${name}`);
  }
  updateNames(); rename.append(label('Key name', nameInput), saveName); track(rename, () => nameInput.value, saveName);
  rename.addEventListener('submit', event => {
    event.preventDefault();
    action(saveName, async () => {
      await api(`/keys/${id}`, 'PUT', { name: nameInput.value }); name = nameInput.value.trim(); nameInput.value = name;
      updateNames(); markSaved(rename); $('notice').textContent = 'Key name saved.';
    });
  });
  const controls = node('div', '', 'key-controls'); const secretLabel = label('SDK key', value); secretLabel.className = 'key-secret';
  const reveal = button('Reveal', '', () => { const visible = value.type === 'password'; value.type = visible ? 'text' : 'password'; reveal.textContent = visible ? 'Hide' : 'Reveal'; reveal.setAttribute('aria-pressed', String(visible)); }); reveal.setAttribute('aria-pressed', 'false');
  let copyReset;
  copy.addEventListener('click', () => action(copy, async () => {
    clearTimeout(copyReset); copy.textContent = 'Copy';
    try { await navigator.clipboard.writeText(secret); } catch { throw new Error('Could not copy. Allow clipboard access or use Reveal to copy the key manually.'); }
    copy.textContent = 'Copied'; copyReset = setTimeout(() => { copy.textContent = 'Copy'; }, 2000); $('notice').textContent = 'SDK key copied.';
  }));
  const revoke = button('Revoke', 'quiet danger', () => {
    if (!confirm('Revoke this key? Connected SDKs keep their last policy but cannot download updates.')) return;
    action(revoke, async () => { await api(`/keys/${id}`, 'DELETE'); untrack(rename); row.remove(); keyEmptyState(); $('notice').textContent = 'Key revoked.'; });
  });
  const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert');
  controls.append(secretLabel, copy, reveal, revoke); row.append(rename, controls, error); $('keys').append(row);
}
function keyEmptyState() {
  if ($('keys').querySelector('.key-row')) return;
  const empty = node('div', '', 'empty'); empty.append(node('h3', 'No SDK keys yet'), node('p', 'Generate a key to connect your Python application.')); $('keys').replaceChildren(empty);
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
    $('flags').querySelector('.empty')?.remove(); $('flags').append(flagRow(saved.key, saved)); updateFlagCount();
    untrack($('new-flag')); $('flag-dialog').close(); $('notice').textContent = `Created ${saved.key} in ${environment}.`;
  }, $('flag-dialog-error'));
});
$('new-key').addEventListener('submit', event => {
  event.preventDefault(); action($('generate-key'), async () => {
    addKey(await api(`/environments/${environment}/keys`, 'POST', { name: $('key-name').value }));
    $('new-key').reset(); markSaved($('new-key')); $('notice').textContent = 'Key created. Select Copy to use it in your application.';
  });
});
$('environment').value = environment;
document.querySelector('.brand').href = `/environments/${environment}/flags`;
$('environment').addEventListener('change', async () => {
  const target = $('environment').value; $('environment').value = environment;
  if (target !== environment) await navigate(`/environments/${target}/${screen}`);
});
const tabs = [...$('screens').querySelectorAll('[role=tab]')];
for (const [index, tab] of tabs.entries()) {
  const active = tab.id === `${screen}-tab`; tab.setAttribute('aria-selected', String(active)); tab.tabIndex = active ? 0 : -1; $(tab.getAttribute('aria-controls')).hidden = !active;
  const go = focus => { if (!active) return navigate(`/environments/${environment}/${tab.id.split('-')[0]}`, focus); };
  tab.addEventListener('click', () => go(false));
  tab.addEventListener('keydown', event => {
    let next;
    if (event.key === 'ArrowRight') next = (index + 1) % tabs.length;
    else if (event.key === 'ArrowLeft') next = (index + tabs.length - 1) % tabs.length;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = tabs.length - 1;
    else return;
    event.preventDefault();
    if (next === index) tab.focus(); else navigate(`/environments/${environment}/${tabs[next].id.split('-')[0]}`, true);
  });
}
$('copy-example').addEventListener('click', () => action($('copy-example'), async () => {
  try { await navigator.clipboard.writeText($('docs-code').textContent); } catch { throw new Error('Could not copy. Select the example and copy it manually.'); }
  $('notice').textContent = 'Python example copied.';
}));
$('logout').addEventListener('click', async () => {
  if (isDirty() && !await confirmDiscard()) return;
  action($('logout'), async () => { await api('/logout', 'POST'); unloading = true; window.removeEventListener('beforeunload', beforeUnload); location.reload(); });
});
$('auth-form').addEventListener('submit', event => {
  event.preventDefault(); action($('auth-submit'), async () => {
    await api(setupRequired ? '/setup' : '/login', 'POST', { username: $('username').value, password: $('password').value });
    $('password').value = ''; location.replace(`/environments/${environment}/${screen}`);
  });
});
async function boot() {
  if (location.pathname !== '/' && !route) throw new Error('Unknown environment or screen. Open /environments/development/flags or /environments/production/flags.');
  const status = await api('/status'); setupRequired = status.setup_required; $('loading').hidden = true;
  if (status.authenticated && !route) { location.replace('/environments/development/flags'); return; }
  $('auth').hidden = status.authenticated; $('workspace').hidden = !status.authenticated; $('session').hidden = !status.authenticated; $('screens').hidden = !status.authenticated;
  if (!status.authenticated) {
    $('auth-title').textContent = setupRequired ? 'Make yourself at home.' : 'Welcome back.';
    $('auth-description').textContent = setupRequired ? 'Create your admin account to start managing flags.' : 'Sign in to your local control plane.';
    $('auth-submit').textContent = setupRequired ? 'Create admin account' : 'Sign in';
    $('password').minLength = setupRequired ? 12 : 1; $('password').autocomplete = setupRequired ? 'new-password' : 'current-password'; $('setup-note').hidden = !setupRequired;
    $('username').focus(); return;
  }
  $('workspace-environment').textContent = `${environment.toUpperCase()} / DEFAULT PROJECT`;
  $('production-note').hidden = environment !== 'production'; $('create-flag').hidden = screen !== 'flags';
  $('screen-title').textContent = $(`${screen}-tab`).textContent; document.title = `Partly · ${$('screen-title').textContent} · ${environment}`;
  $('screen-description').textContent = { flags: 'Release at your pace. Keep control of who sees what.', keys: 'Name and manage access for your applications.', docs: 'A small SDK. Everything you need to make your first check.' }[screen];
  if (screen === 'flags') {
    const flags = await api(`/environments/${environment}/flags`);
    for (const [key, flag] of Object.entries(flags)) $('flags').append(flagRow(key, flag)); updateFlagCount();
  } else if (screen === 'keys') {
    const result = await api(`/environments/${environment}/keys`); result.keys.forEach(addKey); keyEmptyState(); track($('new-key'), () => $('key-name').value);
  } else {
    $('docs-url').textContent = `export PARTLY_URL=${location.origin}`;
    const response = await fetch('/docs/python.py'); if (!response.ok) throw new Error('Could not load the Python example. Reload to try again.');
    $('docs-code').textContent = await response.text(); $('copy-example').disabled = false;
  }
  if (sessionStorage.getItem('partly-focus-tab')) { sessionStorage.removeItem('partly-focus-tab'); $(`${screen}-tab`).focus(); }
}
action(null, boot).finally(() => { $('loading').hidden = true; });
