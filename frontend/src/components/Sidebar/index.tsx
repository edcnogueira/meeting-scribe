'use client';

import React, { useState, useMemo, useEffect, useCallback } from 'react';
import { ChevronDown, ChevronRight, File, Settings, ChevronLeftCircle, ChevronRightCircle, Calendar, StickyNote, Home, Trash2, Mic, Square, Plus, Search, Pencil, NotebookPen, SearchIcon, X, Upload, Folder, FolderOpen, FolderPlus, FolderInput, RefreshCw, AlertTriangle, Inbox } from 'lucide-react';
import { useRouter, usePathname } from 'next/navigation';
import { useSidebar } from './SidebarProvider';
import type { CurrentMeeting, SidebarItem, FolderNode } from '@/components/Sidebar/SidebarProvider';
import { ConfirmationModal } from '../ConfirmationModel/confirmation-modal';
import { ModelConfig } from '@/components/ModelSettingsModal';
import { SettingTabs } from '../SettingTabs';
import { TranscriptModelProps } from '@/components/TranscriptSettings';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/components/ui/tooltip';
import { toast } from 'sonner';
import { useRecordingState } from '@/contexts/RecordingStateContext';
import { useImportDialog } from '@/contexts/ImportDialogContext';
import { useConfig } from '@/contexts/ConfigContext';

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog"
import { VisuallyHidden } from "@/components/ui/visually-hidden"

import { MessageToast } from '../MessageToast';
import Logo from '../Logo';
import Info from '../Info';
import { ComplianceNotification } from '../ComplianceNotification';
import { Input } from '../ui/input';
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from '../ui/input-group';

const Sidebar: React.FC = () => {
  const router = useRouter();
  const pathname = usePathname();
  const {
    currentMeeting,
    setCurrentMeeting,
    sidebarItems,
    isCollapsed,
    toggleCollapse,
    handleRecordingToggle,
    searchTranscripts,
    searchResults,
    isSearching,
    meetings,
    setMeetings,
    serverAddress,
    folderTree,
    refreshFolderTree,
    createFolder,
    renameFolder,
    deleteFolder,
    moveMeeting,
  } = useSidebar();

  // Get recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();
  const { openImportDialog } = useImportDialog();
  const { betaFeatures } = useConfig();
  const EXPANDED_FOLDERS_KEY = 'meetily.sidebar.expandedFolders';
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(() => {
    // Persisted expand/collapse state (O1). 'meetings' root is always included.
    if (typeof window === 'undefined') return new Set(['meetings']);
    try {
      const stored = window.localStorage.getItem(EXPANDED_FOLDERS_KEY);
      if (stored) {
        const ids = JSON.parse(stored) as string[];
        return new Set(['meetings', ...ids]);
      }
    } catch {
      /* ignore malformed storage */
    }
    return new Set(['meetings']);
  });

  // Persist expand/collapse state whenever it changes.
  useEffect(() => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(EXPANDED_FOLDERS_KEY, JSON.stringify(Array.from(expandedFolders)));
    } catch {
      /* ignore quota/serialization errors */
    }
  }, [expandedFolders]);

  // O1 folder-management UI state
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; item: SidebarItem } | null>(null);
  const [folderDialog, setFolderDialog] = useState<{ open: boolean; mode: 'create' | 'rename'; parentPath: string | null; path: string | null; name: string }>(
    { open: false, mode: 'create', parentPath: null, path: null, name: '' }
  );
  const [deleteFolderState, setDeleteFolderState] = useState<{ open: boolean; path: string | null; title: string }>(
    { open: false, path: null, title: '' }
  );
  const [moveDialog, setMoveDialog] = useState<{ open: boolean; meetingId: string | null; title: string; targetPath: string | null }>(
    { open: false, meetingId: null, title: '', targetPath: null }
  );
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [showModelSettings, setShowModelSettings] = useState(false);
  const [modelConfig, setModelConfig] = useState<ModelConfig>({
    provider: 'ollama',
    model: '',
    whisperModel: '',
    apiKey: null,
    ollamaEndpoint: null
  });
  const [transcriptModelConfig, setTranscriptModelConfig] = useState<TranscriptModelProps>({
    provider: 'parakeet',
    model: 'parakeet-tdt-0.6b-v3-int8',
  });
  const [settingsSaveSuccess, setSettingsSaveSuccess] = useState<boolean | null>(null);

  // State for edit modal
  const [editModalState, setEditModalState] = useState<{ isOpen: boolean; meetingId: string | null; currentTitle: string }>({
    isOpen: false,
    meetingId: null,
    currentTitle: ''
  });
  const [editingTitle, setEditingTitle] = useState<string>('');

  // Ensure 'meetings' folder is always expanded
  useEffect(() => {
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders]);

  // useEffect(() => {
  //   if (settingsSaveSuccess !== null) {
  //     const timer = setTimeout(() => {
  //       setSettingsSaveSuccess(null);
  //     }, 3000);
  //   }
  // }, [settingsSaveSuccess]);


  const [deleteModalState, setDeleteModalState] = useState<{ isOpen: boolean; itemId: string | null }>({ isOpen: false, itemId: null });

  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchModelConfig = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching model config');
        return;
      }

      try {
        const data = await invoke('api_get_model_config') as any;
        if (data && data.provider !== null) {
          // Fetch API key if not included and provider requires it
          if (data.provider !== 'ollama' && !data.apiKey) {
            try {
              const apiKeyData = await invoke('api_get_api_key', {
                provider: data.provider
              }) as string;
              data.apiKey = apiKeyData;
            } catch (err) {
              console.error('Failed to fetch API key:', err);
            }
          }
          setModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch model config:', error);
      }
    };

    fetchModelConfig();
  }, [serverAddress]);


  useEffect(() => {
    // Note: Don't set hardcoded defaults - let DB be the source of truth
    const fetchTranscriptSettings = async () => {
      // Only make API call if serverAddress is loaded
      if (!serverAddress) {
        console.log('Waiting for server address to load before fetching transcript settings');
        return;
      }

      try {
        const data = await invoke('api_get_transcript_config') as any;
        if (data && data.provider !== null) {
          setTranscriptModelConfig(data);
        }
      } catch (error) {
        console.error('Failed to fetch transcript settings:', error);
      }
    };
    fetchTranscriptSettings();
  }, [serverAddress]);

  // Listen for model config updates from other components
  useEffect(() => {
    const setupListener = async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const unlisten = await listen<ModelConfig>('model-config-updated', (event) => {
        console.log('Sidebar received model-config-updated event:', event.payload);
        setModelConfig(event.payload);
      });

      return unlisten;
    };

    let cleanup: (() => void) | undefined;
    setupListener().then(fn => cleanup = fn);

    return () => {
      cleanup?.();
    };
  }, []);



  // Handle model config save
  const handleSaveModelConfig = async (config: ModelConfig) => {
    try {
      await invoke('api_save_model_config', {
        provider: config.provider,
        model: config.model,
        whisperModel: config.whisperModel,
        apiKey: config.apiKey,
        ollamaEndpoint: config.ollamaEndpoint,
      });

      setModelConfig(config);
      console.log('Model config saved successfully');
      setSettingsSaveSuccess(true);

      // Emit event to sync other components
      const { emit } = await import('@tauri-apps/api/event');
      await emit('model-config-updated', config);

      // Track settings change
      await Analytics.trackSettingsChanged('model_config', `${config.provider}_${config.model}`);
    } catch (error) {
      console.error('Error saving model config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  const handleSaveTranscriptConfig = async (updatedConfig?: TranscriptModelProps) => {
    try {
      const configToSave = updatedConfig || transcriptModelConfig;
      const payload = {
        provider: configToSave.provider,
        model: configToSave.model,
        apiKey: configToSave.apiKey ?? null
      };
      console.log('Saving transcript config with payload:', payload);

      await invoke('api_save_transcript_config', {
        provider: payload.provider,
        model: payload.model,
        apiKey: payload.apiKey,
      });


      setSettingsSaveSuccess(true);

      // Track settings change
      const transcriptConfigToSave = updatedConfig || transcriptModelConfig;
      await Analytics.trackSettingsChanged('transcript_config', `${transcriptConfigToSave.provider}_${transcriptConfigToSave.model}`);
    } catch (error) {
      console.error('Failed to save transcript config:', error);
      setSettingsSaveSuccess(false);
    }
  };

  // Handle search input changes
  const handleSearchChange = useCallback(async (value: string) => {
    setSearchQuery(value);

    // If search query is empty, just return to normal view
    if (!value.trim()) return;

    // Search through transcripts
    await searchTranscripts(value);

    // Make sure the meetings folder is expanded when searching
    if (!expandedFolders.has('meetings')) {
      const newExpanded = new Set(expandedFolders);
      newExpanded.add('meetings');
      setExpandedFolders(newExpanded);
    }
  }, [expandedFolders, searchTranscripts]);

  // Combine search results with sidebar items
  const filteredSidebarItems = useMemo(() => {
    if (!searchQuery.trim()) return sidebarItems;

    // If we have search results, highlight matching meetings
    if (searchResults.length > 0) {
      // Get the IDs of meetings that matched in transcripts
      const matchedMeetingIds = new Set(searchResults.map(result => result.id));

      return sidebarItems
        .map(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search results or title match
            const filteredChildren = folder.children.filter(item => {
              // Include if the meeting ID is in our search results
              if (matchedMeetingIds.has(item.id)) return true;

              // Or if the title matches the search query
              return item.title.toLowerCase().includes(searchQuery.toLowerCase());
            });

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return (matchedMeetingIds.has(folder.id) ||
            folder.title.toLowerCase().includes(searchQuery.toLowerCase()))
            ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    } else {
      // Fall back to title-only filtering if no transcript results
      return sidebarItems
        .map(folder => {
          // Always include folders in the results
          if (folder.type === 'folder') {
            if (!folder.children) return folder;

            // Filter children based on search query
            const filteredChildren = folder.children.filter(item =>
              item.title.toLowerCase().includes(searchQuery.toLowerCase())
            );

            return {
              ...folder,
              children: filteredChildren
            };
          }

          // For non-folder items, check if they match the search
          return folder.title.toLowerCase().includes(searchQuery.toLowerCase()) ? folder : undefined;
        })
        .filter((item): item is SidebarItem => item !== undefined); // Type-safe filter
    }
  }, [sidebarItems, searchQuery, searchResults, expandedFolders]);


  const handleDelete = async (itemId: string) => {
    console.log('Deleting item:', itemId);
    const payload = {
      meetingId: itemId
    };

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('api_delete_meeting', {
        meetingId: itemId,
      });
      console.log('Meeting deleted successfully');
      const updatedMeetings = meetings.filter((m: CurrentMeeting) => m.id !== itemId);
      setMeetings(updatedMeetings);
      // Keep the on-disk folder tree in sync.
      refreshFolderTree();

      // Track meeting deletion
      Analytics.trackMeetingDeleted(itemId);

      // Show success toast
      toast.success("Meeting deleted successfully", {
        description: "All associated data has been removed"
      });

      // If deleting the active meeting, navigate to home
      if (currentMeeting?.id === itemId) {
        setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
        router.push('/');
      }
    } catch (error) {
      console.error('Failed to delete meeting:', error);
      toast.error("Failed to delete meeting", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleDeleteConfirm = () => {
    if (deleteModalState.itemId) {
      handleDelete(deleteModalState.itemId);
    }
    setDeleteModalState({ isOpen: false, itemId: null });
  };

  // Handle modal editing of meeting names
  const handleEditStart = (meetingId: string, currentTitle: string) => {
    setEditModalState({
      isOpen: true,
      meetingId: meetingId,
      currentTitle: currentTitle
    });
    setEditingTitle(currentTitle);
  };

  const handleEditConfirm = async () => {
    const newTitle = editingTitle.trim();
    const meetingId = editModalState.meetingId;

    if (!meetingId) return;

    // Prevent empty titles
    if (!newTitle) {
      toast.error("Meeting title cannot be empty");
      return;
    }

    try {
      await invoke('api_save_meeting_title', {
        meetingId: meetingId,
        title: newTitle,
      });

      // Update local state
      const updatedMeetings = meetings.map((m: CurrentMeeting) =>
        m.id === meetingId ? { ...m, title: newTitle } : m
      );
      setMeetings(updatedMeetings);

      // Update current meeting if it's the one being edited
      if (currentMeeting?.id === meetingId) {
        setCurrentMeeting({ id: meetingId, title: newTitle });
      }
      // Reflect the new title in the folder tree.
      refreshFolderTree();

      // Track the edit
      Analytics.trackButtonClick('edit_meeting_title', 'sidebar');

      toast.success("Meeting title updated successfully");

      // Close modal and reset state
      setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
      setEditingTitle('');
    } catch (error) {
      console.error('Failed to update meeting title:', error);
      toast.error("Failed to update meeting title", {
        description: error instanceof Error ? error.message : String(error)
      });
    }
  };

  const handleEditCancel = () => {
    setEditModalState({ isOpen: false, meetingId: null, currentTitle: '' });
    setEditingTitle('');
  };

  const toggleFolder = (folderId: string) => {
    // Normal toggle behavior for all folders
    const newExpanded = new Set(expandedFolders);
    if (newExpanded.has(folderId)) {
      newExpanded.delete(folderId);
    } else {
      newExpanded.add(folderId);
    }
    setExpandedFolders(newExpanded);
  };

  // ---- O1: folder-management actions ----

  // Close the context menu on any outside click or Escape.
  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setContextMenu(null); };
    window.addEventListener('click', close);
    window.addEventListener('contextmenu', close);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('contextmenu', close);
      window.removeEventListener('keydown', onKey);
    };
  }, [contextMenu]);

  const openContextMenu = (e: React.MouseEvent, item: SidebarItem) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({ x: e.clientX, y: e.clientY, item });
  };

  const openNewFolderDialog = (parentPath: string | null) => {
    setFolderDialog({ open: true, mode: 'create', parentPath, path: null, name: '' });
  };

  const openRenameFolderDialog = (item: SidebarItem) => {
    setFolderDialog({ open: true, mode: 'rename', parentPath: null, path: item.path ?? null, name: item.title });
  };

  const handleFolderDialogConfirm = async () => {
    const name = folderDialog.name.trim();
    if (!name) {
      toast.error('Folder name cannot be empty');
      return;
    }
    try {
      if (folderDialog.mode === 'create') {
        await createFolder(folderDialog.parentPath, name);
        toast.success('Folder created');
      } else if (folderDialog.path) {
        await renameFolder(folderDialog.path, name);
        toast.success('Folder renamed');
      }
      setFolderDialog({ open: false, mode: 'create', parentPath: null, path: null, name: '' });
    } catch (error) {
      toast.error(folderDialog.mode === 'create' ? 'Failed to create folder' : 'Failed to rename folder', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };

  const handleDeleteFolderConfirm = async () => {
    if (!deleteFolderState.path) return;
    try {
      await deleteFolder(deleteFolderState.path);
      toast.success('Folder deleted');
    } catch (error) {
      toast.error('Failed to delete folder', {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setDeleteFolderState({ open: false, path: null, title: '' });
    }
  };

  const handleMoveConfirm = async () => {
    if (!moveDialog.meetingId) return;
    try {
      await moveMeeting(moveDialog.meetingId, moveDialog.targetPath);
      toast.success('Meeting moved');
      setMoveDialog({ open: false, meetingId: null, title: '', targetPath: null });
    } catch (error) {
      toast.error('Failed to move meeting', {
        description: error instanceof Error ? error.message : String(error),
      });
    }
  };

  // Flatten the folder tree into selectable targets for the "Move to folder" dialog.
  const moveTargets = useMemo(() => {
    const targets: { path: string | null; label: string; depth: number }[] = [
      { path: null, label: 'Unfiled (root)', depth: 0 },
    ];
    const walk = (nodes: FolderNode[], depth: number) => {
      for (const node of nodes) {
        targets.push({ path: node.path, label: node.name, depth });
        if (node.folders.length) walk(node.folders, depth + 1);
      }
    };
    if (folderTree) walk(folderTree.folders, 0);
    return targets;
  }, [folderTree]);

  // Expose setShowModelSettings to window for Rust tray to call
  useEffect(() => {
    (window as any).openSettings = () => {
      setShowModelSettings(true);
    };

    // Cleanup on unmount
    return () => {
      delete (window as any).openSettings;
    };
  }, []);

  const renderCollapsedIcons = () => {
    if (!isCollapsed) return null;

    const isHomePage = pathname === '/';
    const isMeetingPage = pathname?.includes('/meeting-details');
    const isSettingsPage = pathname === '/settings';

    return (
      <TooltipProvider>
        <div className="flex flex-col items-center space-y-4 mt-4">
          <Logo isCollapsed={isCollapsed} />

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/')}
                className={`p-2 rounded-lg transition-colors duration-150 ${isHomePage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <Home className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Home</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={handleRecordingToggle}
                disabled={isRecording}
                className={`p-2 ${isRecording ? 'bg-red-500 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600'} rounded-full transition-colors duration-150 shadow-sm`}
              >
                {isRecording ? (
                  <Square className="w-5 h-5 text-white" />
                ) : (
                  <Mic className="w-5 h-5 text-white" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>{isRecording ? "Recording in progress..." : "Start Recording"}</p>
            </TooltipContent>
          </Tooltip>

          {betaFeatures.importAndRetranscribe && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  onClick={() => openImportDialog()}
                  className="p-2 rounded-lg transition-colors duration-150 hover:bg-blue-100 bg-blue-50"
                >
                  <Upload className="w-5 h-5 text-blue-600" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="right">
                <p>Import Audio</p>
              </TooltipContent>
            </Tooltip>
          )}

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => {
                  if (isCollapsed) toggleCollapse();
                  toggleFolder('meetings');
                }}
                className={`p-2 rounded-lg transition-colors duration-150 ${isMeetingPage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <NotebookPen className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Meeting Notes</p>
            </TooltipContent>
          </Tooltip>

          <Tooltip>
            <TooltipTrigger asChild>
              <button
                onClick={() => router.push('/settings')}
                className={`p-2 rounded-lg transition-colors duration-150 ${isSettingsPage ? 'bg-gray-100' : 'hover:bg-gray-100'
                  }`}
              >
                <Settings className="w-5 h-5 text-gray-600" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="right">
              <p>Settings</p>
            </TooltipContent>
          </Tooltip>

          <Info isCollapsed={isCollapsed} />
        </div>
      </TooltipProvider>
    );
  };

  // Find matching transcript snippet for a meeting item
  const findMatchingSnippet = (itemId: string) => {
    if (!searchQuery.trim() || !searchResults.length) return null;
    return searchResults.find(result => result.id === itemId);
  };

  const renderItem = (item: SidebarItem, depth = 0) => {
    const isExpanded = expandedFolders.has(item.id);
    const paddingLeft = `${depth * 12 + 12}px`;
    const isActive = item.type === 'file' && currentMeeting?.id === item.id;
    const isMeetingItem = item.type === 'file' && !item.id.startsWith('intro-call');
    const isMissing = item.type === 'file' && item.missing === true;

    // Check if this item has a matching transcript snippet
    const matchingResult = isMeetingItem ? findMatchingSnippet(item.id) : null;
    const hasTranscriptMatch = !!matchingResult;

    if (isCollapsed) return null;

    // ---- Organization / Unfiled folder node ----
    if (item.type === 'folder') {
      const isUnfiled = item.folderKind === 'unfiled';
      return (
        <div key={item.id}>
          <div
            className="flex items-center px-3 py-2 my-0.5 rounded-md text-sm hover:bg-gray-50 cursor-pointer group"
            style={{ paddingLeft }}
            onClick={() => toggleFolder(item.id)}
            onContextMenu={(e) => openContextMenu(e, item)}
          >
            {isExpanded ? (
              <ChevronDown className="w-4 h-4 text-gray-400 mr-1 flex-shrink-0" />
            ) : (
              <ChevronRight className="w-4 h-4 text-gray-400 mr-1 flex-shrink-0" />
            )}
            {isUnfiled ? (
              <Inbox className="w-4 h-4 mr-2 text-gray-500 flex-shrink-0" />
            ) : isExpanded ? (
              <FolderOpen className="w-4 h-4 mr-2 text-blue-500 flex-shrink-0" />
            ) : (
              <Folder className="w-4 h-4 mr-2 text-blue-500 flex-shrink-0" />
            )}
            <span className="flex-1 break-words font-medium text-gray-700">{item.title}</span>
            {!isUnfiled && (
              <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                <button
                  onClick={(e) => { e.stopPropagation(); openNewFolderDialog(item.path ?? null); }}
                  className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                  aria-label="New subfolder"
                  title="New subfolder"
                >
                  <FolderPlus className="w-4 h-4" />
                </button>
                <button
                  onClick={(e) => { e.stopPropagation(); openRenameFolderDialog(item); }}
                  className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                  aria-label="Rename folder"
                  title="Rename folder"
                >
                  <Pencil className="w-4 h-4" />
                </button>
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    if (item.isEmpty && item.path) setDeleteFolderState({ open: true, path: item.path, title: item.title });
                  }}
                  disabled={!item.isEmpty}
                  className={`p-1 rounded-md flex-shrink-0 ${item.isEmpty ? 'hover:text-red-600 hover:bg-red-50' : 'text-gray-300 cursor-not-allowed'}`}
                  aria-label="Delete folder"
                  title={item.isEmpty ? 'Delete empty folder' : 'Folder must be empty to delete'}
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              </div>
            )}
          </div>
          {isExpanded && item.children && (
            <div>
              {item.children.map(child => renderItem(child, depth + 1))}
            </div>
          )}
        </div>
      );
    }

    // ---- Meeting leaf ----
    return (
      <div key={item.id}>
        <div
          className={`flex items-center transition-all duration-150 group px-3 py-2 my-0.5 rounded-md text-sm ${
            isMissing ? 'opacity-60' :
            isActive ? 'bg-blue-100 text-blue-700 font-medium' :
            hasTranscriptMatch ? 'bg-yellow-50' : 'hover:bg-gray-50'
          } ${isMissing ? 'cursor-not-allowed' : 'cursor-pointer'}`}
          style={{ paddingLeft }}
          onContextMenu={(e) => !isMissing && openContextMenu(e, item)}
          onClick={() => {
            if (isMissing) return;
            setCurrentMeeting({ id: item.id, title: item.title });
            const basePath = item.id.startsWith('intro-call') ? '/' :
              item.id.includes('-') ? `/meeting-details?id=${item.id}` : `/notes/${item.id}`;
            router.push(basePath);
          }}
        >
          <div className="flex flex-col w-full">
            <div className="flex items-center w-full">
              {isMissing ? (
                <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-amber-100">
                  <AlertTriangle className="w-3.5 h-3.5 text-amber-600" />
                </div>
              ) : isMeetingItem ? (
                <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-gray-100">
                  <File className="w-3.5 h-3.5 text-gray-600" />
                </div>
              ) : (
                <div className="flex-shrink-0 flex items-center justify-center w-6 h-6 rounded-full mr-2 bg-blue-100">
                  <Plus className="w-3.5 h-3.5 text-blue-600" />
                </div>
              )}
              <span className="flex-1 break-words">{item.title}</span>
              {isMissing && (
                <span className="text-xs text-amber-600 mr-1 flex-shrink-0" title="Recording folder not found on disk">missing</span>
              )}
              {isMeetingItem && !isMissing && (
                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setMoveDialog({ open: true, meetingId: item.id, title: item.title, targetPath: item.parentFolderPath ?? null });
                    }}
                    className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                    aria-label="Move meeting to folder"
                    title="Move to folder..."
                  >
                    <FolderInput className="w-4 h-4" />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleEditStart(item.id, item.title);
                    }}
                    className="hover:text-blue-600 p-1 rounded-md hover:bg-blue-50 flex-shrink-0"
                    aria-label="Edit meeting title"
                  >
                    <Pencil className="w-4 h-4" />
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteModalState({ isOpen: true, itemId: item.id });
                    }}
                    className="hover:text-red-600 p-1 rounded-md hover:bg-red-50 flex-shrink-0"
                    aria-label="Delete meeting"
                  >
                    <Trash2 className="w-4 h-4" />
                  </button>
                </div>
              )}
            </div>

            {/* Show transcript match snippet if available */}
            {hasTranscriptMatch && (
              <div className="mt-1 ml-8 text-xs text-gray-500 bg-yellow-50 p-1.5 rounded border border-yellow-100 line-clamp-2">
                <span className="font-medium text-yellow-600">Match:</span> {matchingResult.matchContext}
              </div>
            )}
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="fixed top-0 left-0 h-screen z-40">
      {/* Floating collapse button */}
      <button
        onClick={toggleCollapse}
        className="absolute -right-6 top-20 z-50 p-1 bg-white hover:bg-gray-100 rounded-full shadow-lg border"
        style={{ transform: 'translateX(50%)' }}
      >
        {isCollapsed ? (
          <ChevronRightCircle className="w-6 h-6" />
        ) : (
          <ChevronLeftCircle className="w-6 h-6" />
        )}
      </button>

      <div
        className={`h-screen bg-white border-r shadow-sm flex flex-col transition-all duration-300 ${isCollapsed ? 'w-16' : 'w-64'
          }`}
      >
        {/*  Header with traffic light spacing */}
        <div className="flex-shrink-0 h-22 flex items-center">

          {/* Title container */}



          <div className="flex-1">
            {!isCollapsed && (
              <div className="p-3">
                {/* <span className="text-lg text-center border rounded-full bg-blue-50 border-white font-semibold text-gray-700 mb-2 block items-center">
                  <span>Meetily</span>
                </span> */}
                <Logo isCollapsed={isCollapsed} />

                <div className="relative mb-1">
                  <InputGroup >
                    <InputGroupInput placeholder='Search meeting content...' value={searchQuery}
                      onChange={(e) => handleSearchChange(e.target.value)}
                    />
                    <InputGroupAddon>
                      <SearchIcon />
                    </InputGroupAddon>
                    {searchQuery &&
                      <InputGroupAddon align={'inline-end'}>
                        <InputGroupButton
                          onClick={() => handleSearchChange('')}
                        >
                          <X />
                        </InputGroupButton>
                      </InputGroupAddon>
                    }
                  </InputGroup>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* Main content - scrollable area */}
        <div className="flex-1 flex flex-col min-h-0">
          {/* Fixed navigation items */}
          <div className="flex-shrink-0">
            {!isCollapsed && (
              <div
                onClick={() => router.push('/')}
                className="p-3  text-lg font-semibold items-center hover:bg-gray-100 h-10   flex mx-3 mt-3 rounded-lg cursor-pointer"
              >
                <Home className="w-4 h-4 mr-2" />
                <span>Home</span>
              </div>
            )}
          </div>

          {/* Content area */}
          <div className="flex-1 flex flex-col min-h-0">
            {renderCollapsedIcons()}
            {/* Meeting Notes folder header - fixed */}
            {!isCollapsed && (
              <div className="flex-shrink-0">
                {filteredSidebarItems.filter(item => item.type === 'folder').map(item => (
                  <div key={item.id}>
                    <div
                      className="flex items-center transition-all duration-150 p-3 text-lg font-semibold h-10 mx-3 mt-3 rounded-lg group"
                    >
                      <NotebookPen className="w-4 h-4 mr-2 text-gray-600" />
                      <span className="text-gray-700">{item.title}</span>
                      {searchQuery && item.id === 'meetings' && isSearching && (
                        <span className="ml-2 text-xs text-blue-500 animate-pulse">Searching...</span>
                      )}
                      {item.id === 'meetings' && (
                        <div className="ml-auto flex items-center gap-1">
                          <button
                            onClick={() => openNewFolderDialog(null)}
                            className="p-1 rounded-md text-gray-500 hover:text-blue-600 hover:bg-blue-50"
                            aria-label="New folder"
                            title="New folder"
                          >
                            <FolderPlus className="w-4 h-4" />
                          </button>
                          <button
                            onClick={() => { refreshFolderTree(); }}
                            className="p-1 rounded-md text-gray-500 hover:text-blue-600 hover:bg-blue-50"
                            aria-label="Refresh folders"
                            title="Refresh (pick up Finder changes)"
                          >
                            <RefreshCw className="w-4 h-4" />
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}

            {/* Scrollable meeting items */}
            {!isCollapsed && (
              <div className="flex-1 overflow-y-auto custom-scrollbar min-h-0">
                {filteredSidebarItems
                  .filter(item => item.type === 'folder' && expandedFolders.has(item.id) && item.children)
                  .map(item => (
                    <div key={`${item.id}-children`} className="mx-3">
                      {item.children!.map(child => renderItem(child, 1))}
                    </div>
                  ))}
              </div>
            )}
          </div>
        </div>

        {/* Footer */}
        {!isCollapsed && (

          <div className="flex-shrink-0 p-2 border-t border-gray-100">
            <button
              onClick={handleRecordingToggle}
              disabled={isRecording}
              className={`w-full flex items-center justify-center px-3 py-2 text-sm font-medium text-white ${isRecording ? 'bg-red-300 cursor-not-allowed' : 'bg-red-500 hover:bg-red-600'} rounded-lg transition-colors shadow-sm`}
            >
              {isRecording ? (
                <>
                  <Square className="w-4 h-4 mr-2" />
                  <span>Recording in progress...</span>
                </>
              ) : (
                <>
                  <Mic className="w-4 h-4 mr-2" />
                  <span>Start Recording</span>
                </>
              )}
            </button>

            {betaFeatures.importAndRetranscribe && (
              <button
                onClick={() => openImportDialog()}
                className="w-full flex items-center justify-center px-3 py-2 mt-1 text-sm font-medium text-gray-700 bg-blue-100 hover:bg-blue-200 rounded-lg transition-colors shadow-sm"
              >
                <Upload className="w-4 h-4 mr-2" />
                <span>Import Audio</span>
              </button>
            )}

            <button
              onClick={() => router.push('/settings')}
              className="w-full flex items-center justify-center px-3 py-1.5 mt-1 mb-1 text-sm font-medium text-gray-700 bg-gray-200 hover:bg-gray-300 rounded-lg transition-colors shadow-sm"
            >
              <Settings className="w-4 h-4 mr-2" />
              <span>Settings</span>
            </button>
            <Info isCollapsed={isCollapsed} />
            <div className="w-full flex items-center justify-center px-3 py-1 text-xs text-gray-400">
              v0.4.0
            </div>
          </div>
        )}
      </div>

      {/* Confirmation Modal for Delete */}
      <ConfirmationModal
        isOpen={deleteModalState.isOpen}
        text="Are you sure you want to delete this meeting? This action cannot be undone."
        onConfirm={handleDeleteConfirm}
        onCancel={() => setDeleteModalState({ isOpen: false, itemId: null })}
      />

      {/* Confirmation Modal for empty-folder Delete (O1) */}
      <ConfirmationModal
        isOpen={deleteFolderState.open}
        text={`Delete the empty folder "${deleteFolderState.title}"? This removes the directory from disk.`}
        onConfirm={handleDeleteFolderConfirm}
        onCancel={() => setDeleteFolderState({ open: false, path: null, title: '' })}
      />

      {/* O1: right-click context menu for folders and meetings */}
      {contextMenu && (
        <div
          className="fixed z-[60] min-w-[180px] bg-white border border-gray-200 rounded-md shadow-lg py-1 text-sm"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          onClick={(e) => e.stopPropagation()}
        >
          {contextMenu.item.type === 'folder' ? (
            <>
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-gray-100 flex items-center gap-2"
                onClick={() => { openNewFolderDialog(contextMenu.item.path ?? null); setContextMenu(null); }}
              >
                <FolderPlus className="w-4 h-4" /> New subfolder
              </button>
              {contextMenu.item.folderKind !== 'unfiled' && (
                <>
                  <button
                    className="w-full text-left px-3 py-1.5 hover:bg-gray-100 flex items-center gap-2"
                    onClick={() => { openRenameFolderDialog(contextMenu.item); setContextMenu(null); }}
                  >
                    <Pencil className="w-4 h-4" /> Rename
                  </button>
                  <button
                    className={`w-full text-left px-3 py-1.5 flex items-center gap-2 ${contextMenu.item.isEmpty ? 'hover:bg-gray-100 text-red-600' : 'text-gray-300 cursor-not-allowed'}`}
                    disabled={!contextMenu.item.isEmpty}
                    title={contextMenu.item.isEmpty ? '' : 'Folder must be empty to delete'}
                    onClick={() => {
                      if (contextMenu.item.isEmpty && contextMenu.item.path) {
                        setDeleteFolderState({ open: true, path: contextMenu.item.path, title: contextMenu.item.title });
                      }
                      setContextMenu(null);
                    }}
                  >
                    <Trash2 className="w-4 h-4" /> Delete{!contextMenu.item.isEmpty ? ' (not empty)' : ''}
                  </button>
                </>
              )}
            </>
          ) : (
            <>
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-gray-100 flex items-center gap-2"
                onClick={() => {
                  const it = contextMenu.item;
                  setMoveDialog({ open: true, meetingId: it.id, title: it.title, targetPath: it.parentFolderPath ?? null });
                  setContextMenu(null);
                }}
              >
                <FolderInput className="w-4 h-4" /> Move to folder...
              </button>
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-gray-100 flex items-center gap-2"
                onClick={() => { handleEditStart(contextMenu.item.id, contextMenu.item.title); setContextMenu(null); }}
              >
                <Pencil className="w-4 h-4" /> Rename
              </button>
              <button
                className="w-full text-left px-3 py-1.5 hover:bg-gray-100 flex items-center gap-2 text-red-600"
                onClick={() => { setDeleteModalState({ isOpen: true, itemId: contextMenu.item.id }); setContextMenu(null); }}
              >
                <Trash2 className="w-4 h-4" /> Delete
              </button>
            </>
          )}
        </div>
      )}

      {/* O1: create / rename folder dialog */}
      <Dialog open={folderDialog.open} onOpenChange={(open) => { if (!open) setFolderDialog({ open: false, mode: 'create', parentPath: null, path: null, name: '' }); }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>{folderDialog.mode === 'create' ? 'New Folder' : 'Rename Folder'}</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">{folderDialog.mode === 'create' ? 'New Folder' : 'Rename Folder'}</h3>
            <input
              type="text"
              value={folderDialog.name}
              onChange={(e) => setFolderDialog(prev => ({ ...prev, name: e.target.value }))}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleFolderDialogConfirm();
                else if (e.key === 'Escape') setFolderDialog({ open: false, mode: 'create', parentPath: null, path: null, name: '' });
              }}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
              placeholder="Folder name"
              autoFocus
            />
          </div>
          <DialogFooter>
            <button
              onClick={() => setFolderDialog({ open: false, mode: 'create', parentPath: null, path: null, name: '' })}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleFolderDialogConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              {folderDialog.mode === 'create' ? 'Create' : 'Save'}
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* O1: move meeting to folder dialog */}
      <Dialog open={moveDialog.open} onOpenChange={(open) => { if (!open) setMoveDialog({ open: false, meetingId: null, title: '', targetPath: null }); }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Move Meeting</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-1">Move meeting</h3>
            <p className="text-sm text-gray-500 mb-4 break-words">{moveDialog.title}</p>
            <div className="max-h-64 overflow-y-auto border border-gray-200 rounded-md">
              {moveTargets.map((target) => {
                const selected = moveDialog.targetPath === target.path;
                return (
                  <button
                    key={target.path ?? '__root__'}
                    onClick={() => setMoveDialog(prev => ({ ...prev, targetPath: target.path }))}
                    className={`w-full text-left px-3 py-2 text-sm flex items-center gap-2 ${selected ? 'bg-blue-100 text-blue-700' : 'hover:bg-gray-50'}`}
                    style={{ paddingLeft: `${target.depth * 14 + 12}px` }}
                  >
                    {target.path === null ? <Inbox className="w-4 h-4 flex-shrink-0" /> : <Folder className="w-4 h-4 text-blue-500 flex-shrink-0" />}
                    <span className="break-words">{target.label}</span>
                  </button>
                );
              })}
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={() => setMoveDialog({ open: false, meetingId: null, title: '', targetPath: null })}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleMoveConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Move here
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Edit Meeting Title Modal */}
      <Dialog open={editModalState.isOpen} onOpenChange={(open) => {
        if (!open) handleEditCancel();
      }}>
        <DialogContent className="sm:max-w-[425px]">
          <VisuallyHidden>
            <DialogTitle>Edit Meeting Title</DialogTitle>
          </VisuallyHidden>
          <div className="py-4">
            <h3 className="text-lg font-semibold mb-4">Edit Meeting Title</h3>
            <div className="space-y-4">
              <div>
                <label htmlFor="meeting-title" className="block text-sm font-medium text-gray-700 mb-2">
                  Meeting Title
                </label>
                <input
                  id="meeting-title"
                  type="text"
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      handleEditConfirm();
                    } else if (e.key === 'Escape') {
                      handleEditCancel();
                    }
                  }}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                  placeholder="Enter meeting title"
                  autoFocus
                />
              </div>
            </div>
          </div>
          <DialogFooter>
            <button
              onClick={handleEditCancel}
              className="px-4 py-2 text-sm font-medium text-gray-700 bg-gray-100 hover:bg-gray-200 rounded-md transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={handleEditConfirm}
              className="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-md transition-colors"
            >
              Save
            </button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
};

export default Sidebar;
