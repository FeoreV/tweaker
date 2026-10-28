import { render } from 'preact';
import { useState } from 'preact/hooks';
import tweaks from './data/tweaks.json';
import applications from './data/applications.json';
import {
  applyPreset, encodeSelection, emptySelection, importSelectionJson, selectionFamilies,
  selectionIdsByFamily, selectionPresets, toggleSelection,
  type PresetScope, type SelectionFamily, type SelectionState,
} from './selection';

type Section = 'Overview' | 'Tweaks' | 'Applications' | 'Cleanup' | 'Diagnostics' | 'Settings';
const sections: Section[] = ['Overview', 'Tweaks', 'Applications', 'Cleanup', 'Diagnostics', 'Settings'];
const descriptions: Record<Section, string> = {
  Overview: 'View system information and start common maintenance tasks.',
  Tweaks: 'Review and apply system settings with a record of previous values.',
  Applications: 'Browse application names and package IDs from the reference catalog.',
  Cleanup: 'Review files that can be removed to free space.',
  Diagnostics: 'Inspect system health and repair options.',
  Settings: 'Manage backups, presets, and application preferences.',
};

type ApplicationEntry = {
  category: string;
  content: string;
  description: string;
  link: string;
  winget: string;
  choco?: string;
  foss: boolean;
};

const applicationEntries = Object.entries(applications as Record<string, ApplicationEntry>);
const applicationCategories = [...new Set(applicationEntries.map(([, application]) => application.category))].sort();

function packageId(value?: string) {
  return value && value.trim() && value.toLowerCase() !== 'na' ? value : 'Not listed';
}

function TweaksCatalog({ selection, onToggle }: { selection: SelectionState; onToggle: (family: SelectionFamily, id: string) => void }) {
  const [query, setQuery] = useState('');
  const normalized = query.trim().toLowerCase();
  const matches = tweaks.filter((tweak) => [tweak.name, tweak.description, tweak.category].some((text) => text.toLowerCase().includes(normalized)));
  return <section aria-label="Tweak catalog">
    <div class="catalog-header"><div><h2>Available in the reference catalog</h2><p>Descriptions are from WinUtil. Compatibility and behavior are not yet verified.</p></div><span>{matches.length} of {tweaks.length}</span></div>
    <label class="search-label" for="tweak-search">Search tweaks</label>
    <input id="tweak-search" class="search" type="search" value={query} onInput={(event) => setQuery(event.currentTarget.value)} placeholder="Name, description, or category" />
    <div class="catalog-list">{matches.map((tweak) => <article class="tweak-row" key={tweak.id}><div><h3>{tweak.name}</h3><p>{tweak.description || 'No description provided.'}</p><span class="category">{tweak.category}</span><label class="app-select"><input type="checkbox" checked={selection.tweaks.some((id) => id.toLowerCase() === tweak.id.toLowerCase())} onChange={() => onToggle('tweaks', tweak.id)} aria-label={`Select ${tweak.name} for preview`} /> <span>Select for preview</span></label></div><button class="unavailable" disabled>Unavailable</button></article>)}</div>
    {matches.length === 0 && <p class="no-results">No tweaks match your search.</p>}
  </section>;
}

function ApplicationsCatalog({ selection, onToggle }: { selection: SelectionState; onToggle: (family: SelectionFamily, id: string) => void }) {
  const [query, setQuery] = useState('');
  const [category, setCategory] = useState('All categories');
  const normalized = query.trim().toLowerCase();
  const matches = applicationEntries.filter(([id, application]) => {
    const inCategory = category === 'All categories' || application.category === category;
    const searchable = [id, application.content, application.category, application.description, application.link, application.winget, application.choco ?? ''].join(' ').toLowerCase();
    return inCategory && searchable.includes(normalized);
  });

  return <section aria-label="Application catalog">
    <div class="catalog-header"><div><h2>Reference catalog</h2><p>Package names are shown for reference. Availability and Windows compatibility are not verified.</p></div><span>{matches.length} of {applicationEntries.length}</span></div>
    <div class="app-filters">
      <div class="app-search-field">
        <label class="search-label" for="application-search">Search applications</label>
        <input id="application-search" class="search" type="search" value={query} onInput={(event) => setQuery(event.currentTarget.value)} placeholder="Name, description, ID, or package" />
      </div>
      <div class="app-category-field">
        <label class="search-label" for="application-category">Category</label>
        <select id="application-category" class="category-select" value={category} onChange={(event) => setCategory(event.currentTarget.value)}>
          <option>All categories</option>
          {applicationCategories.map((item) => <option value={item}>{item}</option>)}
        </select>
      </div>
    </div>
    <div class="app-list">{matches.map(([id, application]) => <article class="app-row" key={id}>
      <div class="app-details">
        <div class="app-title-line"><h3>{application.content}</h3><span class="category">{application.category}</span>{application.foss && <span class="app-foss">FOSS in catalog</span>}</div>
        <label class="app-select"><input type="checkbox" checked={selection.apps.some((key) => key.toLowerCase() === `WPFInstall${id}`.toLowerCase())} onChange={() => onToggle('apps', `WPFInstall${id}`)} aria-label={`Select ${application.content} for preview`} /> <span>Select for preview</span></label>
        <p>{application.description || 'No description provided.'}</p>
        <dl class="app-metadata">
          <div><dt>Catalog ID</dt><dd>{id}</dd></div>
          <div><dt>WinGet</dt><dd>{packageId(application.winget)}</dd></div>
          <div><dt>Chocolatey</dt><dd>{packageId(application.choco)}</dd></div>
          <div class="app-reference-row"><dt>Reference</dt><dd>{application.link}</dd></div>
        </dl>
      </div>
      <div class="app-actions" aria-label={`${application.content} actions`}><button class="unavailable" type="button" disabled>Install unavailable</button><button class="unavailable" type="button" disabled>Uninstall unavailable</button><button class="unavailable" type="button" disabled>Upgrade unavailable</button></div>
    </article>)}</div>
    {matches.length === 0 && <p class="no-results">No applications match these filters.</p>}
    <p class="app-footer-note">This preview does not open reference URLs or install software.</p>
  </section>;
}

function SelectionWorkspace({ selection, setSelection, onToggle }: {
  selection: SelectionState;
  setSelection: (selection: SelectionState) => void;
  onToggle: (family: SelectionFamily, id: string) => void;
}) {
  const [importText, setImportText] = useState('');
  const [message, setMessage] = useState('');
  const [isError, setIsError] = useState(false);
  const [presetName, setPresetName] = useState(selectionPresets[0] ?? '');
  const [presetScope, setPresetScope] = useState<PresetScope>('**');
  const exportJson = encodeSelection(selection) ?? '';
  const total = selectionFamilies.reduce((count, family) => count + selection[family].length, 0);

  const importSelection = () => {
    try {
      const result = importSelectionJson(importText);
      if (result.kind === 'noop') {
        const suffix = result.skipped.length ? ` Skipped unknown IDs: ${result.skipped.join(', ')}` : '';
        setMessage(result.reason + suffix);
        setIsError(false);
        return;
      }
      setSelection(result.selection);
      setMessage(result.skipped.length
        ? `Imported supported selections. Skipped unknown IDs: ${result.skipped.join(', ')}`
        : `Imported ${selectionFamilies.reduce((sum, family) => sum + result.selection[family].length, 0)} selections into this preview.`);
      setIsError(false);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Could not import selection JSON.');
      setIsError(true);
    }
  };

  const copySelection = async () => {
    try {
      await navigator.clipboard.writeText(exportJson);
      setMessage('WinUtil selection JSON copied.');
      setIsError(false);
    } catch {
      setMessage('Clipboard access is unavailable. Copy the JSON from the export field.');
      setIsError(true);
    }
  };

  const downloadSelection = () => {
    const url = URL.createObjectURL(new Blob([exportJson], { type: 'application/json' }));
    const link = document.createElement('a');
    link.href = url;
    link.download = 'rustweaker-selection.json';
    link.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    setMessage('WinUtil selection JSON downloaded.');
    setIsError(false);
  };

  const selectPreset = () => {
    try {
      setSelection(applyPreset(selection, presetName, presetScope));
      setMessage(`Applied ${presetName} to the local preview selection (${presetScope === '**' ? 'merge' : `replace ${presetScope}`}).`);
      setIsError(false);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Could not apply preview preset.');
      setIsError(true);
    }
  };

  return <section class="selection-panel workspace-selection" aria-labelledby="selection-title" aria-live="polite">
    <div class="selection-heading"><div><h2 id="selection-title">Local selection and presets</h2><p>{total} selected IDs across five categories</p></div><button class="clear-selection" type="button" onClick={() => setSelection(emptySelection())} disabled={total === 0}>Clear all</button></div>
    <p class="selection-preview-note">Local preview only. Import, export, presets, and checkboxes do not perform machine or package actions.</p>
    <div class="selection-family-list">
      {selectionFamilies.map((family) => <details class="selection-family" key={family}>
        <summary><span>{family}</span><span>{selection[family].length} selected</span></summary>
        <p class="selection-family-values">{selection[family].length ? selection[family].join(', ') : 'No IDs selected.'}</p>
        <div class="selection-id-list">{selectionIdsByFamily[family].map((id) => <label key={id}>
          <input type="checkbox" checked={selection[family].some((selected) => selected.toLowerCase() === id.toLowerCase())} onChange={() => onToggle(family, id)} />
          <code>{id}</code>
        </label>)}</div>
      </details>)}
    </div>
    <div class="selection-transfer" aria-label="Selection import and export">
      <h3>WinUtil JSON</h3>
      <p>Full selection IDs are exported in app, tweak, toggle, feature, then AppX order. One ID uses a JSON string; multiple IDs use an array.</p>
      <label class="search-label" for="selection-export">Export selection</label>
      <textarea id="selection-export" class="selection-json" value={exportJson} readOnly placeholder="Select at least one catalog ID to export." />
      <div class="selection-buttons">
        <button class="selection-button" type="button" onClick={copySelection} disabled={total === 0}>Copy JSON</button>
        <button class="selection-button" type="button" onClick={downloadSelection} disabled={total === 0}>Download JSON</button>
      </div>
      <label class="search-label" for="selection-import">Import JSON</label>
      <textarea id="selection-import" class="selection-json" value={importText} onInput={(event) => setImportText(event.currentTarget.value)} placeholder={'"WPFTweaksActivity" or ["WPFInstall7zip", "WPFTweaksActivity"]'} />
      <button class="selection-button" type="button" onClick={importSelection}>Import into preview</button>
    </div>
    <div class="selection-transfer preset-controls" aria-label="Preset preview">
      <h3>Named preset preview</h3>
      <p>Presets update only the local selection preview.</p>
      <label class="search-label" for="preset-name">Preset</label>
      <select id="preset-name" class="category-select" value={presetName} onChange={(event) => setPresetName(event.currentTarget.value)}>
        {selectionPresets.map((name) => <option value={name}>{name}</option>)}
      </select>
      <label class="search-label" for="preset-scope">Scope</label>
      <select id="preset-scope" class="category-select" value={presetScope} onChange={(event) => setPresetScope(event.currentTarget.value as PresetScope)}>
        <option value="**">** — Merge</option>
        <option value="WPFTweak*">WPFTweak* — Replace tweaks</option>
        <option value="WPFInstall*">WPFInstall* — Replace apps</option>
        <option value="WPFToggle*">WPFToggle* — Replace toggles</option>
        <option value="WPFFeature*">WPFFeature* — Replace features</option>
        <option value="WPFAppx*">WPFAppx* — Replace AppX</option>
      </select>
      <button class="selection-button" type="button" onClick={selectPreset} disabled={!presetName}>Apply preset to preview</button>
    </div>
    {message && <p class={isError ? 'selection-message error' : 'selection-message'} role={isError ? 'alert' : 'status'}>{message}</p>}
  </section>;
}

function App() {
  const [section, setSection] = useState<Section>('Overview');
  const [selection, setSelection] = useState<SelectionState>(emptySelection);
  const onToggle = (family: SelectionFamily, id: string) => setSelection((current) => toggleSelection(current, family, id));
  return (
    <div class="shell">
      <aside class="sidebar">
        <div class="brand"><span class="brand-mark">R</span><span>rustweaker<small>System toolkit</small></span></div>
        <nav aria-label="Main navigation">
          {sections.map((item) => <button type="button" class={section === item ? 'nav-item active' : 'nav-item'} aria-current={section === item ? 'page' : undefined} onClick={() => setSection(item)}>{item}</button>)}
        </nav>
        <div class="sidebar-note"><span class="status-dot" /> Development build</div>
      </aside>
      <main>
        <header class="topbar"><span>Workspace</span><span class="version">Preview</span></header>
        <div class="content">
          <div class="heading"><div><h1>{section}</h1><p>{descriptions[section]}</p></div></div>
          <div class="notice" role="status"><strong>Preview only</strong><p>This preview reads bundled catalog data. It does not read or change system settings; system actions are unavailable.</p></div>
          <SelectionWorkspace selection={selection} setSelection={setSelection} onToggle={onToggle} />
          {section === 'Tweaks' ? <TweaksCatalog selection={selection} onToggle={onToggle} /> : section === 'Applications' ? <ApplicationsCatalog selection={selection} onToggle={onToggle} /> : section === 'Overview' ? <div class="overview"><section class="panel"><h2>System information</h2><p>System details are not available in this preview.</p><dl><div><dt>Windows version</dt><dd>Unavailable</dd></div><div><dt>Processor</dt><dd>Unavailable</dd></div><div><dt>Memory</dt><dd>Unavailable</dd></div></dl></section><section class="panel"><h2>Quick actions</h2><p>System actions are unavailable in this preview.</p><button class="action" disabled>Review tweaks</button><button class="action" disabled>Run diagnostics</button></section></div> : <section class="panel empty"><h2>{section} unavailable</h2><p>Connect the system engine to use this section.</p></section>}
        </div>
      </main>
    </div>
  );
}

const root = document.getElementById('app');
if (root) render(<App />, root);
