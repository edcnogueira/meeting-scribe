'use client';

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useRouter, usePathname } from 'next/navigation';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { useSidebar } from '@/components/Sidebar/SidebarProvider';
import type { SidebarItem, FolderNode } from '@/components/Sidebar/SidebarProvider';
import { useShell } from './ShellContext';
import { ThemeToggle } from './ThemeToggle';
import { ChevIcon, DocIcon, FolderIcon, GearIcon, PanelIcon, RefreshIcon } from './icons';

const EXPANDED_FOLDERS_KEY = 'meetily.sidebar.expandedFolders';

/** Auto-generated provisional titles (see useRecordingStart.generateMeetingTitle). */
const PENDING_TITLE_RE = /^Meeting \d{2}_\d{2}_\d{2}_\d{2}_\d{2}_\d{2}$/;

function isPendingTitle(title: string): boolean {
  return PENDING_TITLE_RE.test(title.trim());
}

function meetingRoute(id: string): string {
  return id.includes('-') ? `/meeting-details?id=${id}` : `/notes/${id}`;
}

interface ContextMenuState {
  x: number;
  y: number;
  item: SidebarItem;
}

/**
 * Shared app shell sidebar for the redesign (task R1): 264px folder tree that
 * mirrors the on-disk meetings folder (O1), a provisional-title dot (O2),
 * per-node context menu, inline rename, move-to-folder modal, sync/refresh, and
 * a footer (settings, theme toggle, collapse). Wired to the existing
 * SidebarProvider / folder APIs — this reskins real functionality.
 */
export function ShellSidebar() {
  const router = useRouter();
  const pathname = usePathname();
  const {
    sidebarItems,
    handleRecordingToggle,
    refreshFolderTree,
    createFolder,
    renameFolder,
    deleteFolder,
    moveMeeting,
    folderTree,
    setMeetings,
    meetings,
  } = useSidebar();
  const { hideSidebar } = useShell();

  const [expanded, setExpanded] = useState<Set<string>>(() => {
    if (typeof window === 'undefined') return new Set(['meetings']);
    try {
      const stored = window.localStorage.getItem(EXPANDED_FOLDERS_KEY);
      if (stored) return new Set(['meetings', ...(JSON.parse(stored) as string[])]);
    } catch {
      /* ignore */
    }
    return new Set(['meetings']);
  });

  useEffect(() => {
    try {
      window.localStorage.setItem(EXPANDED_FOLDERS_KEY, JSON.stringify(Array.from(expanded)));
    } catch {
      /* ignore */
    }
  }, [expanded]);

  // Active meeting id, derived from the current route (path + ?id= query).
  const [activeId, setActiveId] = useState<string | null>(null);
  useEffect(() => {
    if (typeof window === 'undefined') return;
    if (pathname?.startsWith('/meeting-details')) {
      setActiveId(new URLSearchParams(window.location.search).get('id'));
    } else if (pathname?.startsWith('/notes/')) {
      setActiveId(pathname.split('/notes/')[1] ?? null);
    } else {
      setActiveId(null);
    }
  }, [pathname]);

  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [moveItem, setMoveItem] = useState<SidebarItem | null>(null);
  const [moveTarget, setMoveTarget] = useState<string | null>(null);
  const [newFolderParent, setNewFolderParent] = useState<{ path: string | null } | null>(null);
  const [newFolderName, setNewFolderName] = useState('');
  const [deleteItem, setDeleteItem] = useState<SidebarItem | null>(null);
  const [syncing, setSyncing] = useState(false);

  // Close the context menu on any outside interaction or Escape.
  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setContextMenu(null);
    };
    window.addEventListener('click', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('keydown', onKey);
    };
  }, [contextMenu]);

  const toggleFolder = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const openContextMenu = useCallback((e: React.MouseEvent, item: SidebarItem) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, item });
  }, []);

  const handleSync = useCallback(async () => {
    setSyncing(true);
    try {
      await refreshFolderTree();
    } finally {
      window.setTimeout(() => setSyncing(false), 700);
    }
  }, [refreshFolderTree]);

  // ---- rename (meeting title via backend, folder via O1 API) ----
  const commitRename = useCallback(
    async (item: SidebarItem, rawValue: string) => {
      const value = rawValue.trim();
      setRenamingId(null);
      if (!value || value === item.title) return;
      try {
        if (item.type === 'folder') {
          if (item.path) await renameFolder(item.path, value);
        } else {
          await invoke('api_save_meeting_title', { meetingId: item.id, title: value });
          setMeetings(meetings.map((m) => (m.id === item.id ? { ...m, title: value } : m)));
          await refreshFolderTree();
        }
        toast.success('Renamed');
      } catch (error) {
        toast.error('Failed to rename', {
          description: error instanceof Error ? error.message : String(error),
        });
      }
    },
    [renameFolder, refreshFolderTree, setMeetings, meetings]
  );

  const confirmDelete = useCallback(async () => {
    const item = deleteItem;
    if (!item) return;
    setDeleteItem(null);
    try {
      if (item.type === 'folder') {
        if (item.path) await deleteFolder(item.path);
        toast.success('Folder deleted');
      } else {
        await invoke('api_delete_meeting', { meetingId: item.id });
        setMeetings(meetings.filter((m) => m.id !== item.id));
        await refreshFolderTree();
        toast.success('Meeting deleted');
        if (activeId === item.id) router.push('/');
      }
    } catch (error) {
      toast.error('Failed to delete', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  }, [deleteItem, deleteFolder, refreshFolderTree, setMeetings, meetings, activeId, router]);

  const confirmNewFolder = useCallback(async () => {
    if (!newFolderParent) return;
    const name = newFolderName.trim();
    if (!name) {
      toast.error('Folder name cannot be empty');
      return;
    }
    try {
      await createFolder(newFolderParent.path, name);
      toast.success('Folder created');
      setNewFolderParent(null);
      setNewFolderName('');
    } catch (error) {
      toast.error('Failed to create folder', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  }, [newFolderParent, newFolderName, createFolder]);

  const confirmMove = useCallback(async () => {
    if (!moveItem) return;
    try {
      await moveMeeting(moveItem.id, moveTarget);
      toast.success('Meeting moved');
      setMoveItem(null);
      setMoveTarget(null);
    } catch (error) {
      toast.error('Failed to move meeting', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  }, [moveItem, moveTarget, moveMeeting]);

  const revealInFinder = useCallback(async (item: SidebarItem) => {
    try {
      await invoke('api_reveal_meeting_in_finder', { meetingId: item.id });
    } catch {
      toast.message('Reveal in Finder is not available yet.');
    }
  }, []);

  // Flatten folder tree into move-to targets (root + org folders).
  const moveTargets = useMemo(() => {
    const targets: { path: string | null; label: string; depth: number }[] = [
      { path: null, label: 'Sem pasta (raiz)', depth: 0 },
    ];
    const walk = (nodes: FolderNode[], depth: number) => {
      for (const n of nodes) {
        targets.push({ path: n.path, label: n.name, depth });
        walk(n.folders, depth + 1);
      }
    };
    if (folderTree) walk(folderTree.folders, 0);
    return targets;
  }, [folderTree]);

  // The provider wraps everything under a synthetic 'meetings' root; render its
  // children as the top-level tree entries beneath the "Reuniões" label.
  const rootChildren = useMemo(() => {
    const root = sidebarItems.find((it) => it.id === 'meetings');
    return root?.children ?? [];
  }, [sidebarItems]);

  const renderNode = (item: SidebarItem, depth: number): React.ReactNode => {
    if (item.type === 'folder') {
      const isOpen = expanded.has(item.id);
      return (
        <div key={item.id} className={`tree-group${isOpen ? ' open' : ''}`} data-id={item.id}>
          <button
            type="button"
            className="tree-item"
            onClick={() => toggleFolder(item.id)}
            onContextMenu={(e) => openContextMenu(e, item)}
          >
            <span className="chev"><ChevIcon /></span>
            <span className="t-ico"><FolderIcon /></span>
            {renamingId === item.id ? (
              <RenameInput initial={item.title} onCommit={(v) => commitRename(item, v)} onCancel={() => setRenamingId(null)} />
            ) : (
              <span className="t-name">{item.title}</span>
            )}
          </button>
          <div className="tree-children">
            {(item.children ?? []).map((child) => renderNode(child, depth + 1))}
          </div>
        </div>
      );
    }

    const pending = isPendingTitle(item.title);
    const isActive = item.id === activeId;
    return (
      <a
        key={item.id}
        href={meetingRoute(item.id)}
        className={`tree-item${isActive ? ' active' : ''}`}
        onClick={(e) => {
          e.preventDefault();
          if (renamingId === item.id) return;
          router.push(meetingRoute(item.id));
        }}
        onContextMenu={(e) => openContextMenu(e, item)}
      >
        <span className="t-ico"><DocIcon /></span>
        {renamingId === item.id ? (
          <RenameInput initial={item.title} onCommit={(v) => commitRename(item, v)} onCancel={() => setRenamingId(null)} />
        ) : (
          <>
            <span className={`t-name${pending ? ' placeholder' : ''}`}>{item.title}</span>
            {pending && <span className="pending-dot" data-tip="Aguardando primeiro resumo" />}
          </>
        )}
      </a>
    );
  };

  return (
    <aside className="sidebar" id="sidebar">
      <div className="titlebar">
        <div className="traffic"><i /><i /><i /></div>
      </div>

      <div className="sidebar-head">
        <button type="button" className="btn-record" onClick={handleRecordingToggle}>
          <span className="dot" />
          Nova reunião
        </button>
      </div>

      <div className="tree-label">
        <span>Reuniões</span>
        <button
          type="button"
          className="icon-btn"
          style={{ width: 22, height: 22 }}
          data-tip="Sincronizar com o Finder"
          aria-label="Sincronizar"
          onClick={handleSync}
        >
          <span style={syncing ? { animation: 'spin 0.7s linear infinite', display: 'grid', placeItems: 'center' } : undefined}>
            <RefreshIcon />
          </span>
        </button>
      </div>

      <nav className="tree" aria-label="Pastas de reuniões">
        {rootChildren.map((item) => renderNode(item, 0))}
      </nav>

      <div className="sidebar-foot">
        <button
          type="button"
          className="icon-btn"
          data-tip="Configurações"
          aria-label="Configurações"
          onClick={() => router.push('/settings')}
        >
          <GearIcon />
        </button>
        <ThemeToggle />
        <span className="spacer" />
        <button
          type="button"
          className="icon-btn"
          data-tip="Recolher barra lateral"
          aria-label="Recolher barra lateral"
          onClick={hideSidebar}
        >
          <PanelIcon />
        </button>
      </div>

      {contextMenu && (
        <ContextMenu
          state={contextMenu}
          onClose={() => setContextMenu(null)}
          onNewFolder={(parentPath) => setNewFolderParent({ path: parentPath })}
          onRename={(item) => setRenamingId(item.id)}
          onMove={(item) => { setMoveItem(item); setMoveTarget(item.parentFolderPath ?? null); }}
          onReveal={revealInFinder}
          onDelete={(item) => setDeleteItem(item)}
        />
      )}

      {/* New folder modal */}
      <div className={`overlay${newFolderParent ? ' open' : ''}`} onClick={(e) => { if (e.target === e.currentTarget) setNewFolderParent(null); }}>
        {newFolderParent && (
          <div className="modal" role="dialog" aria-label="Nova pasta">
            <div className="modal-head"><h3>Nova pasta</h3></div>
            <div className="modal-body">
              <div className="field">
                <label htmlFor="new-folder-name">Nome da pasta</label>
                <input
                  id="new-folder-name"
                  className="input"
                  autoFocus
                  value={newFolderName}
                  onChange={(e) => setNewFolderName(e.target.value)}
                  onKeyDown={(e) => { if (e.key === 'Enter') confirmNewFolder(); }}
                />
              </div>
            </div>
            <div className="modal-foot">
              <button type="button" className="btn ghost" onClick={() => { setNewFolderParent(null); setNewFolderName(''); }}>Cancelar</button>
              <button type="button" className="btn primary" onClick={confirmNewFolder}>Criar</button>
            </div>
          </div>
        )}
      </div>

      {/* Move to folder modal */}
      <div className={`overlay${moveItem ? ' open' : ''}`} onClick={(e) => { if (e.target === e.currentTarget) setMoveItem(null); }}>
        {moveItem && (
          <div className="modal" role="dialog" aria-label="Mover para pasta">
            <div className="modal-head">
              <h3>Mover para pasta</h3>
              <p>A reunião será movida no disco — a mudança aparece também no Finder.</p>
            </div>
            <div className="modal-body">
              <div className="pick-tree">
                {moveTargets.map((t) => (
                  <label key={t.path ?? '__root__'} className={t.depth > 0 ? 'indent-1' : undefined}>
                    <input
                      type="radio"
                      name="mv"
                      checked={moveTarget === t.path}
                      onChange={() => setMoveTarget(t.path)}
                    />
                    <FolderIcon />
                    {t.label}
                  </label>
                ))}
              </div>
            </div>
            <div className="modal-foot">
              <button type="button" className="btn ghost" onClick={() => setMoveItem(null)}>Cancelar</button>
              <button type="button" className="btn primary" onClick={confirmMove}>Mover</button>
            </div>
          </div>
        )}
      </div>

      {/* Delete confirmation modal */}
      <div className={`overlay${deleteItem ? ' open' : ''}`} onClick={(e) => { if (e.target === e.currentTarget) setDeleteItem(null); }}>
        {deleteItem && (
          <div className="modal" role="dialog" aria-label="Excluir">
            <div className="modal-head">
              <h3>{deleteItem.type === 'folder' ? 'Excluir pasta?' : 'Excluir reunião?'}</h3>
              <p>
                {deleteItem.type === 'folder'
                  ? `A pasta “${deleteItem.title}” será removida do disco.`
                  : `A reunião “${deleteItem.title}” e seus dados serão removidos permanentemente.`}
              </p>
            </div>
            <div className="modal-foot">
              <button type="button" className="btn ghost" onClick={() => setDeleteItem(null)}>Cancelar</button>
              <button type="button" className="btn danger" onClick={confirmDelete}>Excluir</button>
            </div>
          </div>
        )}
      </div>
    </aside>
  );
}

/** Inline rename input: Enter confirms, Esc cancels, blur confirms. */
function RenameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const ref = useRef<HTMLInputElement>(null);
  const done = useRef(false);

  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);

  return (
    <input
      ref={ref}
      className="input rename-input"
      value={value}
      aria-label="Renomear"
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => { e.preventDefault(); e.stopPropagation(); }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') { done.current = true; onCommit(value); }
        if (e.key === 'Escape') { done.current = true; onCancel(); }
      }}
      onBlur={() => { if (!done.current) onCommit(value); }}
    />
  );
}

/** Positioned context menu for a folder or meeting node. */
function ContextMenu({
  state,
  onClose,
  onNewFolder,
  onRename,
  onMove,
  onReveal,
  onDelete,
}: {
  state: ContextMenuState;
  onClose: () => void;
  onNewFolder: (parentPath: string | null) => void;
  onRename: (item: SidebarItem) => void;
  onMove: (item: SidebarItem) => void;
  onReveal: (item: SidebarItem) => void;
  onDelete: (item: SidebarItem) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: state.x, top: state.y });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({
      left: Math.min(state.x, window.innerWidth - r.width - 10),
      top: Math.min(state.y, window.innerHeight - r.height - 10),
    });
  }, [state.x, state.y]);

  const { item } = state;
  const isFolder = item.type === 'folder';
  const childCount = item.children?.length ?? 0;
  const canDelete = !isFolder || childCount === 0;

  return (
    <div
      ref={ref}
      className="ctx"
      style={{ left: pos.left, top: pos.top }}
      onClick={(e) => e.stopPropagation()}
    >
      {isFolder ? (
        <>
          <button type="button" onClick={() => { onNewFolder(item.path ?? null); onClose(); }}>Nova pasta</button>
          <button type="button" onClick={() => { onRename(item); onClose(); }}>Renomear</button>
          <hr />
          {canDelete ? (
            <button type="button" className="destructive" onClick={() => { onDelete(item); onClose(); }}>Excluir pasta</button>
          ) : (
            <button type="button" disabled>
              Excluir pasta
              <span className="why">Disponível apenas para pastas vazias — mova as {childCount} reuniões antes.</span>
            </button>
          )}
        </>
      ) : (
        <>
          <button type="button" onClick={() => { onRename(item); onClose(); }}>Renomear</button>
          <button type="button" onClick={() => { onMove(item); onClose(); }}>Mover para pasta…</button>
          <hr />
          <button type="button" onClick={() => { onReveal(item); onClose(); }}>Revelar no Finder</button>
          <button type="button" className="destructive" onClick={() => { onDelete(item); onClose(); }}>Excluir reunião</button>
        </>
      )}
    </div>
  );
}
