// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

"use strict";

const dropdownMenu = document.createElement('div');
dropdownMenu.id = 'dropdown-options';
dropdownMenu.className = 'dropdown-menu';
dropdownMenu.setAttribute('role', 'listbox');
dropdownMenu.setAttribute('popover', 'manual');
document.body.append(dropdownMenu);
let activeDropdown;
let dropdownId = 0;

function closeDropdown() {
  if (dropdownMenu.matches(':popover-open')) dropdownMenu.hidePopover();
}
dropdownMenu.addEventListener('beforetoggle', event => {
  if (event.newState === 'closed' && activeDropdown) {
    activeDropdown.control.setAttribute('aria-expanded', 'false');
    activeDropdown.control.removeAttribute('aria-activedescendant');
    activeDropdown = undefined;
  }
});

function highlightDropdown(index) {
  const state = activeDropdown;
  state.index = Math.max(0, Math.min(index, state.options.length - 1));
  [...dropdownMenu.children].forEach((option, i) => { option.dataset.active = String(i === state.index); });
  const option = dropdownMenu.children[state.index];
  state.control.setAttribute('aria-activedescendant', option.id);
  option.scrollIntoView({ block: 'nearest' });
}

function commitDropdown(index = activeDropdown.index) {
  const { control, options } = activeDropdown;
  const value = options[index][0];
  closeDropdown();
  if (control.value === value) return;
  control.setValue(value);
  control.dispatchEvent(new Event('input', { bubbles: true }));
  control.dispatchEvent(new Event('change', { bubbles: true }));
}

function openDropdown(control, options) {
  closeDropdown();
  if (control.disabled) return;
  control.focus({ preventScroll: true });
  const index = options.findIndex(([value]) => value === control.value);
  activeDropdown = { control, options, index, search: '', typedAt: 0 };
  // A popup must remain inside its modal's DOM subtree to avoid becoming inert.
  (control.closest('dialog') || document.body).append(dropdownMenu);
  dropdownMenu.replaceChildren(...options.map(([value, text], i) => {
    const option = document.createElement('div');
    option.id = `dropdown-option-${i}`;
    option.className = 'dropdown-option';
    option.setAttribute('role', 'option');
    option.setAttribute('aria-label', text);
    option.setAttribute('aria-selected', String(value === control.value));
    option.dataset.value = value;
    option.dataset.index = i;
    option.textContent = text;
    return option;
  }));
  dropdownMenu.setAttribute('aria-labelledby', control.id);
  const rect = control.getBoundingClientRect();
  dropdownMenu.style.minWidth = `${rect.width}px`;
  dropdownMenu.style.maxHeight = '280px';
  dropdownMenu.style.left = '0px';
  dropdownMenu.style.top = '0px';
  dropdownMenu.showPopover();
  const below = innerHeight - rect.bottom - 8;
  const above = rect.top - 8;
  const upwards = below < Math.min(dropdownMenu.scrollHeight, 180) && above > below;
  dropdownMenu.style.maxHeight = `${Math.max(0, Math.min(280, (upwards ? above : below) - 4))}px`;
  const menuRect = dropdownMenu.getBoundingClientRect();
  dropdownMenu.style.left = `${Math.max(8, Math.min(rect.left, innerWidth - menuRect.width - 8))}px`;
  dropdownMenu.style.top = `${upwards ? rect.top - menuRect.height - 4 : rect.bottom + 4}px`;
  control.setAttribute('aria-expanded', 'true');
  highlightDropdown(index);
}

function createDropdown(options, value, name) {
  const control = document.createElement('button');
  control.type = 'button';
  control.name = name;
  control.id = `dropdown-${++dropdownId}`;
  control.className = 'dropdown';
  control.setAttribute('role', 'combobox');
  control.setAttribute('aria-haspopup', 'listbox');
  control.setAttribute('aria-controls', dropdownMenu.id);
  control.setAttribute('aria-expanded', 'false');
  control.setValue = value => {
    control.value = value;
    control.textContent = options.find(([key]) => key === value)[1];
  };
  control.setValue(value);
  control.addEventListener('click', () => {
    if (activeDropdown?.control === control) closeDropdown();
    else openDropdown(control, options);
  });
  control.addEventListener('keydown', event => {
    const opened = activeDropdown?.control === control;
    if (event.key === 'Escape' && opened) {
      event.preventDefault(); event.stopPropagation(); closeDropdown(); return;
    }
    if (event.key === 'Tab') {
      if (opened) commitDropdown();
      return;
    }
    if (['Enter', ' ', 'ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault(); event.stopPropagation();
      if (opened && ['Enter', ' '].includes(event.key)) { commitDropdown(); return; }
      if (!opened) openDropdown(control, options);
      if (event.key === 'Home') highlightDropdown(0);
      else if (event.key === 'End') highlightDropdown(options.length - 1);
      else if (opened && event.key === 'ArrowDown') highlightDropdown(activeDropdown.index + 1);
      else if (opened && event.key === 'ArrowUp') highlightDropdown(activeDropdown.index - 1);
      return;
    }
    if (event.key.length !== 1 || event.ctrlKey || event.metaKey || event.altKey) return;
    event.preventDefault();
    if (!opened) openDropdown(control, options);
    const state = activeDropdown;
    const now = performance.now();
    state.search = (now - state.typedAt < 700 ? state.search : '') + event.key.toLowerCase();
    state.typedAt = now;
    const query = [...state.search].every(char => char === state.search[0]) ? state.search[0] : state.search;
    const start = state.index + (query.length === 1 ? 1 : 0);
    for (let i = 0; i < options.length; i++) {
      const index = (start + i) % options.length;
      if (options[index][1].toLowerCase().startsWith(query)) { highlightDropdown(index); break; }
    }
  });
  return control;
}

dropdownMenu.addEventListener('pointerdown', event => { event.preventDefault(); });
dropdownMenu.addEventListener('pointermove', event => {
  const option = event.target.closest('[role=option]');
  if (option && activeDropdown) highlightDropdown(Number(option.dataset.index));
});
dropdownMenu.addEventListener('click', event => {
  const option = event.target.closest('[role=option]');
  if (option && activeDropdown) commitDropdown(Number(option.dataset.index));
});
document.addEventListener('pointerdown', event => {
  if (activeDropdown && !activeDropdown.control.contains(event.target) && !dropdownMenu.contains(event.target)) closeDropdown();
}, true);
document.addEventListener('focusin', event => {
  if (activeDropdown && event.target !== activeDropdown.control) closeDropdown();
});
document.addEventListener('scroll', event => {
  if (event.target !== dropdownMenu) closeDropdown();
}, true);
window.addEventListener('resize', closeDropdown);
