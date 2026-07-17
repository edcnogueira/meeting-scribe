'use client';

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { toast } from 'sonner';
import { Pencil, Check, X, Trash2, Merge } from 'lucide-react';
import { SpeakerChip } from '@/components/SpeakerChip';

interface SpeakerIdentityInfo {
  id: string;
  name: string;
  sample_count: number;
  is_self: boolean;
  has_embedding: boolean;
}

/**
 * Registry of enrolled people (task D5): list, rename, merge, and delete voice
 * profiles. Deleting removes the biometric embedding and is confirmed.
 */
export function SpeakerIdentityManager() {
  const [identities, setIdentities] = useState<SpeakerIdentityInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  const [mergeSource, setMergeSource] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await invoke<SpeakerIdentityInfo[]>('api_list_speaker_identities');
      setIdentities(list);
    } catch (err) {
      console.error('Failed to load identities:', err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const saveRename = useCallback(
    async (speakerId: string) => {
      const name = editValue.trim();
      if (!name) {
        setEditing(null);
        return;
      }
      try {
        await invoke('api_rename_speaker_identity', { speakerId, name });
        toast.success('Person renamed');
        setEditing(null);
        await load();
      } catch (err) {
        toast.error('Rename failed', { description: String(err) });
      }
    },
    [editValue, load]
  );

  const remove = useCallback(
    async (identity: SpeakerIdentityInfo) => {
      const ok = window.confirm(
        `Delete "${identity.name}"? This removes their stored voice profile (biometric embedding). Past transcript labels are kept.`
      );
      if (!ok) return;
      try {
        await invoke('api_delete_speaker_identity', { speakerId: identity.id });
        toast.success(`Deleted ${identity.name}`);
        await load();
      } catch (err) {
        toast.error('Delete failed', { description: String(err) });
      }
    },
    [load]
  );

  const merge = useCallback(
    async (sourceId: string, targetId: string) => {
      const source = identities.find((i) => i.id === sourceId);
      const target = identities.find((i) => i.id === targetId);
      if (!source || !target) return;
      const ok = window.confirm(
        `Merge "${source.name}" into "${target.name}"? "${source.name}" will be removed and its voice samples folded into "${target.name}".`
      );
      if (!ok) {
        setMergeSource(null);
        return;
      }
      try {
        await invoke('api_merge_speaker_identities', { targetId, sourceId });
        toast.success('People merged');
        setMergeSource(null);
        await load();
      } catch (err) {
        toast.error('Merge failed', { description: String(err) });
      }
    },
    [identities, load]
  );

  if (loading) {
    return <p className="text-sm text-gray-400">Loading people...</p>;
  }

  if (identities.length === 0) {
    return (
      <p className="text-sm text-gray-500">
        No people enrolled yet. Rename a detected speaker in a meeting to enroll them.
      </p>
    );
  }

  return (
    <ul className="space-y-2">
      {identities.map((identity) => (
        <li
          key={identity.id}
          className="flex items-center gap-2 rounded-md border border-gray-200 px-3 py-2"
        >
          {editing === identity.id ? (
            <div className="flex items-center gap-1 flex-1 min-w-0">
              <input
                autoFocus
                value={editValue}
                onChange={(e) => setEditValue(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') saveRename(identity.id);
                  if (e.key === 'Escape') setEditing(null);
                }}
                className="flex-1 min-w-0 px-2 py-1 text-sm border border-gray-300 rounded focus:outline-none focus:ring-1 focus:ring-blue-500"
              />
              <button
                type="button"
                onClick={() => saveRename(identity.id)}
                className="p-1 text-green-600 hover:bg-green-50 rounded"
                title="Save"
              >
                <Check className="w-4 h-4" />
              </button>
              <button
                type="button"
                onClick={() => setEditing(null)}
                className="p-1 text-gray-400 hover:bg-gray-100 rounded"
                title="Cancel"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
          ) : (
            <>
              <SpeakerChip speaker={identity.name} />
              {identity.is_self && <span className="text-[10px] text-gray-400">you</span>}
              <span className="text-xs text-gray-400">
                {identity.sample_count} sample{identity.sample_count === 1 ? '' : 's'}
              </span>

              <div className="ml-auto flex items-center gap-1">
                {mergeSource === identity.id ? (
                  <select
                    autoFocus
                    defaultValue=""
                    onChange={(e) => e.target.value && merge(identity.id, e.target.value)}
                    onBlur={() => setMergeSource(null)}
                    className="text-xs border border-gray-300 rounded px-1 py-0.5"
                  >
                    <option value="" disabled>
                      Merge into...
                    </option>
                    {identities
                      .filter((o) => o.id !== identity.id)
                      .map((o) => (
                        <option key={o.id} value={o.id}>
                          {o.name}
                        </option>
                      ))}
                  </select>
                ) : (
                  <button
                    type="button"
                    onClick={() => setMergeSource(identity.id)}
                    disabled={identities.length < 2}
                    className="p-1 text-gray-400 hover:text-gray-600 hover:bg-gray-100 rounded disabled:opacity-30"
                    title="Merge into another person"
                  >
                    <Merge className="w-3.5 h-3.5" />
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => {
                    setEditing(identity.id);
                    setEditValue(identity.name);
                  }}
                  className="p-1 text-gray-400 hover:text-gray-600 hover:bg-gray-100 rounded"
                  title="Rename"
                >
                  <Pencil className="w-3.5 h-3.5" />
                </button>
                <button
                  type="button"
                  onClick={() => remove(identity)}
                  className="p-1 text-gray-400 hover:text-red-600 hover:bg-red-50 rounded"
                  title="Delete voice profile"
                >
                  <Trash2 className="w-3.5 h-3.5" />
                </button>
              </div>
            </>
          )}
        </li>
      ))}
    </ul>
  );
}
