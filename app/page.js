'use client';
import { useEffect, useState } from 'react';
import DataProvider from '@/components/data/DataProvider';
import ToastProvider from '@/components/ui/Toast';
import PlayerProvider from '@/components/player/PlayerProvider';
import TrackMenuProvider from '@/components/tracks/TrackMenu';
import Shell from '@/components/app/Shell';
import Onboarding from '@/components/onboarding/Onboarding';

export default function Page() {
  const [state, setState] = useState({ ready: false, onboarded: false, name: '' });

  useEffect(() => {
    if (!window.mosaic) return setState({ ready: true, onboarded: false, name: '', noElectron: true });
    window.mosaic.getConfig().then((c) => setState({ ready: true, ...c }));
  }, []);

  if (!state.ready) return null;
  if (state.noElectron) {
    return (
      <main className="onboard">
        <div className="onboard-inner">
          <div className="step">
            <h1>Open Mosaic in Electron</h1>
            <p>Run <code>npm run dev</code> — this page needs the desktop shell.</p>
          </div>
        </div>
      </main>
    );
  }
  if (!state.onboarded) {
    return (
      <>
        <div className="titlebar" style={window.mosaic?.platform === 'darwin' ? { paddingLeft: 92 } : undefined} />
        <Onboarding onDone={(name) => setState({ ready: true, onboarded: true, name })} />
      </>
    );
  }

  return (
    <DataProvider>
      <ToastProvider>
        <PlayerProvider>
          <TrackMenuProvider>
            <Shell
              name={state.name}
              onReset={async () => {
                await window.mosaic.resetConfig();
                setState({ ready: true, onboarded: false, name: '' });
              }}
            />
          </TrackMenuProvider>
        </PlayerProvider>
      </ToastProvider>
    </DataProvider>
  );
}
