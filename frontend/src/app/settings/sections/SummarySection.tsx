'use client';

import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import { useConfig } from '@/contexts/ConfigContext';
import { configService, type ModelConfig } from '@/services/configService';

type ProviderValue = ModelConfig['provider'];
type KeyProvider = 'claude' | 'openai' | 'groq' | 'openrouter';

interface ProviderMeta {
  value: ProviderValue;
  title: string;
  sub: string;
  badge?: { cls: string; text: string };
  keyProvider?: KeyProvider;
  keyPlaceholder?: string;
}

const PROVIDERS: ProviderMeta[] = [
  { value: 'builtin-ai', title: 'IA integrada (local)', sub: 'Modelo embutido no app · nada sai da máquina', badge: { cls: 'ok', text: 'recomendado' } },
  { value: 'ollama', title: 'Ollama', sub: 'Seus modelos locais via http://localhost:11434', badge: { cls: '', text: 'local' } },
  { value: 'claude', title: 'Claude', sub: 'API da Anthropic · requer chave', badge: { cls: 'warn', text: 'nuvem' }, keyProvider: 'claude', keyPlaceholder: 'sk-ant-…' },
  { value: 'openai', title: 'OpenAI', sub: 'requer chave', badge: { cls: 'warn', text: 'nuvem' }, keyProvider: 'openai', keyPlaceholder: 'sk-…' },
  { value: 'groq', title: 'Groq', sub: 'requer chave', badge: { cls: 'warn', text: 'nuvem' }, keyProvider: 'groq', keyPlaceholder: 'gsk_…' },
  { value: 'openrouter', title: 'OpenRouter', sub: 'requer chave', badge: { cls: 'warn', text: 'nuvem' }, keyProvider: 'openrouter', keyPlaceholder: 'sk-or-…' },
  { value: 'custom-openai', title: 'Endpoint personalizado', sub: 'Qualquer API compatível com OpenAI' },
  { value: 'cli-agent', title: 'CLI Agent', sub: 'Usa a assinatura de um CLI já instalado (Codex, Claude Code, Gemini)', badge: { cls: 'warn', text: 'nuvem' } },
];

const CLI_PRESETS: { value: 'codex' | 'claude' | 'gemini'; label: string; cmd: string }[] = [
  { value: 'codex', label: 'Codex', cmd: 'codex exec --model gpt-5' },
  { value: 'claude', label: 'Claude Code', cmd: 'claude -p' },
  { value: 'gemini', label: 'Gemini', cmd: 'gemini' },
];

type InstallStatus = 'unknown' | 'checking' | 'installed' | 'not-found';
type CliPreset = 'codex' | 'claude' | 'gemini' | 'custom';

/**
 * Summary-provider section (task R4): a single radio card listing every summary
 * provider, wired to the real model-config backend. Cloud providers reveal a
 * Keychain-hinted API-key field; the CLI Agent reveals installed-detection
 * presets and a real "Testar conexão" round-trip.
 */
export function SummarySection() {
  const { modelConfig, setModelConfig, providerApiKeys, updateProviderApiKey } = useConfig();

  const selected = modelConfig.provider;
  const [customEndpoint, setCustomEndpoint] = useState<string>(modelConfig.customOpenAIEndpoint ?? '');

  // CLI agent state.
  const [cliPreset, setCliPreset] = useState<CliPreset>('codex');
  const [customCommand, setCustomCommand] = useState<string>('');
  const [installStatus, setInstallStatus] = useState<Record<string, InstallStatus>>({});
  const [test, setTest] = useState<{ state: 'idle' | 'running' | 'ok' | 'error'; message?: string; seconds?: number }>({ state: 'idle' });

  useEffect(() => {
    setCustomEndpoint(modelConfig.customOpenAIEndpoint ?? '');
  }, [modelConfig.customOpenAIEndpoint]);

  // Load CLI agent config once.
  useEffect(() => {
    configService.getCliAgentConfig().then((cfg) => {
      if (cfg) {
        setCliPreset(cfg.preset);
        setCustomCommand(cfg.command ?? '');
      }
    }).catch(() => {});
  }, []);

  const persistProvider = useCallback(
    async (provider: ProviderValue) => {
      const next: ModelConfig = { ...modelConfig, provider };
      setModelConfig(next);
      try {
        const keyForProvider =
          provider === 'claude' || provider === 'openai' || provider === 'groq' || provider === 'openrouter'
            ? providerApiKeys[provider]
            : null;
        await invoke('api_save_model_config', {
          provider: next.provider,
          model: next.model,
          whisperModel: next.whisperModel,
          apiKey: keyForProvider ?? null,
          ollamaEndpoint: next.ollamaEndpoint ?? null,
        });
        await emit('model-config-updated', next);
      } catch (err) {
        toast.error('Não foi possível salvar o provider', { description: String(err) });
      }
    },
    [modelConfig, providerApiKeys, setModelConfig]
  );

  const persistApiKey = useCallback(
    async (provider: KeyProvider, key: string) => {
      updateProviderApiKey(provider, key);
      if (selected !== provider) return;
      try {
        await invoke('api_save_model_config', {
          provider,
          model: modelConfig.model,
          whisperModel: modelConfig.whisperModel,
          apiKey: key || null,
          ollamaEndpoint: modelConfig.ollamaEndpoint ?? null,
        });
      } catch (err) {
        toast.error('Não foi possível salvar a chave', { description: String(err) });
      }
    },
    [modelConfig, selected, updateProviderApiKey]
  );

  const persistCustomEndpoint = useCallback(async () => {
    if (!customEndpoint) return;
    try {
      await configService.saveCustomOpenAIConfig({
        endpoint: customEndpoint,
        apiKey: modelConfig.customOpenAIApiKey ?? null,
        model: modelConfig.customOpenAIModel ?? '',
        maxTokens: modelConfig.maxTokens ?? null,
        temperature: modelConfig.temperature ?? null,
        topP: modelConfig.topP ?? null,
      });
      setModelConfig({ ...modelConfig, customOpenAIEndpoint: customEndpoint });
      toast.success('Endpoint salvo');
    } catch (err) {
      toast.error('Não foi possível salvar o endpoint', { description: String(err) });
    }
  }, [customEndpoint, modelConfig, setModelConfig]);

  const persistCliPreset = useCallback(
    async (preset: CliPreset, command: string) => {
      try {
        await configService.saveCliAgentConfig({
          preset,
          command: preset === 'custom' ? command || null : null,
          args: null,
          timeoutSecs: null,
        });
      } catch (err) {
        console.warn('Failed to save CLI agent config:', err);
      }
    },
    []
  );

  // Installed-detection for CLI presets when the CLI detail is visible.
  useEffect(() => {
    if (selected !== 'cli-agent') return;
    let active = true;
    (async () => {
      for (const p of CLI_PRESETS) {
        setInstallStatus((s) => ({ ...s, [p.value]: 'checking' }));
        try {
          await configService.testCliAgentConnection(p.value, null, null);
          if (active) setInstallStatus((s) => ({ ...s, [p.value]: 'installed' }));
        } catch {
          if (active) setInstallStatus((s) => ({ ...s, [p.value]: 'not-found' }));
        }
      }
    })();
    return () => {
      active = false;
    };
  }, [selected]);

  const runTest = useCallback(async () => {
    setTest({ state: 'running' });
    const start = Date.now();
    try {
      const res = await configService.testCliAgentConnection(
        cliPreset,
        cliPreset === 'custom' ? customCommand || null : null,
        null
      );
      const seconds = (Date.now() - start) / 1000;
      setTest({ state: 'ok', seconds, message: res.message });
    } catch (err) {
      setTest({ state: 'error', message: String(err) });
    }
  }, [cliPreset, customCommand]);

  const runningCmd =
    cliPreset === 'custom' ? customCommand || 'meu-cli' : CLI_PRESETS.find((p) => p.value === cliPreset)?.cmd ?? '';

  return (
    <section className="sect on set-col" id="s-sum">
      <h1>Modelo de resumo</h1>
      <p className="lede">Quem escreve os resumos. Local por padrão; provedores externos só quando você escolher.</p>

      <div className="card">
        {PROVIDERS.map((p) => (
          <React.Fragment key={p.value}>
            <label className="card-row">
              <input
                type="radio"
                name="prov"
                className="prov-radio"
                value={p.value}
                checked={selected === p.value}
                onChange={() => persistProvider(p.value)}
              />
              <div className="info">
                <b>{p.title}</b>
                <span>{p.sub}</span>
              </div>
              {p.badge && <span className={`badge ${p.badge.cls}`.trim()}>{p.badge.text}</span>}
            </label>

            {/* cloud API-key detail */}
            {p.keyProvider && (
              <div className={`prov-detail${selected === p.value ? ' on' : ''}`}>
                <div className="field">
                  <label htmlFor={`key-${p.value}`}>Chave da API</label>
                  <input
                    className="input"
                    id={`key-${p.value}`}
                    type="password"
                    autoComplete="off"
                    placeholder={p.keyPlaceholder}
                    value={providerApiKeys[p.keyProvider] ?? ''}
                    onChange={(e) => updateProviderApiKey(p.keyProvider!, e.target.value)}
                    onBlur={(e) => persistApiKey(p.keyProvider!, e.target.value)}
                  />
                  <span className="hint">Guardada no Keychain do macOS, nunca em texto puro.</span>
                </div>
              </div>
            )}

            {/* custom endpoint detail */}
            {p.value === 'custom-openai' && (
              <div className={`prov-detail${selected === 'custom-openai' ? ' on' : ''}`}>
                <div className="field">
                  <label htmlFor="custom-ep">URL do endpoint</label>
                  <input
                    className="input"
                    id="custom-ep"
                    placeholder="http://192.168.0.10:8080/v1"
                    value={customEndpoint}
                    onChange={(e) => setCustomEndpoint(e.target.value)}
                    onBlur={persistCustomEndpoint}
                  />
                </div>
              </div>
            )}

            {/* CLI agent detail */}
            {p.value === 'cli-agent' && (
              <div className={`prov-detail${selected === 'cli-agent' ? ' on' : ''}`}>
                <div className="banner warn" style={{ marginBottom: 12 }}>
                  <span className="b-ico">⚠</span>
                  <div className="b-body">
                    <strong>Este provider envia o transcrito para fora da máquina</strong> — incluindo os nomes dos
                    falantes, se a opção estiver ativa na Diarização. Use apenas se a sua assinatura permitir esse conteúdo.
                  </div>
                </div>

                {CLI_PRESETS.map((preset) => {
                  const status = installStatus[preset.value] ?? 'unknown';
                  return (
                    <div className="cli-preset" key={preset.value}>
                      <input
                        type="radio"
                        name="cli"
                        checked={cliPreset === preset.value}
                        onChange={() => {
                          setCliPreset(preset.value);
                          persistCliPreset(preset.value, customCommand);
                          setTest({ state: 'idle' });
                        }}
                        aria-label={`Usar ${preset.label}`}
                      />
                      <div>
                        <b>{preset.label}</b>
                        <div className="cmd">{preset.cmd}</div>
                      </div>
                      <span className="grow" />
                      {status === 'installed' ? (
                        <span className="badge ok">✓ instalado</span>
                      ) : status === 'not-found' ? (
                        <span className="badge danger" data-tip={`Nenhum executável '${preset.value}' no PATH`}>não encontrado</span>
                      ) : status === 'checking' ? (
                        <span className="mono-meta">verificando…</span>
                      ) : (
                        <span className="mono-meta">—</span>
                      )}
                    </div>
                  );
                })}

                <div className="cli-preset">
                  <input
                    type="radio"
                    name="cli"
                    checked={cliPreset === 'custom'}
                    onChange={() => {
                      setCliPreset('custom');
                      persistCliPreset('custom', customCommand);
                      setTest({ state: 'idle' });
                    }}
                    aria-label="Comando personalizado"
                  />
                  <div style={{ flex: 1 }}>
                    <b>Comando personalizado</b>
                    <input
                      className="input"
                      style={{ marginTop: 6, fontFamily: 'var(--font-mono)', fontSize: 12 }}
                      placeholder="meu-cli --prompt -"
                      value={customCommand}
                      onChange={(e) => setCustomCommand(e.target.value)}
                      onFocus={() => setCliPreset('custom')}
                      onBlur={(e) => persistCliPreset('custom', e.target.value)}
                    />
                  </div>
                </div>

                <button className="btn small" onClick={runTest} disabled={test.state === 'running'}>
                  Testar conexão
                </button>

                {test.state === 'running' && (
                  <div className="test-out on" style={{ color: 'var(--muted)', fontSize: 12.5 }}>
                    <span className="spinner" style={{ display: 'inline-block', verticalAlign: -3, marginRight: 7 }} />
                    Executando <span className="mono">{runningCmd}</span>…
                  </div>
                )}
                {test.state === 'ok' && (
                  <div className="test-out on">
                    <div className="banner" style={{ borderColor: 'transparent', background: 'var(--ok-soft)', color: 'var(--ok)' }}>
                      <span className="b-ico">✓</span>
                      <div className="b-body">
                        Resposta recebida em <strong>{(test.seconds ?? 0).toFixed(1).replace('.', ',')}s</strong> — o CLI está autenticado e pronto.
                      </div>
                    </div>
                  </div>
                )}
                {test.state === 'error' && (
                  <div className="test-out on">
                    <div className="banner danger">
                      <span className="b-ico">⚠</span>
                      <div className="b-body">{test.message || 'A conexão falhou. Verifique se o CLI está instalado e autenticado.'}</div>
                    </div>
                  </div>
                )}
              </div>
            )}
          </React.Fragment>
        ))}
      </div>
    </section>
  );
}
