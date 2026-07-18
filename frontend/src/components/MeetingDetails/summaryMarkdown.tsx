'use client';

import React from 'react';
import { Summary } from '@/types';

/**
 * Renders a generated summary as the design's `article.md` body (task R3):
 * an H1 title, an `h1-meta` line (provider · timestamp · title note), section
 * headings, bullet lists, and owner-tagged checkbox tasks.
 *
 * The summary arrives either as a markdown string (current backend flow) or as
 * the legacy `Summary` section map; both are normalized to markdown first.
 */

/** Flatten the legacy section map into markdown so one renderer covers both. */
export function summaryToMarkdown(summary: Summary | { markdown?: string } | null, fallbackTitle: string): string {
  if (!summary) return '';
  if ('markdown' in summary && typeof (summary as any).markdown === 'string') {
    return (summary as any).markdown as string;
  }
  const sections = Object.entries(summary as Summary)
    .filter(([key]) => key !== 'markdown' && key !== 'summary_json' && key !== '_section_order' && key !== 'MeetingName')
    .map(([, section]) => {
      if (section && typeof section === 'object' && 'title' in section && 'blocks' in section) {
        const title = `## ${section.title}`;
        const body = (section.blocks || []).map((b: any) => `- ${b.content}`).join('\n');
        return `${title}\n${body}`;
      }
      return '';
    })
    .filter(Boolean);
  const heading = fallbackTitle ? `# ${fallbackTitle}\n\n` : '';
  return heading + sections.join('\n\n');
}

/** Split a string on **bold** spans into React nodes. */
function inline(text: string, keyPrefix: string): React.ReactNode[] {
  const parts = text.split(/(\*\*[^*]+\*\*)/g);
  return parts
    .filter((p) => p.length > 0)
    .map((p, i) => {
      const m = p.match(/^\*\*([^*]+)\*\*$/);
      if (m) return <strong key={`${keyPrefix}-b${i}`}>{m[1]}</strong>;
      return <React.Fragment key={`${keyPrefix}-t${i}`}>{p}</React.Fragment>;
    });
}

/** A task line may lead with an owner: `**Rebeca** — …` or `Rebeca — …`. */
function renderTask(rest: string, checked: boolean, key: string): React.ReactNode {
  let owner: string | null = null;
  let body = rest;
  const bold = rest.match(/^\*\*(.+?)\*\*\s*[—–-]\s*(.*)$/);
  const plain = rest.match(/^([^—–-]{1,32}?)\s+[—–-]\s+(.*)$/);
  if (bold) {
    owner = bold[1].trim();
    body = bold[2];
  } else if (plain) {
    owner = plain[1].trim();
    body = plain[2];
  }
  return (
    <div className="task" key={key}>
      <input type="checkbox" defaultChecked={checked} />
      <span>
        {owner && <span className="owner">{owner}</span>}
        {owner ? ' — ' : ''}
        {inline(body, key)}
      </span>
    </div>
  );
}

export interface SummaryArticleProps {
  markdown: string;
  /** Provider label shown in the h1-meta line, e.g. "CLI Agent (codex)". */
  providerLabel?: string;
  /** Generated-at label shown in the h1-meta line. */
  generatedAt?: string;
  /** Note appended to the h1-meta line about the title provenance. */
  titleNote?: string;
}

export function SummaryArticle({ markdown, providerLabel, generatedAt, titleNote }: SummaryArticleProps) {
  const lines = markdown.replace(/\r\n/g, '\n').split('\n');
  const nodes: React.ReactNode[] = [];

  let listItems: React.ReactNode[] = [];
  let paragraph: string[] = [];
  let seenH1 = false;
  let key = 0;

  const flushList = () => {
    if (listItems.length) {
      nodes.push(<ul key={`ul-${key++}`}>{listItems}</ul>);
      listItems = [];
    }
  };
  const flushParagraph = () => {
    if (paragraph.length) {
      const text = paragraph.join(' ');
      nodes.push(<p key={`p-${key++}`}>{inline(text, `p-${key}`)}</p>);
      paragraph = [];
    }
  };
  const flushAll = () => {
    flushList();
    flushParagraph();
  };

  for (const raw of lines) {
    const line = raw.trimEnd();
    const trimmed = line.trim();

    if (!trimmed) {
      flushAll();
      continue;
    }

    const h1 = trimmed.match(/^#\s+(.*)$/);
    if (h1 && !seenH1) {
      flushAll();
      seenH1 = true;
      nodes.push(
        <React.Fragment key={`h1-${key++}`}>
          <h1>{h1[1]}</h1>
          {(providerLabel || generatedAt || titleNote) && (
            <div className="h1-meta">
              {[providerLabel, generatedAt, titleNote].filter(Boolean).join(' · ')}
            </div>
          )}
        </React.Fragment>
      );
      continue;
    }

    const h2 = trimmed.match(/^##\s+(.*)$/);
    if (h2) {
      flushAll();
      nodes.push(<h2 key={`h2-${key++}`}>{h2[1]}</h2>);
      continue;
    }

    const task = trimmed.match(/^[-*]\s+\[( |x|X)\]\s+(.*)$/);
    if (task) {
      flushParagraph();
      listItems.length && flushList();
      nodes.push(renderTask(task[2], task[1].toLowerCase() === 'x', `task-${key++}`));
      continue;
    }

    const li = trimmed.match(/^[-*]\s+(.*)$/);
    if (li) {
      flushParagraph();
      listItems.push(<li key={`li-${key++}`}>{inline(li[1], `li-${key}`)}</li>);
      continue;
    }

    // Plain prose accumulates into a paragraph.
    flushList();
    paragraph.push(trimmed.replace(/^#+\s*/, ''));
  }
  flushAll();

  return <article className="md">{nodes}</article>;
}
