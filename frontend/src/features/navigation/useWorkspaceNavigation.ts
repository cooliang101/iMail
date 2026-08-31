import { useCallback, useState } from 'preact/compat';
import type { AppView, ParticipantFilters, SearchFilters, WorkspaceFolder } from '../../app-model';

export function useWorkspaceNavigation(initialView: AppView) {
  const [view, setView] = useState<AppView>(initialView);
  const [accountFilter, setAccountFilter] = useState('all');
  const [groupFilter, setGroupFilter] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [searchFilters, setSearchFilters] = useState<SearchFilters | null>(null);
  const [smartFolderId, setSmartFolderId] = useState<string | null>(null);
  const [participantFilters, setParticipantFilters] = useState<ParticipantFilters>({ sender: null, recipient: null });
  const [activeLabel, setActiveLabel] = useState<string | null>(null);
  const [activeMailbox, setActiveMailbox] = useState<WorkspaceFolder | null>(null);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  const selectScope = useCallback((nextView: AppView, nextAccount = 'all', nextGroup: string | null = null) => {
    setSearchFilters(null); setSmartFolderId(null);
    setSearch((current) => (view === 'contacts') !== (nextView === 'contacts') ? '' : current);
    setView(nextView);
    setAccountFilter(nextAccount);
    setGroupFilter(nextGroup);
    setActiveLabel(null);
    setActiveMailbox(null);
    setSidebarOpen(false);
  }, [view]);

  const selectMailbox = useCallback((folder: WorkspaceFolder) => {
    setSearchFilters(null); setSmartFolderId(null);
    setView('folder');
    setAccountFilter('all');
    setGroupFilter(null);
    setActiveLabel(null);
    setActiveMailbox(folder);
    setSidebarOpen(false);
  }, []);

  const selectLabel = useCallback((label: string) => {
    setSearchFilters(null); setSmartFolderId(null);
    setView('inbox');
    setAccountFilter('all');
    setGroupFilter(null);
    setActiveLabel(label);
    setActiveMailbox(null);
    setSidebarOpen(false);
  }, []);

  return {
    searchFilters, setSearchFilters, smartFolderId, setSmartFolderId,
    view, setView,
    accountFilter, setAccountFilter,
    groupFilter, setGroupFilter,
    search, setSearch,
    participantFilters, setParticipantFilters,
    activeLabel,
    activeMailbox,
    sidebarOpen, setSidebarOpen,
    sidebarCollapsed, setSidebarCollapsed,
    selectScope,
    selectMailbox,
    selectLabel,
  };
}
