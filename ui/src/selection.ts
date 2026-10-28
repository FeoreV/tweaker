import selectionCatalog from '../../config/selection-catalog.v1.json';

export const selectionFamilies = ['apps', 'tweaks', 'toggles', 'features', 'appx'] as const;
export type SelectionFamily = typeof selectionFamilies[number];
export type SelectionState = Record<SelectionFamily, string[]>;
export type PresetScope = '**' | 'WPFTweak*' | 'WPFInstall*' | 'WPFToggle*' | 'WPFFeature*' | 'WPFAppx*';

type SelectionCatalog = {
  categories: Record<SelectionFamily, string[]>;
  named_presets: Record<string, string[]>;
};

const catalog = selectionCatalog as SelectionCatalog;
const knownByFamily = Object.fromEntries(selectionFamilies.map((family) => [
  family,
  new Map(catalog.categories[family].map((id) => [id.toLowerCase(), id])),
])) as Record<SelectionFamily, Map<string, string>>;
const knownIds = new Map(selectionFamilies.flatMap((family) => [...knownByFamily[family]]));

export const emptySelection = (): SelectionState => ({
  apps: [], tweaks: [], toggles: [], features: [], appx: [],
});

export function familyForId(id: string): SelectionFamily | undefined {
  const found = knownIds.get(id.toLowerCase());
  if (!found) return undefined;
  return selectionFamilies.find((family) => knownByFamily[family].has(found.toLowerCase()));
}

export function toggleSelection(state: SelectionState, family: SelectionFamily, id: string): SelectionState {
  const current = state[family];
  if (current.some((selected) => selected.toLowerCase() === id.toLowerCase())) {
    return { ...state, [family]: current.filter((selected) => selected.toLowerCase() !== id.toLowerCase()) };
  }
  const canonical = knownByFamily[family].get(id.toLowerCase());
  return canonical ? { ...state, [family]: [...current, canonical] } : state;
}

export function selectionKeys(state: SelectionState): string[] {
  return selectionFamilies.flatMap((family) => state[family]);
}

export function encodeSelection(state: SelectionState): string | undefined {
  const keys = selectionKeys(state);
  if (keys.length === 0) return undefined;
  return JSON.stringify(keys.length === 1 ? keys[0] : keys);
}

export type SelectionImportResult =
  | { kind: 'replace'; selection: SelectionState; skipped: string[]; legacy: boolean }
  | { kind: 'noop'; skipped: string[]; reason: string; legacy: boolean };

export function importSelectionJson(text: string): SelectionImportResult {
  if (new TextEncoder().encode(text).length > 1024 * 1024) throw new Error('Selection JSON exceeds the 1 MiB limit.');
  const parsed: unknown = JSON.parse(text);
  if (parsed === '') return { kind: 'noop', skipped: [], reason: 'Empty selection; existing preview selection kept.', legacy: false };

  if (parsed !== null && typeof parsed === 'object' && !Array.isArray(parsed)) {
    const entries = Object.entries(parsed as Record<string, unknown>);
    const hasInstall = entries.some(([key]) => key.toLowerCase() === 'install');
    const hasWpfInstall = entries.some(([key]) => key.toLowerCase() === 'wpfinstall');
    if (hasInstall && hasWpfInstall) return importLegacy(entries);
  }

  const values = typeof parsed === 'string' ? [parsed] : parsed;
  if (!Array.isArray(values) || !values.every((value): value is string => typeof value === 'string')) {
    throw new Error('Expected one selection string or a flat array of strings.');
  }
  if (values.length === 0) return { kind: 'noop', skipped: [], reason: 'Empty selection; existing preview selection kept.', legacy: false };
  const next = emptySelection();
  for (const id of values) {
    if (!id.trim()) throw new Error('Selection arrays cannot contain empty strings.');
    const family = familyForId(id);
    if (!family) throw new Error(`Unknown selection ID: ${id}`);
    next[family].push(id);
  }
  return { kind: 'replace', selection: next, skipped: [], legacy: false };
}

function importLegacy(entries: [string, unknown][]): SelectionImportResult {
  const next = emptySelection();
  const skipped: string[] = [];
  const prefixToFamily: [string, SelectionFamily][] = [
    ['WPFInstall', 'apps'], ['WPFTweaks', 'tweaks'], ['WPFToggle', 'toggles'],
    ['WPFFeature', 'features'], ['WPFAppx', 'appx'],
  ];
  for (const [property, raw] of entries) {
    const familyPrefix = prefixToFamily.find(([prefix]) => property.toLowerCase().startsWith(prefix.toLowerCase()));
    if (!familyPrefix) continue;
    const values = Array.isArray(raw) ? raw : [raw];
    for (const value of values) {
      if (typeof value !== 'string' || !value.trim()) continue;
      const family = familyForId(value);
      if (!family) {
        skipped.push(value);
        continue;
      }
      next[family].push(value);
    }
  }
  if (selectionFamilies.every((family) => next[family].length === 0)) {
    return { kind: 'noop', skipped, reason: 'No supported selections found; existing preview selection kept.', legacy: true };
  }
  return { kind: 'replace', selection: next, skipped, legacy: true };
}

export function applyPreset(state: SelectionState, presetName: string, scope: PresetScope): SelectionState {
  const ids = catalog.named_presets[presetName];
  if (!ids) throw new Error(`Unknown preset: ${presetName}`);
  const categorized = emptySelection();
  for (const id of ids) {
    const family = familyForId(id);
    if (!family) throw new Error(`Preset contains unknown selection ID: ${id}`);
    categorized[family].push(id);
  }
  const familyForScope: Partial<Record<PresetScope, SelectionFamily>> = {
    'WPFTweak*': 'tweaks', 'WPFInstall*': 'apps', 'WPFToggle*': 'toggles',
    'WPFFeature*': 'features', 'WPFAppx*': 'appx',
  };
  const next = { ...state, ...Object.fromEntries(selectionFamilies.map((family) => [family, [...state[family]]])) } as SelectionState;
  const clearFamily = familyForScope[scope];
  if (clearFamily) next[clearFamily] = [];
  for (const family of selectionFamilies) next[family].push(...categorized[family]);
  return next;
}

export const selectionPresets = Object.keys(catalog.named_presets);
export const selectionIdsByFamily = catalog.categories;
