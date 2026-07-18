'use client';

import { useState } from 'react';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { SummaryArticle } from './summaryMarkdown';

type SummaryStatus = 'idle' | 'processing' | 'summarizing' | 'regenerating' | 'completed' | 'error';

interface SummaryPanelProps {
  /** Resolved summary markdown ("" when there is none yet). */
  markdown: string;
  hasSummary: boolean;
  summaryStatus: SummaryStatus;
  summaryError: string | null;
  hasTranscripts: boolean;
  modelConfig: ModelConfig;
  generatedAtLabel: string;
  templates: Array<{ id: string; name: string }>;
  selectedTemplate: string;
  onTemplateSelect: (id: string, name: string) => void;
  onGenerate: () => void;
  onRegenerate: () => void;
  onCopySummary: () => Promise<void>;
  onExportMarkdown: () => void;
  aiContext: string;
  onAiContextChange: (v: string) => void;
  onOpenSettings: () => void;
}

function providerLabel(config: ModelConfig): string {
  switch (config.provider) {
    case 'builtin-ai':
      return 'IA local (built-in)';
    case 'ollama':
      return config.model ? `Ollama (${config.model})` : 'Ollama';
    case 'claude':
      return 'Claude';
    case 'openai':
      return 'OpenAI';
    case 'groq':
      return 'Groq';
    case 'openrouter':
      return 'OpenRouter';
    case 'custom-openai':
      return 'OpenAI compatível';
    case 'cli-agent':
      return `CLI Agent (${config.cliAgentPreset || config.cliAgentCommand || 'codex'})`;
    default:
      return config.provider;
  }
}

function cliName(config: ModelConfig): string {
  return config.cliAgentPreset || config.cliAgentCommand || 'codex';
}

function loginHint(config: ModelConfig): string {
  const name = cliName(config);
  if (name === 'codex') return '$ codex login';
  if (config.cliAgentCommand && config.cliAgentPreset === 'custom') return `$ ${config.cliAgentCommand}`;
  return `$ ${name}`;
}

interface ErrorContent {
  title: string;
  body: string;
  hint?: string;
}

/** Turn a raw provider error into an actionable banner (C2/C3). */
function parseError(config: ModelConfig, msg: string): ErrorContent {
  const m = msg.toLowerCase();
  if (config.provider === 'cli-agent') {
    const label = providerLabel(config);
    if (/expired|session|login|unauthorized|401|not authenticated|auth/.test(m)) {
      return {
        title: `A sessão do ${cliName(config)} CLI expirou.`,
        body: 'O provider CLI Agent depende do login da sua assinatura. Renove a sessão no terminal e tente de novo:',
        hint: loginHint(config),
      };
    }
    if (/not installed|command not found|no such file|not found|enoent/.test(m)) {
      return {
        title: `O CLI ${cliName(config)} não está instalado.`,
        body: 'Instale o CLI e verifique que ele está no PATH, ou troque de provider.',
        hint: `$ which ${cliName(config)}`,
      };
    }
    if (/timeout|timed out|deadline/.test(m)) {
      return {
        title: `O ${label} demorou demais para responder.`,
        body: 'A geração excedeu o tempo limite. Tente novamente ou troque de provider.',
      };
    }
    return { title: `Falha no ${label}.`, body: msg };
  }
  return { title: 'Falha ao gerar o resumo.', body: msg };
}

/**
 * Redesigned meeting summary panel (task R3): copy-as-markdown / export header,
 * template + regenerate toolbar, `article.md` body, a persisted "Contexto para a
 * IA" field, and the empty → generating → actionable-CLI-error → success state
 * machine. Wired to the existing summary generation plumbing.
 */
export function SummaryPanel({
  markdown,
  hasSummary,
  summaryStatus,
  summaryError,
  hasTranscripts,
  modelConfig,
  generatedAtLabel,
  templates,
  selectedTemplate,
  onTemplateSelect,
  onGenerate,
  onRegenerate,
  onCopySummary,
  onExportMarkdown,
  aiContext,
  onAiContextChange,
  onOpenSettings,
}: SummaryPanelProps) {
  const [copied, setCopied] = useState(false);

  const isGenerating =
    summaryStatus === 'processing' || summaryStatus === 'summarizing' || summaryStatus === 'regenerating';
  const isError = summaryStatus === 'error' && !!summaryError;

  const handleCopy = async () => {
    await onCopySummary();
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <section className="panel" id="panel-summary" aria-label="Resumo">
      <div className="panel-head">
        <h2>Resumo</h2>
        <button
          type="button"
          className="icon-btn"
          onClick={handleCopy}
          data-tip={copied ? 'Copiado ✓' : 'Copiar como Markdown'}
          disabled={!hasSummary}
        >
          ⧉
        </button>
        <button
          type="button"
          className="icon-btn"
          onClick={onExportMarkdown}
          data-tip="Exportar .md"
          disabled={!hasSummary}
        >
          ↧
        </button>
      </div>

      <div className="sum-toolbar">
        <select
          className="input"
          value={selectedTemplate}
          onChange={(e) => {
            const opt = templates.find((t) => t.id === e.target.value);
            onTemplateSelect(e.target.value, opt?.name ?? e.target.value);
          }}
        >
          {templates.length === 0 && <option value={selectedTemplate}>Reunião padrão</option>}
          {templates.map((t) => (
            <option key={t.id} value={t.id}>
              {t.name}
            </option>
          ))}
        </select>
        {hasSummary && !isGenerating && (
          <button type="button" className="btn small primary" onClick={onRegenerate}>
            Regenerar
          </button>
        )}
      </div>

      <div className="panel-body">
        {isGenerating ? (
          <div className="sum-state">
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--muted)', fontSize: 12.5 }}>
              <span className="spinner" /> <span>Gerando com {providerLabel(modelConfig)}…</span>
            </div>
            <div className="progress indeterminate" style={{ marginTop: 12 }}>
              <i />
            </div>
          </div>
        ) : isError ? (
          (() => {
            const err = parseError(modelConfig, summaryError as string);
            return (
              <div className="sum-state">
                <div className="banner danger">
                  <span className="b-ico">✕</span>
                  <div className="b-body">
                    <strong>{err.title}</strong>
                    <br />
                    {err.body}
                    {err.hint && <div className="code-hint">{err.hint}</div>}
                    <div className="b-actions">
                      <button type="button" className="btn small" onClick={onGenerate}>
                        Tentar novamente
                      </button>
                      <button type="button" className="btn small ghost" onClick={onOpenSettings}>
                        Trocar provider
                      </button>
                    </div>
                  </div>
                </div>
              </div>
            );
          })()
        ) : hasSummary ? (
          <div className="sum-body">
            <SummaryArticle
              markdown={markdown}
              providerLabel={`Gerado com ${providerLabel(modelConfig)}`}
              generatedAt={generatedAtLabel}
              titleNote="o título da reunião vem deste H1"
            />
          </div>
        ) : (
          <div className="sum-state empty">
            <span className="e-ico">
              <svg width="20" height="20" viewBox="0 0 16 16" fill="none">
                <rect x="3" y="2" width="10" height="12" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <path d="M5.6 5.5h4.8M5.6 8h4.8M5.6 10.5h2.8" stroke="currentColor" strokeWidth="1.1" strokeLinecap="round" />
              </svg>
            </span>
            <h3>Nenhum resumo ainda</h3>
            <p>O primeiro resumo também dá o título definitivo à reunião, a partir do H1.</p>
            <button type="button" className="btn primary" onClick={onGenerate} disabled={!hasTranscripts}>
              Gerar resumo
            </button>
          </div>
        )}
      </div>

      <div className="ctx-field field">
        <label htmlFor="aiCtx">Contexto para a IA</label>
        <input
          className="input"
          id="aiCtx"
          value={aiContext}
          onChange={(e) => onAiContextChange(e.target.value)}
          placeholder={'ex.: "Falante 4 é o cliente; foque nas decisões técnicas"'}
        />
      </div>
    </section>
  );
}
