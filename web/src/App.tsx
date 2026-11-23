import { BENCHES } from './data/benches';
import { integer } from './lib/format';
import { setTab, useAppState } from './state/store';
import type { Tab } from './state/store';
import { DesignBench } from './views/DesignBench';
import { ImageBench } from './views/ImageBench';
import { Manual } from './views/Manual';
import { MissionBench } from './views/MissionBench';
import { SyntheticBench } from './views/SyntheticBench';
import { VideoBench } from './views/VideoBench';
import { ValidationBench } from './views/ValidationBench';

const TABS: readonly { id: Tab; label: string }[] = [
  { id: 'synthetic', label: 'Synthetic' },
  { id: 'image', label: 'Image' },
  { id: 'video', label: 'Video' },
  { id: 'design', label: 'Design' },
  { id: 'mission', label: 'Mission' },
  { id: 'validation', label: 'Validation' },
  { id: 'manual', label: 'Manual' },
];

export function App() {
  const state = useAppState();
  const question = BENCHES.find((bench) => bench.tab === state.tab)?.asks;

  return (
    <div className="app">
      <header className="masthead">
        <h1>Asterope</h1>
        <nav className="tabs" aria-label="Benches">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={tab.id === state.tab ? 'is-active' : undefined}
              aria-current={tab.id === state.tab ? 'page' : undefined}
              onClick={() => {
                setTab(tab.id);
              }}
            >
              {tab.label}
            </button>
          ))}
        </nav>
        <p className="masthead__meta">
          {state.ready ? `Hipparcos ${integer(state.catalogSize)} stars · V ≤ 6.5 · ICRS J2000` : 'Loading engine'}
        </p>
      </header>
      {state.fatal ? (
        <main className="fatal">
          <h2>The engine failed to start</h2>
          <p>{state.fatal}</p>
          <p>Build the WebAssembly module with <code>npm run wasm</code> and reload.</p>
        </main>
      ) : (
        <main className={question ? 'app__main app__main--asked' : 'app__main'}>
          {question ? <p className="question">{question}</p> : null}
          {state.tab === 'synthetic' ? <SyntheticBench state={state} /> : null}
          {state.tab === 'image' ? <ImageBench state={state} /> : null}
          {state.tab === 'video' ? <VideoBench state={state} /> : null}
          {state.tab === 'design' ? <DesignBench state={state} /> : null}
          {state.tab === 'mission' ? <MissionBench state={state} /> : null}
          {state.tab === 'validation' ? <ValidationBench state={state} /> : null}
          {state.tab === 'manual' ? <Manual /> : null}
        </main>
      )}
    </div>
  );
}
