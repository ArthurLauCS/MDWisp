import { t } from '../i18n'
import { ACTIONS } from './registry'
import type { ActionDef } from './types'

/**
 * Binding resolution.
 *
 * A binding string is normalised once at module load into a canonical form
 * (`ctrl+shift+key`) so a keydown event can be matched with one map lookup
 * instead of walking every action and comparing each modifier.
 */

export interface ParsedKey {
  ctrl: boolean
  shift: boolean
  alt: boolean
  meta: boolean
  key: string
}

export function parseBinding(binding: string): ParsedKey {
  const parts = binding.split('+')
  const key = parts[parts.length - 1]
  const mods = parts.slice(0, -1).map((m) => m.toLowerCase())
  return {
    ctrl: mods.includes('ctrl'),
    shift: mods.includes('shift'),
    alt: mods.includes('alt'),
    meta: mods.includes('meta'),
    key: normaliseKey(key)
  }
}

/** `Digit1` and `1` are the same physical key; `ArrowUp` and `up` likewise. */
function normaliseKey(key: string): string {
  const aliases: Record<string, string> = {
    ' ': 'space', '\\': 'backslash', '|': 'backslash', ',': 'comma', '<': 'comma',
    ')': '0', '!': '1', '@': '2', '#': '3', '$': '4', '%': '5', '^': '6', '&': '7', '*': '8', '(': '9'
  }
  if (aliases[key]) return aliases[key]
  return key
    .replace(/^Digit/, '')
    .replace(/^Key/, '')
    .replace(/^Arrow/, '')
    .toLowerCase()
}

export function eventToKey(e: KeyboardEvent): ParsedKey {
  return {
    ctrl: e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    meta: e.metaKey,
    // Shift changes `key` to !/@/...; `code` still identifies Digit1/Digit2.
    key: normaliseKey(e.code || e.key)
  }
}

function serialise(k: ParsedKey): string {
  const out: string[] = []
  if (k.ctrl) out.push('ctrl')
  if (k.alt) out.push('alt')
  if (k.shift) out.push('shift')
  if (k.meta) out.push('meta')
  out.push(k.key)
  return out.join('+')
}

const index = new Map<string, ActionDef>()

for (const action of ACTIONS) {
  for (const binding of [action.key, ...(action.altKeys ?? [])]) {
    if (!binding) continue
    const combo = serialise(parseBinding(binding))
    // First registration wins; a duplicate is a bug worth seeing in the console.
    if (index.has(combo)) {
      console.warn(`duplicate binding ${combo}: ${action.id} vs ${index.get(combo)?.id}`)
      continue
    }
    index.set(combo, action)
  }
}

export function resolveBinding(e: KeyboardEvent): ActionDef | null {
  // A bare `Shift+Enter` never reaches a plain keydown as `enter` with shift
  // already handled — checking the serialised form covers both.
  return index.get(serialise(eventToKey(e))) ?? null
}

export function lookup(binding: string): ActionDef | null {
  return index.get(serialise(parseBinding(binding))) ?? null
}

/** Conflicts between two bindings that would fight over the same keystroke. */
export function conflicts(): { combo: string; ids: string[] }[] {
  const seen = new Map<string, string[]>()
  for (const a of ACTIONS) {
    for (const binding of [a.key, ...(a.altKeys ?? [])]) {
      if (!binding) continue
      const combo = serialise(parseBinding(binding))
      seen.set(combo, [...(seen.get(combo) ?? []), a.id])
    }
  }
  return [...seen.entries()]
    .filter(([, ids]) => ids.length > 1)
    .map(([combo, ids]) => ({ combo, ids }))
}

/** Human-readable form for menus: `Ctrl+Shift+Digit1` → `Ctrl+Shift+1`. */
export function prettyKey(binding: string): string {
  return binding
    .replace(/Digit(\d)/g, '$1')
    .replace(/ArrowUp/g, '↑')
    .replace(/ArrowDown/g, '↓')
    .replace(/ArrowLeft/g, '←')
    .replace(/ArrowRight/g, '→')
    .replace(/Backspace/g, '⌫')
    .replace(/Enter/g, '↵')
    .replace(/Space/g, t('空格'))
    .replace(/Escape/g, 'Esc')
    .replace(/Backslash/g, '\\')
    .replace(/Slash/g, '/')
    .replace(/Equal/g, '=')
    .replace(/Minus/g, '-')
    .replace(/Ctrl/g, 'Ctrl')
    .replace(/Meta/g, 'Cmd')
    .replace(/\+/g, ' + ')
}
