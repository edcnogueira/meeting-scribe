'use client';

import React, { createContext, useContext, useState, useEffect } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import Analytics from '@/lib/analytics';
import { invoke } from '@tauri-apps/api/core';
import { useRecordingState } from '@/contexts/RecordingStateContext';


export interface SidebarItem {
  id: string;
  title: string;
  type: 'folder' | 'file';
  children?: SidebarItem[];
  // O1 folder-organization metadata (all optional so legacy usage keeps working):
  path?: string;                       // folder path (folders) or meeting dir path (meetings)
  missing?: boolean;                   // meeting whose directory is gone from disk
  folderKind?: 'root' | 'org' | 'unfiled'; // discriminator for folder nodes
  isEmpty?: boolean;                   // folder has no children (drives delete-enable)
  parentFolderPath?: string | null;    // meeting's containing folder (null = root/unfiled)
}

export interface CurrentMeeting {
  id: string;
  title: string;
}

// ---- O1: folder tree mirrored from disk (shape returned by the Rust command) ----
export interface MeetingNode {
  id: string | null;
  title: string;
  path: string | null;
  missing: boolean;
}

export interface FolderNode {
  name: string;
  path: string;
  folders: FolderNode[];
  meetings: MeetingNode[];
}

export interface MeetingFolderTree {
  base_path: string;
  folders: FolderNode[];
  unfiled: MeetingNode[];
}

// Search result type for transcript search
interface TranscriptSearchResult {
  id: string;
  title: string;
  matchContext: string;
  timestamp: string;
};

interface SidebarContextType {
  currentMeeting: CurrentMeeting | null;
  setCurrentMeeting: (meeting: CurrentMeeting | null) => void;
  sidebarItems: SidebarItem[];
  isCollapsed: boolean;
  toggleCollapse: () => void;
  meetings: CurrentMeeting[];
  setMeetings: (meetings: CurrentMeeting[]) => void;
  isMeetingActive: boolean;
  setIsMeetingActive: (active: boolean) => void;
  handleRecordingToggle: () => void;
  searchTranscripts: (query: string) => Promise<void>;
  searchResults: TranscriptSearchResult[];
  isSearching: boolean;
  setServerAddress: (address: string) => void;
  serverAddress: string;
  transcriptServerAddress: string;
  setTranscriptServerAddress: (address: string) => void;
  // Summary polling management
  activeSummaryPolls: Map<string, NodeJS.Timeout>;
  startSummaryPolling: (meetingId: string, processId: string, onUpdate: (result: any) => void) => void;
  stopSummaryPolling: (meetingId: string) => void;
  // Refetch meetings from backend
  refetchMeetings: () => Promise<void>;

  // O1: folder organization mirrored on disk
  folderTree: MeetingFolderTree | null;
  refreshFolderTree: () => Promise<void>;
  createFolder: (parentPath: string | null, name: string) => Promise<void>;
  renameFolder: (path: string, newName: string) => Promise<void>;
  deleteFolder: (path: string) => Promise<void>;
  moveMeeting: (meetingId: string, targetFolderPath: string | null) => Promise<void>;
}

const SidebarContext = createContext<SidebarContextType | null>(null);

export const useSidebar = () => {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider');
  }
  return context;
};

export function SidebarProvider({ children }: { children: React.ReactNode }) {
  const [currentMeeting, setCurrentMeeting] = useState<CurrentMeeting | null>({ id: 'intro-call', title: '+ New Call' });
  const [isCollapsed, setIsCollapsed] = useState(true);
  const [meetings, setMeetings] = useState<CurrentMeeting[]>([]);
  const [sidebarItems, setSidebarItems] = useState<SidebarItem[]>([]);
  const [isMeetingActive, setIsMeetingActive] = useState(false);
  const [searchResults, setSearchResults] = useState<any[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [serverAddress, setServerAddress] = useState('');
  const [transcriptServerAddress, setTranscriptServerAddress] = useState('');
  const [activeSummaryPolls, setActiveSummaryPolls] = useState<Map<string, NodeJS.Timeout>>(new Map());
  const [folderTree, setFolderTree] = useState<MeetingFolderTree | null>(null);

  // Use recording state from RecordingStateContext (single source of truth)
  const { isRecording } = useRecordingState();

  const pathname = usePathname();
  const router = useRouter();

  // Extract fetchMeetings as a reusable function
  const fetchMeetings = React.useCallback(async () => {
    if (serverAddress) {
      try {
        const meetings = await invoke('api_get_meetings') as Array<{ id: string, title: string }>;
        const transformedMeetings = meetings.map((meeting: any) => ({
          id: meeting.id,
          title: meeting.title
        }));
        setMeetings(transformedMeetings);
        Analytics.trackBackendConnection(true);
      } catch (error) {
        console.error('Error fetching meetings:', error);
        setMeetings([]);
        Analytics.trackBackendConnection(false, error instanceof Error ? error.message : 'Unknown error');
      }
    }
  }, [serverAddress]);

  useEffect(() => {
    fetchMeetings();
  }, [serverAddress, fetchMeetings]);

  // ---- O1: folder tree management ----
  const refreshFolderTree = React.useCallback(async () => {
    try {
      const tree = await invoke('api_list_meeting_folder_tree') as MeetingFolderTree;
      setFolderTree(tree);
    } catch (error) {
      console.error('Error loading folder tree:', error);
    }
  }, []);

  useEffect(() => {
    refreshFolderTree();
  }, [refreshFolderTree]);

  const createFolder = React.useCallback(async (parentPath: string | null, name: string) => {
    await invoke('api_create_meeting_folder', { parentPath, name });
    await refreshFolderTree();
  }, [refreshFolderTree]);

  const renameFolder = React.useCallback(async (path: string, newName: string) => {
    await invoke('api_rename_meeting_folder', { path, newName });
    await refreshFolderTree();
  }, [refreshFolderTree]);

  const deleteFolder = React.useCallback(async (path: string) => {
    await invoke('api_delete_meeting_folder', { path });
    await refreshFolderTree();
  }, [refreshFolderTree]);

  const moveMeeting = React.useCallback(async (meetingId: string, targetFolderPath: string | null) => {
    await invoke('api_move_meeting_to_folder', { meetingId, targetFolderPath });
    await refreshFolderTree();
  }, [refreshFolderTree]);

  useEffect(() => {
    const fetchSettings = async () => {
      setServerAddress('http://localhost:5167');
      setTranscriptServerAddress('http://127.0.0.1:8178/stream');
    };
    fetchSettings();
  }, []);

  // ---- O1: build the sidebar tree from the on-disk folder tree ----
  const meetingNodeToItem = React.useCallback(
    (m: MeetingNode, parentFolderPath: string | null): SidebarItem => ({
      id: m.id ?? `orphan:${m.path ?? m.title}`,
      title: m.title,
      type: 'file',
      path: m.path ?? undefined,
      missing: m.missing,
      parentFolderPath,
    }),
    []
  );

  const folderNodeToItem = React.useCallback(
    (folder: FolderNode): SidebarItem => {
      const childFolders = folder.folders.map(folderNodeToItem);
      const childMeetings = folder.meetings.map(m => meetingNodeToItem(m, folder.path));
      const children = [...childFolders, ...childMeetings];
      return {
        id: `folder:${folder.path}`,
        title: folder.name,
        type: 'folder',
        children,
        path: folder.path,
        folderKind: 'org',
        isEmpty: children.length === 0,
        parentFolderPath: null,
      };
    },
    [meetingNodeToItem]
  );

  const buildSidebarItems = React.useCallback((): SidebarItem[] => {
    if (folderTree) {
      const orgItems = folderTree.folders.map(folderNodeToItem);
      const children: SidebarItem[] = [...orgItems];
      if (folderTree.unfiled.length > 0) {
        children.push({
          id: '__unfiled__',
          title: 'Unfiled',
          type: 'folder',
          folderKind: 'unfiled',
          isEmpty: folderTree.unfiled.length === 0,
          parentFolderPath: null,
          children: folderTree.unfiled.map(m => meetingNodeToItem(m, null)),
        });
      }
      return [
        {
          id: 'meetings',
          title: 'Meeting Notes',
          type: 'folder',
          folderKind: 'root',
          path: folderTree.base_path,
          parentFolderPath: null,
          children,
        },
      ];
    }

    // Fallback (tree not loaded yet): flat list from the meetings we already have.
    return [
      {
        id: 'meetings',
        title: 'Meeting Notes',
        type: 'folder',
        folderKind: 'root',
        children: meetings.map(meeting => ({ id: meeting.id, title: meeting.title, type: 'file' as const })),
      },
    ];
  }, [folderTree, meetings, folderNodeToItem, meetingNodeToItem]);

  const toggleCollapse = () => {
    setIsCollapsed(!isCollapsed);
  };

  // Update current meeting when on home page
  useEffect(() => {
    if (pathname === '/') {
      setCurrentMeeting({ id: 'intro-call', title: '+ New Call' });
    }
    setSidebarItems(buildSidebarItems());
  }, [pathname, buildSidebarItems]);

  // Rebuild sidebar items when the folder tree (or meetings fallback) changes
  useEffect(() => {
    setSidebarItems(buildSidebarItems());
  }, [buildSidebarItems]);

  // Function to handle recording toggle from sidebar
  const handleRecordingToggle = () => {
    if (!isRecording) {
      // Check if already on home page
      if (pathname === '/') {
        // Already on home - trigger recording directly via custom event
        console.log('Triggering recording from sidebar (already on home page)');
        window.dispatchEvent(new CustomEvent('start-recording-from-sidebar'));
      } else {
        // Not on home - navigate and use auto-start mechanism
        console.log('Navigating to home page with auto-start flag');
        sessionStorage.setItem('autoStartRecording', 'true');
        router.push('/');
      }

      // Track recording initiation from sidebar
      Analytics.trackButtonClick('start_recording', 'sidebar');
    }
    // The actual recording start/stop is handled in the Home component
  };

  // Function to search through meeting transcripts
  const searchTranscripts = async (query: string) => {
    if (!query.trim()) {
      setSearchResults([]);
      return;
    }

    try {
      setIsSearching(true);


      const results = await invoke('api_search_transcripts', { query }) as TranscriptSearchResult[];
      setSearchResults(results);
    } catch (error) {
      console.error('Error searching transcripts:', error);
      setSearchResults([]);
    } finally {
      setIsSearching(false);
    }
  };

  // Summary polling management
  const startSummaryPolling = React.useCallback((
    meetingId: string,
    processId: string,
    onUpdate: (result: any) => void
  ) => {
    // Stop existing poll for this meeting if any
    if (activeSummaryPolls.has(meetingId)) {
      clearInterval(activeSummaryPolls.get(meetingId)!);
    }

    console.log(`📊 Starting polling for meeting ${meetingId}, process ${processId}`);

    let pollCount = 0;
    const MAX_POLLS = 200; // ~16.5 minutes at 5-second intervals (slightly longer than backend's 15-min timeout to avoid race conditions)

    const pollInterval = setInterval(async () => {
      pollCount++;

      // Timeout safety: Stop after 10 minutes
      if (pollCount >= MAX_POLLS) {
        console.warn(`⏱️ Polling timeout for ${meetingId} after ${MAX_POLLS} iterations`);
        clearInterval(pollInterval);
        setActiveSummaryPolls(prev => {
          const next = new Map(prev);
          next.delete(meetingId);
          return next;
        });
        onUpdate({
          status: 'error',
          error: 'Summary generation timed out after 15 minutes. Please try again or check your model configuration.'
        });
        return;
      }
      try {
        const result = await invoke('api_get_summary', {
          meetingId: meetingId,
        }) as any;

        console.log(`📊 Polling update for ${meetingId}:`, result.status);

        // Call the update callback with result
        onUpdate(result);

        // Stop polling if completed, error, failed, cancelled, or idle (after initial processing)
        if (result.status === 'completed' || result.status === 'error' || result.status === 'failed' || result.status === 'cancelled') {
          console.log(`Polling completed for ${meetingId}, status: ${result.status}`);
          clearInterval(pollInterval);
          setActiveSummaryPolls(prev => {
            const next = new Map(prev);
            next.delete(meetingId);
            return next;
          });
        } else if (result.status === 'idle' && pollCount > 1) {
          // If we get 'idle' after polling started, process completed/disappeared
          console.log(`Process completed or not found for ${meetingId}, stopping poll`);
          clearInterval(pollInterval);
          setActiveSummaryPolls(prev => {
            const next = new Map(prev);
            next.delete(meetingId);
            return next;
          });
        }
      } catch (error) {
        console.error(`Polling error for ${meetingId}:`, error);
        // Report error to callback
        onUpdate({
          status: 'error',
          error: error instanceof Error ? error.message : 'Unknown error'
        });
        clearInterval(pollInterval);
        setActiveSummaryPolls(prev => {
          const next = new Map(prev);
          next.delete(meetingId);
          return next;
        });
      }
    }, 5000); // Poll every 5 seconds

    setActiveSummaryPolls(prev => new Map(prev).set(meetingId, pollInterval));
  }, [activeSummaryPolls]);

  const stopSummaryPolling = React.useCallback((meetingId: string) => {
    const pollInterval = activeSummaryPolls.get(meetingId);
    if (pollInterval) {
      console.log(`⏹️ Stopping polling for meeting ${meetingId}`);
      clearInterval(pollInterval);
      setActiveSummaryPolls(prev => {
        const next = new Map(prev);
        next.delete(meetingId);
        return next;
      });
    }
  }, [activeSummaryPolls]);

  // Cleanup all polling intervals on unmount
  useEffect(() => {
    return () => {
      console.log('🧹 Cleaning up all summary polling intervals');
      activeSummaryPolls.forEach(interval => clearInterval(interval));
    };
  }, [activeSummaryPolls]);



  return (
    <SidebarContext.Provider value={{
      currentMeeting,
      setCurrentMeeting,
      sidebarItems,
      isCollapsed,
      toggleCollapse,
      meetings,
      setMeetings,
      isMeetingActive,
      setIsMeetingActive,
      handleRecordingToggle,
      searchTranscripts,
      searchResults,
      isSearching,
      setServerAddress,
      serverAddress,
      transcriptServerAddress,
      setTranscriptServerAddress,
      activeSummaryPolls,
      startSummaryPolling,
      stopSummaryPolling,
      refetchMeetings: fetchMeetings,
      folderTree,
      refreshFolderTree,
      createFolder,
      renameFolder,
      deleteFolder,
      moveMeeting,
    }}>
      {children}
    </SidebarContext.Provider>
  );
}
