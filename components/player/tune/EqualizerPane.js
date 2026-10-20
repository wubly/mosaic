'use client';
import { MdCheck } from 'react-icons/md';
import { useData } from '@/components/data/DataProvider';
import Switch from '@/components/ui/Switch';
import GainSlider from '@/components/player/tune/GainSlider';
import { BANDS, PRESETS } from '@/components/player/tune/presets';

export default function EqualizerPane() {
  const { settings, setSetting } = useData();

  const setGain = (index, value) => {
    const gains = [...settings.eqGains];
    gains[index] = value;
    setSetting('eqGains', gains);
    setSetting('eqPreset', 'Custom');
    if (!settings.eqOn) setSetting('eqOn', true);
  };

  const applyPreset = (name) => {
    setSetting('eqGains', PRESETS[name]);
    setSetting('eqPreset', name);
    setSetting('eqOn', name !== 'Flat');
  };

  return (
    <div className="pane">
      <label className="switch-row">
        <span><b>Equalizer</b><em>{settings.eqOn ? settings.eqPreset : 'Off'}</em></span>
        <Switch on={settings.eqOn} onChange={(on) => setSetting('eqOn', on)} />
      </label>

      <div className={`eq-card ${settings.eqOn ? '' : 'off'}`}>
        <div className="db-scale"><span>+12</span><span>0</span><span>−12</span></div>
        <div className="gains">
          {settings.eqGains.map((gain, i) => (
            <div key={i} className="gain-col">
              <span className="gv">{gain > 0 ? `+${gain}` : gain}</span>
              <GainSlider value={gain} onChange={(value) => setGain(i, value)} label={`${BANDS[i]} Hz`} />
              <span className="gl">{BANDS[i]}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="chip-row">
        {Object.keys(PRESETS).map((name) => {
          const on = settings.eqPreset === name;
          return (
            <button key={name} className={`chip ${on ? 'on' : ''}`} onClick={() => applyPreset(name)}>
              {on && <MdCheck size={16} />}{name}
            </button>
          );
        })}
      </div>
    </div>
  );
}
