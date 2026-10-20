'use client';
import { useState } from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import MosaicArt from '@/components/onboarding/MosaicArt';
import WelcomeStep from '@/components/onboarding/steps/WelcomeStep';
import NameStep from '@/components/onboarding/steps/NameStep';
import MusicStep from '@/components/onboarding/steps/MusicStep';
import SpotifyStep from '@/components/onboarding/steps/SpotifyStep';
import OptionsStep from '@/components/onboarding/steps/OptionsStep';
import { STEPS } from '@/components/onboarding/constants';

export default function Onboarding({ onDone }) {
  const [step, setStep] = useState(0);
  const [name, setName] = useState('');
  const [folders, setFolders] = useState([]);
  const [imports, setImports] = useState([]);
  const [presence, setPresence] = useState(true);
  const [lyrics, setLyrics] = useState(true);
  const [busy, setBusy] = useState(false);

  const next = () => setStep((s) => Math.min(STEPS - 1, s + 1));
  const back = () => setStep((s) => Math.max(0, s - 1));

  const finish = async () => {
    setBusy(true);
    await window.mosaic.saveConfig({ name });
    const saved = await window.mosaic.loadStore();
    const playlists = imports.filter((i) => i.tracks.length).map(({ id, name: title, tracks }) => ({ id, name: title, tracks }));

    await window.mosaic.saveStore({
      ...saved,
      sources: [...new Set([...(saved.sources || []), ...folders])],
      playlists: [...(saved.playlists || []), ...playlists],
      settings: { ...(saved.settings || {}), presence, lyricsOpen: lyrics },
    });
    onDone(name.trim());
  };

  return (
    <main className="ob">
      <section className="ob-art"><MosaicArt step={step} /></section>

      <section className="ob-main">
        <div className="ob-col">
          <p className="ob-count">{String(step + 1).padStart(2, '0')} / {String(STEPS).padStart(2, '0')}</p>

          <AnimatePresence mode="wait" initial={false}>
            {step === 0 && <WelcomeStep key="welcome" onNext={next} />}
            {step === 1 && <NameStep key="name" name={name} setName={setName} onBack={back} onNext={next} />}
            {step === 2 && <MusicStep key="music" folders={folders} setFolders={setFolders} onBack={back} onNext={next} />}
            {step === 3 && <SpotifyStep key="spotify" imports={imports} setImports={setImports} onBack={back} onNext={next} />}
            {step === 4 && <OptionsStep key="options" presence={presence} setPresence={setPresence} lyrics={lyrics} setLyrics={setLyrics} busy={busy} onBack={back} onFinish={finish} />}
          </AnimatePresence>

          <div className="ob-bars">
            {Array.from({ length: STEPS }).map((_, i) => (
              <motion.span key={i} animate={{ backgroundColor: i <= step ? 'var(--md-primary)' : 'var(--md-outline-variant)' }} />
            ))}
          </div>
        </div>
      </section>
    </main>
  );
}
