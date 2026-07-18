'use client';

import React, { useState } from 'react';
import './settings.css';
import { ScreenHeader } from '@/components/shell/ScreenHeader';
import { TranscriptionSection } from './sections/TranscriptionSection';
import { SummarySection } from './sections/SummarySection';
import { DiarizationSection } from './sections/DiarizationSection';
import { RecordingSection } from './sections/RecordingSection';

type SectionId = 's-trans' | 's-sum' | 's-diar' | 's-rec';

const NAV: { id: SectionId; label: string }[] = [
  { id: 's-trans', label: 'Transcrição' },
  { id: 's-sum', label: 'Modelo de resumo' },
  { id: 's-diar', label: 'Diarização' },
  { id: 's-rec', label: 'Gravação' },
];

/**
 * Settings screen (task R4): a dedicated full screen replacing the old modal /
 * tabbed settings. Own left nav with four sections and a 640px content column,
 * matching docs/design/redesign-2026-07/settings.html. Rendered inside the R1
 * AppShell, so it only composes its header + section body here.
 */
export default function SettingsPage() {
  const [active, setActive] = useState<SectionId>('s-trans');

  return (
    <>
      <ScreenHeader
        crumbs={[{ label: 'Configurações' }]}
        actions={<span className="badge">sem telemetria · sem conta · sem nuvem</span>}
      />

      <div className="set-wrap">
        <nav className="set-nav" aria-label="Seções de configurações">
          {NAV.map((item) => (
            <button
              key={item.id}
              className={active === item.id ? 'on' : ''}
              onClick={() => setActive(item.id)}
            >
              {item.label}
            </button>
          ))}
        </nav>

        <div className="set-main">
          {active === 's-trans' && <TranscriptionSection />}
          {active === 's-sum' && <SummarySection />}
          {active === 's-diar' && <DiarizationSection />}
          {active === 's-rec' && <RecordingSection />}
        </div>
      </div>
    </>
  );
}
