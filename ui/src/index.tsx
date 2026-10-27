import { render } from 'preact';
import { useState } from 'preact/hooks';
import tweaks from './data/tweaks.json';

type Section = 'Overview' | 'Tweaks' | 'Applications' | 'Cleanup' | 'Diagnostics' | 'Settings';
const sections: Section[] = ['Overview', 'Tweaks', 'Applications', 'Cleanup', 'Diagnostics', 'Settings'];
const descriptions: Record<Section, string> = {
  Overview: 'View system information and start common maintenance tasks.',
  Tweaks: 'Review and apply system settings with a record of previous values.',
  Applications: 'Find and install applications for this computer.',
  Cleanup: 'Review files that can be removed to free space.',
  Diagnostics: 'Inspect system health and repair options.',
  Settings: 'Manage backups, presets, and application preferences.',
};

function TweaksCatalog() {
  const [query, setQuery] = useState('');
  const normalized = query.trim().toLowerCase();
  const matches = tweaks.filter((tweak) => [tweak.name, tweak.description, tweak.category].some((text) => text.toLowerCase().includes(normalized)));
  return <section aria-label="Tweak catalog">
    <div class="catalog-header"><div><h2>Available in the reference catalog</h2><p>Descriptions are from WinUtil. Compatibility and behavior are not yet verified.</p></div><span>{matches.length} of {tweaks.length}</span></div>
    <label class="search-label" for="tweak-search">Search tweaks</label>
    <input id="tweak-search" class="search" type="search" value={query} onInput={(event) => setQuery(event.currentTarget.value)} placeholder="Name, description, or category" />
    <div class="catalog-list">{matches.map((tweak) => <article class="tweak-row" key={tweak.id}><div><h3>{tweak.name}</h3><p>{tweak.description || 'No description provided.'}</p><span class="category">{tweak.category}</span></div><button class="unavailable" disabled>Unavailable</button></article>)}</div>
    {matches.length === 0 && <p class="no-results">No tweaks match your search.</p>}
  </section>;
}

function App() {
  const [section, setSection] = useState<Section>('Overview');
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
          <div class="notice" role="status"><strong>System connection unavailable</strong><p>This preview does not read or change system settings. Actions will become available when the system connection is ready.</p></div>
          {section === 'Tweaks' ? <TweaksCatalog /> : section === 'Overview' ? <div class="overview"><section class="panel"><h2>System information</h2><p>System details are not available in this preview.</p><dl><div><dt>Windows version</dt><dd>Unavailable</dd></div><div><dt>Processor</dt><dd>Unavailable</dd></div><div><dt>Memory</dt><dd>Unavailable</dd></div></dl></section><section class="panel"><h2>Quick actions</h2><p>Choose a maintenance task when the system connection is ready.</p><button class="action" disabled>Review tweaks</button><button class="action" disabled>Run diagnostics</button></section></div> : <section class="panel empty"><h2>{section} unavailable</h2><p>Connect the system engine to use this section.</p></section>}
        </div>
      </main>
    </div>
  );
}

const root = document.getElementById('app');
if (root) render(<App />, root);
