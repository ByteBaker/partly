// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

/* global $, node, input, label, button, createDropdown, createEditor, environment, canManage,
   api, action, track, untrack, trackers, isDirty, confirmDiscard, confirmDialog, showNotice, updateDirty */
/* exported resultTypes, createFlagEditor, setupTypes, typeSummary, resultSummary */
"use strict";
let resultTypes = {};
let resultSequence = 0;
const builtInTypes = [['boolean', 'Boolean'], ['string', 'String'], ['integer', 'Integer'], ['float', 'Float']];

function defaultResult(type) {
  if (type === 'boolean') return false;
  if (type === 'string') return '';
  if (type === 'integer' || type === 'float') return 0;
  const definition = resultTypes[type];
  return definition.kind === 'enum' ? definition.values[0]
    : Object.fromEntries(Object.entries(definition.fields).map(([key, type]) => [key, defaultResult(type)]));
}
function resultInput(type, value, title) {
  const area = node('div', '', 'result-input');
  if (resultTypes[type]?.kind === 'record') {
    const fields = Object.entries(resultTypes[type].fields).map(([key, type]) => [key, resultInput(type, value[key], key)]);
    area.append(...fields.map(([, field]) => field.area));
    return { area, read: () => Object.fromEntries(fields.map(([key, field]) => [key, field.read()])), value: () => Object.fromEntries(fields.map(([key, field]) => [key, field.value()])) };
  }
  if (type === 'boolean') {
    const group = node('fieldset', '', 'bool-values'); group.append(node('legend', title));
    const name = `result-${++resultSequence}`;
    const choices = [true, false].map(value => { const radio = input(name, String(value), 'radio'); group.append(label(value ? 'True' : 'False', radio)); return radio; });
    choices[value ? 0 : 1].checked = true; area.append(group);
    return { area, read: () => choices[0].checked, value: () => choices[0].checked };
  }
  const control = resultTypes[type]?.kind === 'enum'
    ? createDropdown(resultTypes[type].values.map(value => [value, value]), value, title)
    : type === 'string' ? node('textarea') : input(title, value, 'number');
  if (type === 'string') { control.value = value; control.rows = 2; control.name = title; }
  control.setAttribute('aria-label', title);
  if (type === 'integer' || type === 'float') control.step = type === 'integer' ? '1' : 'any';
  if (type === 'string') control.required = false;
  const initialText = control.value;
  area.append(label(`${title} (${type})`, control));
  return { area, read: () => control.value, value: () => {
    if (type === 'integer' || type === 'float') {
      const value = Number(control.value);
      if (control.value === '' || !Number.isFinite(value)) throw new Error(`${title}: enter a finite number.`);
      if (type === 'integer' && !Number.isSafeInteger(value)) throw new Error(`${title}: enter a whole number between −9007199254740991 and 9007199254740991.`);
      return value;
    }
    const result = type === 'string' && control.value === initialText ? value : control.value;
    if (new TextEncoder().encode(result).length > 4096) throw new Error(`${title}: use at most 4096 UTF-8 bytes.`);
    return result;
  } };
}

function createFlagEditor(flag, name, creating = false) {
  const area = node('div', '', 'typed-editor');
  let type = flag.result_type || 'boolean';
  if (!creating && type === 'boolean') return createEditor(flag, name);
  const choices = [...builtInTypes, ...Object.entries(resultTypes).map(([name, definition]) => [name, `${name} (${definition.kind})`])];
  if (creating) {
    const select = createDropdown(choices, type, 'result_type'); select.setAttribute('aria-label', 'Result type');
    area.append(label('Result type', select));
    select.addEventListener('change', () => { type = select.value; render({ mode: 'disabled' }); updateDirty(); });
  }
  const content = node('div'); area.append(content);
  let rollout; let selected; let other;
  function render(flag) {
    content.replaceChildren();
    rollout = createEditor(flag.rollout || flag, name, type !== 'boolean'); content.append(rollout.area);
    if (type !== 'boolean') {
      const results = node('div', '', 'result-values');
      selected = resultInput(type, flag.selected ?? defaultResult(type), 'Selected value');
      other = resultInput(type, flag.other ?? defaultResult(type), 'Other value');
      for (const [title, editor] of [['Selected', selected], ['Otherwise', other]]) {
        const panel = node('section', '', 'result-panel'); panel.append(node('h3', title), editor.area); results.append(panel);
      }
      content.append(node('p', 'The rollout selects which value to return. Both values must match the result type.', 'small muted'), results);
    }
  }
  render(flag);
  return { area,
    read: () => type === 'boolean' ? rollout.read() : { result_type: type, selected: selected.read(), other: other.read(), rollout: rollout.read() },
    value: () => type === 'boolean' ? rollout.value() : { mode: 'typed', result_type: type, selected: selected.value(), other: other.value(), rollout: rollout.value() },
  };
}

function typeSummary(definition) {
  const content = node('div', '', 'type-definition');
  if (definition.kind === 'enum') {
    const choices = node('ul', '', 'type-choices');
    definition.values.forEach(value => choices.append(node('li', value)));
    content.append(choices);
  } else {
    const fields = node('dl', '', 'type-fields');
    for (const [key, type] of Object.entries(definition.fields)) fields.append(node('dt', key), node('dd', type));
    content.append(fields);
  }
  return content;
}
function resultSummary(value) {
  if (value !== null && typeof value === 'object') {
    const fields = node('dl', '', 'type-fields');
    for (const [key, field] of Object.entries(value)) fields.append(node('dt', key), node('dd', String(field)));
    return fields;
  }
  return node('p', typeof value === 'string' ? `“${value}”` : String(value), 'result-value');
}

async function setupTypes() {
  const flags = await api(`/environments/${environment}/flags`);
  function render() {
    $('types').replaceChildren();
    for (const [name, definition] of Object.entries(resultTypes)) {
      const card = node('article', '', 'type-card'); card.dataset.type = name; const head = node('div', '', 'flag-heading');
      const identity = node('div', '', 'flag-identity'); identity.append(node('h2', name), node('span', definition.kind === 'enum' ? 'Enum' : 'Record', 'type-pill')); head.append(identity);
      const remove = button('Delete', 'quiet danger', async () => {
        await action(remove, async () => {
          $('delete-type-title').textContent = `Delete ${name}?`;
          if (!await confirmDialog($('delete-type-dialog'), $('cancel-type-delete'), $('confirm-type-delete'))) return;
          await api(`/environments/${environment}/types/${name}`, 'DELETE'); delete resultTypes[name]; render(); showNotice('Deleted', $('create-type'));
        }, error);
        // Type cards have no form to restore focus after the disabled dialog opener.
        if (document.activeElement === document.body || document.activeElement.closest('dialog:not([open])')) {
          (remove.isConnected ? remove : $('create-type')).focus();
        }
      });
      remove.setAttribute('aria-label', `Delete ${name}`); remove.hidden = !canManage; head.append(remove);
      const error = node('p', '', 'notice error item-error'); error.hidden = true; error.setAttribute('role', 'alert');
      const references = [...Object.entries(flags).filter(([, flag]) => flag.result_type === name).map(([key]) => key),
        ...Object.entries(resultTypes).filter(([, type]) => type.kind === 'record' && Object.values(type.fields).includes(name)).map(([key]) => key)];
      remove.disabled = references.length > 0;
      card.append(head, typeSummary(definition), node('p', references.length ? `Used by: ${references.join(', ')}` : 'No flags or types use this type.', 'type-footer'), error);
      $('types').append(card);
    }
    if (!Object.keys(resultTypes).length) $('types').append(node('p', 'No named types in this environment.', 'empty'));
  }
  render();
  const form = $('new-type'); const fields = $('type-fields'); let rows = [];
  const kind = createDropdown([['enum', 'Enum'], ['record', 'Record']], 'enum', 'kind'); kind.id = 'type-kind'; kind.setAttribute('aria-label', 'Type kind'); $('type-kind').replaceWith(kind);
  function setKind() { $('type-enum').hidden = kind.value !== 'enum'; $('type-record').hidden = kind.value !== 'record'; $('type-choices').disabled = kind.value !== 'enum'; fields.querySelectorAll('input, button').forEach(control => { control.disabled = kind.value !== 'record'; }); }
  kind.addEventListener('change', setKind);
  function addField() {
    const row = node('div', '', 'type-field-row'); const key = input('field'); key.maxLength = 64; key.pattern = '[A-Za-z][A-Za-z0-9_]{0,63}';
    const types = [...builtInTypes, ...Object.entries(resultTypes).filter(([, type]) => type.kind === 'enum').map(([name]) => [name, `${name} (enum)`])];
    const type = createDropdown(types, 'string', 'field_type'); type.setAttribute('aria-label', 'Field type');
    const item = { key, type, row };
    row.append(label('Field name', key), label('Field type', type), button('Remove', 'quiet', () => { rows = rows.filter(value => value !== item); row.remove(); updateDirty(); }));
    rows.push(item); fields.append(row); return key;
  }
  $('add-type-field').onclick = () => { if (rows.length < 32) addField().focus(); updateDirty(); };
  function draft() { return { name: $('type-name').value, kind: kind.value, choices: $('type-choices').value, fields: rows.map(({ key, type }) => [key.value, type.value]) }; }
  $('create-type').onclick = () => {
    form.reset(); kind.setValue('enum'); rows = []; fields.replaceChildren(); addField(); setKind();
    $('type-dialog-context').textContent = `Environment: ${environment}. Names and definitions cannot be changed after creation.`;
    $('type-dialog-error').hidden = true; track(form, draft); $('type-dialog').showModal(); $('type-name').focus();
  };
  async function close() { if (trackers.get(form)?.pending || (isDirty(form) && !await confirmDiscard())) return; untrack(form); $('type-dialog').close(); }
  $('cancel-type').onclick = close; $('type-dialog').oncancel = event => { event.preventDefault(); close(); };
  form.onsubmit = event => {
    event.preventDefault(); action(event.submitter, async () => {
      const current = draft();
      if (current.kind === 'record' && new Set(current.fields.map(([name]) => name)).size !== rows.length) throw new Error('Each field needs a unique name.');
      const definition = current.kind === 'enum' ? { kind: 'enum', values: current.choices.split('\n') } : { kind: 'record', fields: Object.fromEntries(current.fields) };
      const saved = await api(`/environments/${environment}/types`, 'POST', { name: current.name, definition });
      resultTypes[saved.name] = saved.definition; untrack(form); $('type-dialog').close(); render(); showNotice('Created', $('types').querySelector(`[data-type="${saved.name}"]`));
    }, $('type-dialog-error'));
  };
}
