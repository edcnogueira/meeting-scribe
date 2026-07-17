import { useState, useEffect, useRef } from 'react';
import { useSidebar } from './Sidebar/SidebarProvider';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '@/components/ui/button';
import { useOllamaDownload } from '@/contexts/OllamaDownloadContext';
import { BuiltInModelManager } from '@/components/BuiltInModelManager';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { useConfig } from '@/contexts/ConfigContext';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Alert, AlertDescription } from '@/components/ui/alert';
import { ScrollArea } from '@/components/ui/scroll-area';
import { Switch } from '@/components/ui/switch';
import { Lock, Unlock, Eye, EyeOff, RefreshCw, CheckCircle2, XCircle, ChevronDown, ChevronUp, Download, ExternalLink, Check, ChevronsUpDown } from 'lucide-react';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command';
import { cn, isOllamaNotInstalledError } from '@/lib/utils';
import { toast } from 'sonner';

export interface ModelConfig {
  provider: 'ollama' | 'groq' | 'claude' | 'openai' | 'openrouter' | 'builtin-ai' | 'custom-openai' | 'cli-agent';
  model: string;
  whisperModel: string;
  apiKey?: string | null;
  ollamaEndpoint?: string | null;
  // Custom OpenAI fields
  customOpenAIEndpoint?: string | null;
  customOpenAIModel?: string | null;
  customOpenAIApiKey?: string | null;
  maxTokens?: number | null;
  temperature?: number | null;
  topP?: number | null;
  // CLI Agent fields (only populated when provider is 'cli-agent')
  cliAgentPreset?: string | null;
  cliAgentCommand?: string | null;
  cliAgentArgs?: string[] | null;
}

// CLI agent presets surfaced in the settings UI. `custom` lets the user point at
// any installed binary that reads a prompt from stdin and writes markdown to stdout.
const CLI_AGENT_PRESETS: { id: string; label: string; hint: string }[] = [
  { id: 'codex', label: 'Codex (OpenAI)', hint: 'codex login' },
  { id: 'claude', label: 'Claude Code (Anthropic)', hint: 'claude — then complete the login flow' },
  { id: 'gemini', label: 'Gemini (Google)', hint: 'gemini — then complete the login flow' },
  { id: 'custom', label: 'Custom command', hint: 'any CLI that reads the prompt from stdin' },
];

interface OllamaModel {
  name: string;
  id: string;
  size: string;
  modified: string;
}

interface OpenRouterModel {
  id: string;
  name: string;
  context_length?: number;
  prompt_price?: string;
  completion_price?: string;
}

interface OpenAIModel {
  id: string;
}

interface AnthropicModel {
  id: string;
  display_name?: string;
}

interface GroqModel {
  id: string;
  owned_by?: string;
}

// Fallback models for when API fetch fails or no API key provided
const OPENAI_FALLBACK_MODELS = [
  'gpt-4o',
  'gpt-4o-mini',
  'gpt-4-turbo',
  'gpt-4',
  'gpt-3.5-turbo',
  'o1',
  'o1-mini',
  'o3',
  'o3-mini',
];

const CLAUDE_FALLBACK_MODELS = [
  'claude-sonnet-4-5-20250929',
  'claude-haiku-4-5-20251001',
  'claude-opus-4-5-20251101',
  'claude-3-5-sonnet-latest',
];

const GROQ_FALLBACK_MODELS = [
  'llama-3.3-70b-versatile',
  'llama-3.1-70b-versatile',
  'mixtral-8x7b-32768',
  'gemma2-9b-it',
];

interface ModelSettingsModalProps {
  modelConfig: ModelConfig;
  setModelConfig: (config: ModelConfig | ((prev: ModelConfig) => ModelConfig)) => void;
  onSave: (config: ModelConfig) => void;
  skipInitialFetch?: boolean; // Optional: skip fetching config from backend if parent manages it
  layout?: 'inline' | 'dialog';
}

export function ModelSettingsModal({
  modelConfig: propsModelConfig,
  setModelConfig: propsSetModelConfig,
  onSave,
  skipInitialFetch = false,
  layout = 'inline',
}: ModelSettingsModalProps) {
  // Use ConfigContext if available, fallback to props for backward compatibility
  const configContext = useConfig();
  const modelConfig = configContext?.modelConfig || propsModelConfig;
  const setModelConfig = configContext?.setModelConfig || propsSetModelConfig;
  const providerApiKeys = configContext?.providerApiKeys;
  const updateProviderApiKey = configContext?.updateProviderApiKey;

  const [models, setModels] = useState<OllamaModel[]>([]);
  const [error, setError] = useState<string>('');
  const [apiKey, setApiKey] = useState<string | null>(modelConfig.apiKey || null);
  const [showApiKey, setShowApiKey] = useState<boolean>(false);
  const [isApiKeyLocked, setIsApiKeyLocked] = useState<boolean>(!!modelConfig.apiKey?.trim());
  const [isLockButtonVibrating, setIsLockButtonVibrating] = useState<boolean>(false);
  const { serverAddress } = useSidebar();
  const [openRouterModels, setOpenRouterModels] = useState<OpenRouterModel[]>([]);
  const [openRouterError, setOpenRouterError] = useState<string>('');
  const [isLoadingOpenRouter, setIsLoadingOpenRouter] = useState<boolean>(false);
  const [ollamaEndpoint, setOllamaEndpoint] = useState<string>(modelConfig.ollamaEndpoint || '');
  const [isLoadingOllama, setIsLoadingOllama] = useState<boolean>(false);
  const [lastFetchedEndpoint, setLastFetchedEndpoint] = useState<string>(modelConfig.ollamaEndpoint || '');
  const [endpointValidationState, setEndpointValidationState] = useState<'valid' | 'invalid' | 'none'>('none');
  const [hasAutoFetched, setHasAutoFetched] = useState<boolean>(false);
  const hasSyncedFromParent = useRef<boolean>(false);
  const hasLoadedInitialConfig = useRef<boolean>(false);
  const [autoGenerateEnabled, setAutoGenerateEnabled] = useState<boolean>(true); // Default to true
  const [searchQuery, setSearchQuery] = useState<string>('');
  const [isEndpointSectionCollapsed, setIsEndpointSectionCollapsed] = useState<boolean>(true); // Collapsed by default
  const [ollamaNotInstalled, setOllamaNotInstalled] = useState<boolean>(false); // Track if Ollama is not installed

  // Custom OpenAI state
  const [customOpenAIEndpoint, setCustomOpenAIEndpoint] = useState<string>(modelConfig.customOpenAIEndpoint || '');
  const [customOpenAIModel, setCustomOpenAIModel] = useState<string>(modelConfig.customOpenAIModel || '');
  const [customOpenAIApiKey, setCustomOpenAIApiKey] = useState<string>(modelConfig.customOpenAIApiKey || '');
  const [customMaxTokens, setCustomMaxTokens] = useState<string>(modelConfig.maxTokens?.toString() || '');
  const [customTemperature, setCustomTemperature] = useState<string>(modelConfig.temperature?.toString() || '');
  const [customTopP, setCustomTopP] = useState<string>(modelConfig.topP?.toString() || '');
  const [isCustomOpenAIAdvancedOpen, setIsCustomOpenAIAdvancedOpen] = useState<boolean>(false);
  const [isTestingConnection, setIsTestingConnection] = useState<boolean>(false);

  // CLI Agent state
  const [cliAgentPreset, setCliAgentPreset] = useState<string>(modelConfig.cliAgentPreset || 'codex');
  const [cliAgentCommand, setCliAgentCommand] = useState<string>(modelConfig.cliAgentCommand || '');
  const [cliAgentArgs, setCliAgentArgs] = useState<string>((modelConfig.cliAgentArgs || []).join(' '));
  const [cliAgentInstallStatus, setCliAgentInstallStatus] =
    useState<'unknown' | 'checking' | 'installed' | 'not-found'>('unknown');
  const [cliAgentTestResult, setCliAgentTestResult] =
    useState<{ ok: boolean; message: string } | null>(null);
  const [isTestingCliAgent, setIsTestingCliAgent] = useState<boolean>(false);
  const cliAgentConfigLoaded = useRef<boolean>(false);

  // Split a whitespace-separated args string into a clean array (custom preset only)
  const parseCliAgentArgs = (raw: string): string[] =>
    raw.split(/\s+/).map((a) => a.trim()).filter((a) => a.length > 0);

  // Combobox state
  const [modelComboboxOpen, setModelComboboxOpen] = useState<boolean>(false);

  // Dynamic model fetching state for OpenAI, Claude, and Groq
  const [openaiModels, setOpenaiModels] = useState<string[]>([]);
  const [claudeModels, setClaudeModels] = useState<string[]>([]);
  const [groqModels, setGroqModels] = useState<string[]>([]);
  const [isLoadingOpenAI, setIsLoadingOpenAI] = useState<boolean>(false);
  const [isLoadingClaude, setIsLoadingClaude] = useState<boolean>(false);
  const [isLoadingGroq, setIsLoadingGroq] = useState<boolean>(false);

  // Use global download context instead of local state
  const { isDownloading, getProgress, downloadingModels } = useOllamaDownload();

  // Built-in AI models state
  const [builtinAiModels, setBuiltinAiModels] = useState<any[]>([]);

  // Cache models by endpoint to avoid refetching when reverting endpoint changes
  const modelsCache = useRef<Map<string, OllamaModel[]>>(new Map());

  // URL validation helper
  const validateOllamaEndpoint = (url: string): boolean => {
    if (!url.trim()) return true; // Empty is valid (uses default)
    try {
      const parsed = new URL(url);
      return parsed.protocol === 'http:' || parsed.protocol === 'https:';
    } catch {
      return false;
    }
  };

  // Debounced URL validation with visual feedback
  useEffect(() => {
    const timer = setTimeout(() => {
      const trimmed = ollamaEndpoint.trim();

      if (!trimmed) {
        setEndpointValidationState('none');
      } else if (validateOllamaEndpoint(trimmed)) {
        setEndpointValidationState('valid');
      } else {
        setEndpointValidationState('invalid');
      }
    }, 500); // 500ms debounce

    return () => clearTimeout(timer);
  }, [ollamaEndpoint]);

  const fetchApiKey = async (provider: string) => {
    try {
      const data = (await invoke('api_get_api_key', {
        provider,
      })) as string;
      setApiKey(data || '');
    } catch (err) {
      console.error('Error fetching API key:', err);
      setApiKey(null);
    }
  };

  // Auto-unlock when API key becomes empty, 
  useEffect(() => {
    const hasContent = !!apiKey?.trim();
    if (!hasContent) {
      setIsApiKeyLocked(false);
    }
  }, [apiKey]);

  const modelOptions: Record<string, string[]> = {
    ollama: models.map((model) => model.name),
    claude: claudeModels.length > 0 ? claudeModels : CLAUDE_FALLBACK_MODELS,
    groq: groqModels.length > 0 ? groqModels : GROQ_FALLBACK_MODELS,
    openai: openaiModels.length > 0 ? openaiModels : OPENAI_FALLBACK_MODELS,
    openrouter: openRouterModels.map((m) => m.id),
    'builtin-ai': builtinAiModels.map((m) => m.name),
    'custom-openai': customOpenAIModel ? [customOpenAIModel] : [], // User specifies model manually
    'cli-agent': [], // No model list — the CLI decides its own model
  };

  const requiresApiKey =
    modelConfig.provider === 'claude' ||
    modelConfig.provider === 'groq' ||
    modelConfig.provider === 'openai' ||
    modelConfig.provider === 'openrouter';

  // Check if Ollama endpoint has changed but models haven't been fetched yet
  const ollamaEndpointChanged = modelConfig.provider === 'ollama' &&
    ollamaEndpoint.trim() !== lastFetchedEndpoint.trim();

  // Custom OpenAI validation
  const isCustomOpenAIInvalid = modelConfig.provider === 'custom-openai' && (
    !customOpenAIEndpoint.trim() ||
    !customOpenAIModel.trim()
  );

  // CLI agent validation: the custom preset needs an explicit command
  const isCliAgentInvalid =
    modelConfig.provider === 'cli-agent' && cliAgentPreset === 'custom' && !cliAgentCommand.trim();

  const isDoneDisabled =
    (requiresApiKey && (!apiKey || (typeof apiKey === 'string' && !apiKey.trim()))) ||
    (modelConfig.provider === 'ollama' && ollamaEndpointChanged) ||
    isCustomOpenAIInvalid ||
    isCliAgentInvalid;

  useEffect(() => {
    const fetchModelConfig = async () => {
      // If parent component manages config, skip fetch and just mark as loaded
      if (skipInitialFetch) {
        hasLoadedInitialConfig.current = true;
        return;
      }

      try {
        const data = (await invoke('api_get_model_config')) as any;
        if (data && data.provider !== null) {
          setModelConfig(data);

          // Fetch API key if not included in response and provider requires it
          if (data.provider !== 'ollama' && !data.apiKey) {
            try {
              const apiKeyData = await invoke('api_get_api_key', {
                provider: data.provider
              }) as string;
              data.apiKey = apiKeyData;
              setApiKey(apiKeyData);
            } catch (err) {
              console.error('Failed to fetch API key:', err);
            }
          }

          // Sync ollamaEndpoint state with fetched config
          if (data.ollamaEndpoint) {
            setOllamaEndpoint(data.ollamaEndpoint);
            // Don't set lastFetchedEndpoint here - it will be set after successful model fetch
          }
          hasLoadedInitialConfig.current = true; // Mark that initial config is loaded

          // Fetch Custom OpenAI config if that's the active provider
          if (data.provider === 'custom-openai') {
            try {
              const customConfig = (await invoke('api_get_custom_openai_config')) as any;
              if (customConfig) {
                setCustomOpenAIEndpoint(customConfig.endpoint || '');
                setCustomOpenAIModel(customConfig.model || '');
                setCustomOpenAIApiKey(customConfig.apiKey || '');
                setCustomMaxTokens(customConfig.maxTokens?.toString() || '');
                setCustomTemperature(customConfig.temperature?.toString() || '');
                setCustomTopP(customConfig.topP?.toString() || '');
              }
            } catch (err) {
              console.error('Failed to fetch custom OpenAI config:', err);
            }
          }
        }
      } catch (error) {
        console.error('Failed to fetch model config:', error);
        hasLoadedInitialConfig.current = true; // Mark as loaded even on error
      }
    };

    fetchModelConfig();
  }, [skipInitialFetch]);

  // Fetch auto-generate setting on mount
  useEffect(() => {
    const fetchAutoGenerateSetting = async () => {
      try {
        const enabled = (await invoke('api_get_auto_generate_setting')) as boolean;
        setAutoGenerateEnabled(enabled);
        console.log('Auto-generate setting loaded:', enabled);
      } catch (err) {
        console.error('Failed to fetch auto-generate setting:', err);
        // Keep default value (true) on error
      }
    };

    fetchAutoGenerateSetting();
  }, []);

  // Sync ollamaEndpoint state when modelConfig.ollamaEndpoint changes from parent
  useEffect(() => {
    const endpoint = modelConfig.ollamaEndpoint || '';
    if (endpoint !== ollamaEndpoint) {
      setOllamaEndpoint(endpoint);
      // Don't set lastFetchedEndpoint here - only after successful model fetch
    }
    // Only mark as synced if we have a valid provider (prevents race conditions during init)
    if (modelConfig.provider) {
      hasSyncedFromParent.current = true; // Mark that we've received prop value
    }
  }, [modelConfig.ollamaEndpoint, modelConfig.provider]);

  // Sync custom OpenAI state from modelConfig (context or props)
  useEffect(() => {
    if (modelConfig.provider === 'custom-openai') {
      console.log('Syncing custom OpenAI fields from ConfigContext:', {
        endpoint: modelConfig.customOpenAIEndpoint,
        model: modelConfig.customOpenAIModel,
        hasApiKey: !!modelConfig.customOpenAIApiKey,
      });

      // Always sync from modelConfig (which comes from context if available)
      setCustomOpenAIEndpoint(modelConfig.customOpenAIEndpoint || '');
      setCustomOpenAIModel(modelConfig.customOpenAIModel || '');
      setCustomOpenAIApiKey(modelConfig.customOpenAIApiKey || '');
      setCustomMaxTokens(modelConfig.maxTokens?.toString() || '');
      setCustomTemperature(modelConfig.temperature?.toString() || '');
      setCustomTopP(modelConfig.topP?.toString() || '');
    }
  }, [
    modelConfig.provider,
    modelConfig.customOpenAIEndpoint,
    modelConfig.customOpenAIModel,
    modelConfig.customOpenAIApiKey,
    modelConfig.maxTokens,
    modelConfig.temperature,
    modelConfig.topP
  ]);

  // Load the saved CLI agent config whenever cli-agent becomes the active provider.
  // Owns its own fetch (independent of skipInitialFetch) so it works both when the
  // modal manages config itself and when a parent (SummaryModelSettings) does.
  useEffect(() => {
    if (modelConfig.provider !== 'cli-agent' || cliAgentConfigLoaded.current) return;
    cliAgentConfigLoaded.current = true;

    invoke<any>('api_get_cli_agent_config')
      .then((config) => {
        if (config) {
          setCliAgentPreset(config.preset || 'codex');
          setCliAgentCommand(config.command || '');
          setCliAgentArgs(Array.isArray(config.args) ? config.args.join(' ') : '');
        }
      })
      .catch((err) => {
        console.error('Failed to load CLI agent config:', err);
      });
  }, [modelConfig.provider]);

  // Allow re-loading if the user switches away from cli-agent and back
  useEffect(() => {
    if (modelConfig.provider !== 'cli-agent') {
      cliAgentConfigLoaded.current = false;
    }
  }, [modelConfig.provider]);

  // Reset hasAutoFetched flag and clear models when switching away from Ollama
  useEffect(() => {
    if (modelConfig.provider !== 'ollama') {
      setHasAutoFetched(false); // Reset flag so it can auto-fetch again if user switches back
      setModels([]); // Clear models list
      setError(''); // Clear any error state
      setOllamaNotInstalled(false); // Reset installation status
    }
  }, [modelConfig.provider]);

  // Handle endpoint changes - restore cached models or clear
  useEffect(() => {
    if (modelConfig.provider === 'ollama' &&
      ollamaEndpoint.trim() !== lastFetchedEndpoint.trim()) {

      // Check if we have cached models for this endpoint (including empty endpoint = default)
      const cachedModels = modelsCache.current.get(ollamaEndpoint.trim());

      if (cachedModels && cachedModels.length > 0) {
        // Restore cached models and update tracking
        setModels(cachedModels);
        setLastFetchedEndpoint(ollamaEndpoint.trim());
        setError('');
      } else {
        // No cache - clear models and allow refetch
        setHasAutoFetched(false);
        setModels([]);
        setError('');
      }
    }
  }, [ollamaEndpoint, lastFetchedEndpoint, modelConfig.provider]);

  // Sync local apiKey state when provider changes
  useEffect(() => {
    if (providerApiKeys && requiresApiKey && modelConfig.provider !== 'custom-openai') {
      const correctKey = providerApiKeys[modelConfig.provider as keyof typeof providerApiKeys];
      if (correctKey !== apiKey) {
        setApiKey(correctKey || '');
        setIsApiKeyLocked(!!correctKey?.trim());
      }
    }
  }, [modelConfig.provider, providerApiKeys, requiresApiKey]);

  // Manual fetch function for Ollama models
  const fetchOllamaModels = async (silent = false) => {
    const trimmedEndpoint = ollamaEndpoint.trim();

    // Validate URL if provided
    if (trimmedEndpoint && !validateOllamaEndpoint(trimmedEndpoint)) {
      const errorMsg = 'Invalid Ollama endpoint URL. Must start with http:// or https://';
      setError(errorMsg);
      if (!silent) {
        toast.error(errorMsg);
      }
      return;
    }

    setIsLoadingOllama(true);
    setError(''); // Clear previous errors

    try {
      const endpoint = trimmedEndpoint || null;
      const modelList = (await invoke('get_ollama_models', { endpoint })) as OllamaModel[];
      setModels(modelList);
      setLastFetchedEndpoint(trimmedEndpoint); // Track successful fetch

      // Cache the fetched models for this endpoint
      modelsCache.current.set(trimmedEndpoint, modelList);

      // Successfully fetched models, Ollama is installed
      setOllamaNotInstalled(false);
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to load Ollama models';
      setError(errorMsg);

      // Check if error indicates Ollama is not installed
      if (isOllamaNotInstalledError(errorMsg)) {
        setOllamaNotInstalled(true);
      } else {
        setOllamaNotInstalled(false);
      }

      if (!silent) {
        toast.error(errorMsg);
      }
      console.error('Error loading models:', err);
    } finally {
      setIsLoadingOllama(false);
    }
  };

  // Auto-fetch models on initial load only (not on endpoint changes)
  useEffect(() => {
    let mounted = true;

    const initialLoad = async () => {
      // Only auto-fetch on initial load if:
      // 1. Provider is ollama
      // 2. Haven't fetched yet
      // 3. Component is still mounted
      // If skipInitialFetch is true, fetch silently (no error toasts)
      if (modelConfig.provider === 'ollama' &&
        !hasAutoFetched &&
        mounted) {
        await fetchOllamaModels(skipInitialFetch); // Silent if skipInitialFetch=true
        setHasAutoFetched(true);
      }
    };

    initialLoad();

    return () => {
      mounted = false;
    };
  }, [modelConfig.provider]); // Only depend on provider, NOT endpoint

  const loadOpenRouterModels = async () => {
    if (openRouterModels.length > 0) return; // Already loaded

    try {
      setIsLoadingOpenRouter(true);
      setOpenRouterError('');
      const data = (await invoke('get_openrouter_models')) as OpenRouterModel[];
      setOpenRouterModels(data);
    } catch (err) {
      console.error('Error loading OpenRouter models:', err);
      setOpenRouterError(
        err instanceof Error ? err.message : 'Failed to load OpenRouter models'
      );
    } finally {
      setIsLoadingOpenRouter(false);
    }
  };

  const loadBuiltinAiModels = async () => {
    if (builtinAiModels.length > 0) return; // Already loaded

    try {
      const data = (await invoke('builtin_ai_list_models')) as any[];
      setBuiltinAiModels(data);

      // Auto-select first available model if none selected
      if (data.length > 0 && !modelConfig.model) {
        const firstAvailable = data.find((m: any) => m.status?.type === 'available');
        if (firstAvailable) {
          setModelConfig((prev: ModelConfig) => ({ ...prev, model: firstAvailable.name }));
        }
      }
    } catch (err) {
      console.error('Error loading Built-in AI models:', err);
      toast.error('Failed to load Built-in AI models');
    }
  };

  // Fetch OpenAI models from API
  const loadOpenAIModels = async (key: string | null) => {
    if (!key?.trim()) {
      setOpenaiModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingOpenAI(true);
    try {
      const data = (await invoke('get_openai_models', { apiKey: key })) as OpenAIModel[];
      setOpenaiModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading OpenAI models:', err);
      setOpenaiModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingOpenAI(false);
    }
  };

  // Fetch Anthropic (Claude) models from API
  const loadClaudeModels = async (key: string | null) => {
    if (!key?.trim()) {
      setClaudeModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingClaude(true);
    try {
      const data = (await invoke('get_anthropic_models', { apiKey: key })) as AnthropicModel[];
      setClaudeModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading Claude models:', err);
      setClaudeModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingClaude(false);
    }
  };

  // Fetch Groq models from API
  const loadGroqModels = async (key: string | null) => {
    if (!key?.trim()) {
      setGroqModels([]); // Will use fallback via modelOptions
      return;
    }
    setIsLoadingGroq(true);
    try {
      const data = (await invoke('get_groq_models', { apiKey: key })) as GroqModel[];
      setGroqModels(data.map((m) => m.id));
    } catch (err) {
      console.error('Error loading Groq models:', err);
      setGroqModels([]); // Will use fallback via modelOptions
    } finally {
      setIsLoadingGroq(false);
    }
  };

  // Auto-fetch OpenAI models when provider is openai and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'openai' && apiKey?.trim()) {
      loadOpenAIModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Auto-fetch Claude models when provider is claude and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'claude' && apiKey?.trim()) {
      loadClaudeModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Auto-fetch Groq models when provider is groq and we have an API key
  useEffect(() => {
    if (modelConfig.provider === 'groq' && apiKey?.trim()) {
      loadGroqModels(apiKey);
    }
  }, [modelConfig.provider, apiKey]);

  // Restore cached model when async model lists become available
  useEffect(() => {
    const providerModels = modelOptions[modelConfig.provider];
    if (!providerModels || providerModels.length === 0) return;

    // If current model is already valid, nothing to do
    if (modelConfig.model && providerModels.includes(modelConfig.model)) return;

    // Try to restore from localStorage cache
    const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
    const cachedModel = map[modelConfig.provider];
    if (cachedModel && providerModels.includes(cachedModel)) {
      setModelConfig((prev: ModelConfig) => ({ ...prev, model: cachedModel }));
    }
  }, [models, openRouterModels, builtinAiModels, openaiModels, claudeModels, groqModels, modelConfig.provider]);

  const handleSave = async () => {
    // For cli-agent provider, persist the CLI agent config first
    if (modelConfig.provider === 'cli-agent') {
      try {
        await invoke('api_save_cli_agent_config', {
          preset: cliAgentPreset,
          command: cliAgentPreset === 'custom' ? (cliAgentCommand.trim() || null) : null,
          args: cliAgentPreset === 'custom' ? parseCliAgentArgs(cliAgentArgs) : null,
          timeoutSecs: null,
        });
        console.log('CLI agent config saved successfully');
      } catch (err) {
        console.error('Failed to save CLI agent config:', err);
        toast.error(err instanceof Error ? err.message : 'Failed to save CLI agent configuration');
        return;
      }
    }

    // For custom-openai provider, save the custom config first
    if (modelConfig.provider === 'custom-openai') {
      try {
        await invoke('api_save_custom_openai_config', {
          endpoint: customOpenAIEndpoint.trim(),
          apiKey: customOpenAIApiKey.trim() || null,
          model: customOpenAIModel.trim(),
          maxTokens: customMaxTokens ? parseInt(customMaxTokens, 10) : null,
          temperature: customTemperature ? parseFloat(customTemperature) : null,
          topP: customTopP ? parseFloat(customTopP) : null,
        });
        console.log('Custom OpenAI config saved successfully');
      } catch (err) {
        console.error('Failed to save custom OpenAI config:', err);
        toast.error('Failed to save custom OpenAI configuration');
        return;
      }
    }

    const updatedConfig = {
      ...modelConfig,
      apiKey: typeof apiKey === 'string' ? apiKey.trim() || null : null,
      ollamaEndpoint: modelConfig.provider === 'ollama'
        ? (ollamaEndpoint.trim() || null)
        : (modelConfig.ollamaEndpoint || null),
      // Include custom OpenAI fields
      customOpenAIEndpoint: modelConfig.provider === 'custom-openai' ? customOpenAIEndpoint.trim() : null,
      customOpenAIModel: modelConfig.provider === 'custom-openai' ? customOpenAIModel.trim() : null,
      customOpenAIApiKey: modelConfig.provider === 'custom-openai' && customOpenAIApiKey.trim() ? customOpenAIApiKey.trim() : null,
      maxTokens: modelConfig.provider === 'custom-openai' && customMaxTokens ? parseInt(customMaxTokens, 10) : null,
      temperature: modelConfig.provider === 'custom-openai' && customTemperature ? parseFloat(customTemperature) : null,
      topP: modelConfig.provider === 'custom-openai' && customTopP ? parseFloat(customTopP) : null,
      // CLI agent fields
      cliAgentPreset: modelConfig.provider === 'cli-agent' ? cliAgentPreset : null,
      cliAgentCommand: modelConfig.provider === 'cli-agent' && cliAgentPreset === 'custom' ? (cliAgentCommand.trim() || null) : null,
      cliAgentArgs: modelConfig.provider === 'cli-agent' && cliAgentPreset === 'custom' ? parseCliAgentArgs(cliAgentArgs) : null,
      // For custom-openai use the model name; for cli-agent use the preset id as the model label
      model: modelConfig.provider === 'custom-openai'
        ? customOpenAIModel.trim()
        : modelConfig.provider === 'cli-agent'
          ? cliAgentPreset
          : modelConfig.model,
    };
    setModelConfig(updatedConfig);
    console.log('ModelSettingsModal - handleSave - Updated ModelConfig:', updatedConfig);

    // Persist confirmed model choice to per-provider cache
    if (updatedConfig.model) {
      const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
      map[updatedConfig.provider] = updatedConfig.model;
      localStorage.setItem('providerModelMap', JSON.stringify(map));
    }

    // Update provider-specific key in context
    if (updateProviderApiKey && updatedConfig.apiKey && updatedConfig.provider !== 'custom-openai') {
      updateProviderApiKey(updatedConfig.provider, updatedConfig.apiKey);
    }

    onSave(updatedConfig);
  };

  // Lightweight install probe that drives the preset badge (no toast, no prompt).
  const checkCliAgentInstall = async (preset: string, command: string, argsRaw: string) => {
    if (preset === 'custom' && !command.trim()) {
      setCliAgentInstallStatus('unknown');
      return;
    }
    setCliAgentInstallStatus('checking');
    try {
      await invoke('api_test_cli_agent_connection', {
        preset,
        command: preset === 'custom' ? command.trim() : null,
        args: preset === 'custom' ? parseCliAgentArgs(argsRaw) : null,
      });
      setCliAgentInstallStatus('installed');
    } catch (err) {
      setCliAgentInstallStatus('not-found');
    }
  };

  // Auto-probe install status when cli-agent is active and the preset/command changes.
  useEffect(() => {
    if (modelConfig.provider !== 'cli-agent') return;
    const timer = setTimeout(() => {
      checkCliAgentInstall(cliAgentPreset, cliAgentCommand, cliAgentArgs);
    }, 400); // debounce custom command typing
    return () => clearTimeout(timer);
  }, [modelConfig.provider, cliAgentPreset, cliAgentCommand, cliAgentArgs]);

  // Explicit Test button: surfaces a visible success/error result inline.
  const handleTestCliAgent = async () => {
    if (cliAgentPreset === 'custom' && !cliAgentCommand.trim()) {
      setCliAgentTestResult({ ok: false, message: 'Enter a command to run first.' });
      return;
    }
    setIsTestingCliAgent(true);
    setCliAgentTestResult(null);
    try {
      const result = await invoke<{ status: string; message: string; version?: string }>(
        'api_test_cli_agent_connection',
        {
          preset: cliAgentPreset,
          command: cliAgentPreset === 'custom' ? cliAgentCommand.trim() : null,
          args: cliAgentPreset === 'custom' ? parseCliAgentArgs(cliAgentArgs) : null,
        }
      );
      const detail = result.version ? `${result.message} (${result.version})` : result.message;
      setCliAgentTestResult({ ok: true, message: detail || 'CLI is installed and reachable.' });
      setCliAgentInstallStatus('installed');
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      setCliAgentTestResult({ ok: false, message: errorMsg });
      setCliAgentInstallStatus('not-found');
    } finally {
      setIsTestingCliAgent(false);
    }
  };

  // Test custom OpenAI connection
  const testCustomOpenAIConnection = async () => {
    if (!customOpenAIEndpoint.trim() || !customOpenAIModel.trim()) {
      toast.error('Please enter endpoint URL and model name first');
      return;
    }

    setIsTestingConnection(true);
    try {
      const result = await invoke<{ status: string; message: string }>('api_test_custom_openai_connection', {
        endpoint: customOpenAIEndpoint.trim(),
        apiKey: customOpenAIApiKey.trim() || null,
        model: customOpenAIModel.trim(),
      });
      toast.success(result.message || 'Connection successful!');
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      toast.error(errorMsg);
    } finally {
      setIsTestingConnection(false);
    }
  };

  const handleInputClick = () => {
    if (isApiKeyLocked) {
      setIsLockButtonVibrating(true);
      setTimeout(() => setIsLockButtonVibrating(false), 500);
    }
  };

  // Function to download recommended model
  const downloadRecommendedModel = async () => {
    const recommendedModel = 'gemma3:1b';

    // Prevent duplicate downloads (defense in depth - backend also checks)
    if (isDownloading(recommendedModel)) {
      toast.info(`${recommendedModel} is already downloading`, {
        description: `Progress: ${Math.round(getProgress(recommendedModel) || 0)}%`
      });
      return;
    }

    try {
      const endpoint = ollamaEndpoint.trim() || null;

      // The download will be tracked by the global context via events
      // Progress toasts are shown automatically by OllamaDownloadContext
      await invoke('pull_ollama_model', {
        modelName: recommendedModel,
        endpoint
      });

      // Refresh the models list after successful download
      await fetchOllamaModels(true);

      // Note: Model is NOT auto-selected - user must explicitly choose it
      // This respects the database as the single source of truth
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to download model';
      console.error('Error downloading model:', err);

      // Check if Ollama is not installed and show appropriate error
      if (isOllamaNotInstalledError(errorMsg)) {
        toast.error('Ollama is not installed', {
          description: 'Please download and install Ollama before downloading models.',
          duration: 7000,
          action: {
            label: 'Download',
            onClick: () => invoke('open_external_url', { url: 'https://ollama.com/download' })
          }
        });
        // Update the installation status flag
        setOllamaNotInstalled(true);
      }
      // Other errors are handled by the context
    }
  };

  // Function to delete Ollama model
  const deleteOllamaModel = async (modelName: string) => {
    try {
      const endpoint = ollamaEndpoint.trim() || null;
      await invoke('delete_ollama_model', {
        modelName,
        endpoint
      });

      toast.success(`Model ${modelName} deleted`);
      await fetchOllamaModels(true); // Refresh list
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : 'Failed to delete model';
      toast.error(errorMsg);
      console.error('Error deleting model:', err);
    }
  };

  // Track previous downloading models to detect completions
  const previousDownloadingRef = useRef<Set<string>>(new Set());

  // Refresh models list when download completes
  useEffect(() => {
    const current = downloadingModels;
    const previous = previousDownloadingRef.current;

    // Check if any downloads completed (were in previous, not in current)
    for (const modelName of previous) {
      if (!current.has(modelName)) {
        // Download completed, refresh models list
        console.log(`[ModelSettingsModal] Download completed for ${modelName}, refreshing list`);
        fetchOllamaModels(true);
        break; // Only refresh once even if multiple completed
      }
    }

    // Update ref for next comparison
    previousDownloadingRef.current = new Set(current);
  }, [downloadingModels]);

  // Filter Ollama models based on search query
  const filteredModels = models.filter((model) => {
    if (!searchQuery.trim()) return true;

    const query = searchQuery.toLowerCase();
    const isLoaded = modelConfig.model === model.name;
    const loadedText = isLoaded ? 'loaded' : '';

    return (
      model.name.toLowerCase().includes(query) ||
      model.size.toLowerCase().includes(query) ||
      loadedText.includes(query)
    );
  });

  return (
    <div>
      <div className="flex justify-between items-center mb-4">
        <h3 className="text-lg font-semibold">Model Settings</h3>
      </div>

      <div className="space-y-4">
        <div>
          <Label>Summarization Model</Label>
          <div className="flex space-x-2 mt-1">
            <Select
              value={modelConfig.provider}
              onValueChange={(value) => {
                const provider = value as ModelConfig['provider'];

                // Clear error state when switching providers
                setError('');

                // Save current provider's model to localStorage before switching
                const map = JSON.parse(localStorage.getItem('providerModelMap') || '{}');
                if (modelConfig.model) {
                  map[modelConfig.provider] = modelConfig.model;
                  localStorage.setItem('providerModelMap', JSON.stringify(map));
                }

                // Try to restore cached model for the new provider
                const savedModel = map[provider];
                const providerModels = modelOptions[provider];
                const defaultModel = providerModels && providerModels.length > 0
                  ? providerModels[0]
                  : '';
                const model = (savedModel && providerModels?.includes(savedModel))
                  ? savedModel
                  : defaultModel;

                setModelConfig({
                  ...modelConfig,
                  provider,
                  model,
                });
                // API key is now synced automatically via useEffect watching providerApiKeys

                // Load OpenRouter models only when OpenRouter is selected
                if (provider === 'openrouter') {
                  loadOpenRouterModels();
                }

                // Load Built-in AI models when selected
                if (provider === 'builtin-ai') {
                  loadBuiltinAiModels();
                }

                // Load CLI agent config when selected
                if (provider === 'cli-agent') {
                  cliAgentConfigLoaded.current = true;
                  setCliAgentTestResult(null);
                  invoke<any>('api_get_cli_agent_config').then((config) => {
                    if (config) {
                      setCliAgentPreset(config.preset || 'codex');
                      setCliAgentCommand(config.command || '');
                      setCliAgentArgs(Array.isArray(config.args) ? config.args.join(' ') : '');
                    }
                  }).catch((err) => {
                    console.error('Failed to load CLI agent config:', err);
                  });
                }

                // Load custom OpenAI config when selected
                if (provider === 'custom-openai') {
                  invoke<any>('api_get_custom_openai_config').then((config) => {
                    if (config) {
                      setCustomOpenAIEndpoint(config.endpoint || '');
                      setCustomOpenAIModel(config.model || '');
                      setCustomOpenAIApiKey(config.apiKey || '');
                      setCustomMaxTokens(config.maxTokens?.toString() || '');
                      setCustomTemperature(config.temperature?.toString() || '');
                      setCustomTopP(config.topP?.toString() || '');
                    }
                  }).catch((err) => {
                    console.error('Failed to load custom OpenAI config:', err);
                  });
                }
              }}
            >
              <SelectTrigger>
                <SelectValue placeholder="Select provider" />
              </SelectTrigger>
              <SelectContent className="max-h-64 overflow-y-auto">
                <SelectItem value="builtin-ai">Built-in AI (Offline, No API needed)</SelectItem>
                <SelectItem value="claude">Claude</SelectItem>
                <SelectItem value="cli-agent">CLI Agent (Codex, Claude Code, Gemini)</SelectItem>
                <SelectItem value="custom-openai">Custom Server (OpenAI)</SelectItem>
                <SelectItem value="groq">Groq</SelectItem>
                <SelectItem value="ollama">Ollama</SelectItem>
                <SelectItem value="openai">OpenAI</SelectItem>
                <SelectItem value="openrouter">OpenRouter</SelectItem>
              </SelectContent>
            </Select>

            {modelConfig.provider !== 'builtin-ai' && modelConfig.provider !== 'custom-openai' && modelConfig.provider !== 'cli-agent' && (
              <Popover open={modelComboboxOpen} onOpenChange={setModelComboboxOpen} modal={true}>
                <PopoverTrigger asChild>
                  <Button
                    variant="outline"
                    role="combobox"
                    aria-expanded={modelComboboxOpen}
                    className="flex-1 max-w-[200px] justify-between font-normal"
                  >
                    <span className="truncate">
                      {modelConfig.model || "Select model..."}
                    </span>
                    <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
                  </Button>
                </PopoverTrigger>
                <PopoverContent className="w-[250px] p-0" align="start">
                  <Command>
                    <CommandInput placeholder="Search models..." />
                    <CommandList className="max-h-[300px]">
                      {(modelConfig.provider === 'openrouter' && isLoadingOpenRouter) ||
                       (modelConfig.provider === 'openai' && isLoadingOpenAI) ||
                       (modelConfig.provider === 'claude' && isLoadingClaude) ||
                       (modelConfig.provider === 'groq' && isLoadingGroq) ? (
                        <div className="py-6 text-center text-sm text-muted-foreground">
                          <RefreshCw className="mx-auto h-4 w-4 animate-spin mb-2" />
                          Loading models...
                        </div>
                      ) : (
                        <>
                          <CommandEmpty>No models found.</CommandEmpty>
                          <CommandGroup>
                            {modelOptions[modelConfig.provider]?.map((model) => (
                              <CommandItem
                                key={model}
                                value={model}
                                onSelect={(currentValue) => {
                                  setModelConfig((prev: ModelConfig) => ({ ...prev, model: currentValue }));
                                  setModelComboboxOpen(false);
                                }}
                              >
                                <Check
                                  className={cn(
                                    "mr-2 h-4 w-4",
                                    modelConfig.model === model ? "opacity-100" : "opacity-0"
                                  )}
                                />
                                <span className="truncate">{model}</span>
                              </CommandItem>
                            ))}
                          </CommandGroup>
                        </>
                      )}
                    </CommandList>
                  </Command>
                </PopoverContent>
              </Popover>
            )}
          </div>
        </div>

        {/* Custom OpenAI Configuration Section */}
        {modelConfig.provider === 'custom-openai' && (
          <div className="space-y-4 border-t pt-4">
            <div>
              <Label htmlFor="custom-endpoint">Endpoint URL *</Label>
              <Input
                id="custom-endpoint"
                value={customOpenAIEndpoint}
                onChange={(e) => setCustomOpenAIEndpoint(e.target.value)}
                placeholder="http://localhost:8000/v1"
                className="mt-1"
              />
              <p className="text-xs text-muted-foreground mt-1">
                Base URL of the OpenAI-compatible API
              </p>
            </div>

            <div>
              <Label htmlFor="custom-model">Model Name *</Label>
              <Input
                id="custom-model"
                value={customOpenAIModel}
                onChange={(e) => setCustomOpenAIModel(e.target.value)}
                placeholder="gpt-4, llama-3-70b, etc."
                className="mt-1"
              />
              <p className="text-xs text-muted-foreground mt-1">
                Model identifier to use for requests
              </p>
            </div>

            <div>
              <Label htmlFor="custom-api-key">API Key (optional)</Label>
              <Input
                id="custom-api-key"
                type="password"
                value={customOpenAIApiKey}
                onChange={(e) => setCustomOpenAIApiKey(e.target.value)}
                placeholder="Leave empty if not required"
                className="mt-1"
              />
            </div>

            {/* Advanced Options (Collapsible) */}
            <div>
              <div
                className="flex items-center justify-between cursor-pointer py-2"
                onClick={() => setIsCustomOpenAIAdvancedOpen(!isCustomOpenAIAdvancedOpen)}
              >
                <Label className="cursor-pointer">Advanced Options</Label>
                {isCustomOpenAIAdvancedOpen ? (
                  <ChevronUp className="h-4 w-4 text-muted-foreground" />
                ) : (
                  <ChevronDown className="h-4 w-4 text-muted-foreground" />
                )}
              </div>

              {isCustomOpenAIAdvancedOpen && (
                <div className="space-y-3 pl-2 border-l-2 border-muted mt-2">
                  <div>
                    <Label htmlFor="custom-max-tokens">Max Tokens</Label>
                    <Input
                      id="custom-max-tokens"
                      type="number"
                      value={customMaxTokens}
                      onChange={(e) => setCustomMaxTokens(e.target.value)}
                      placeholder="e.g., 4096"
                      className="mt-1"
                    />
                  </div>
                  <div>
                    <Label htmlFor="custom-temperature">Temperature (0.0-2.0)</Label>
                    <Input
                      id="custom-temperature"
                      type="number"
                      step="0.1"
                      min="0"
                      max="2"
                      value={customTemperature}
                      onChange={(e) => setCustomTemperature(e.target.value)}
                      placeholder="e.g., 0.7"
                      className="mt-1"
                    />
                  </div>
                  <div>
                    <Label htmlFor="custom-top-p">Top P (0.0-1.0)</Label>
                    <Input
                      id="custom-top-p"
                      type="number"
                      step="0.1"
                      min="0"
                      max="1"
                      value={customTopP}
                      onChange={(e) => setCustomTopP(e.target.value)}
                      placeholder="e.g., 0.9"
                      className="mt-1"
                    />
                  </div>
                </div>
              )}
            </div>

            {/* Test Connection Button */}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={testCustomOpenAIConnection}
              disabled={isTestingConnection || !customOpenAIEndpoint.trim() || !customOpenAIModel.trim()}
              className="w-full"
            >
              {isTestingConnection ? (
                <>
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                  Testing Connection...
                </>
              ) : (
                <>
                  <CheckCircle2 className="mr-2 h-4 w-4" />
                  Test Connection
                </>
              )}
            </Button>
          </div>
        )}

        {/* CLI Agent Configuration Section */}
        {modelConfig.provider === 'cli-agent' && (
          <div className="space-y-4 border-t pt-4">
            <div>
              <div className="flex items-center justify-between">
                <Label htmlFor="cli-agent-preset">CLI</Label>
                {/* Install status badge, driven by api_test_cli_agent_connection */}
                {cliAgentInstallStatus === 'checking' && (
                  <span className="inline-flex items-center gap-1 rounded-full bg-muted px-2 py-0.5 text-xs text-muted-foreground">
                    <RefreshCw className="h-3 w-3 animate-spin" />
                    checking…
                  </span>
                )}
                {cliAgentInstallStatus === 'installed' && (
                  <span className="inline-flex items-center gap-1 rounded-full bg-green-100 px-2 py-0.5 text-xs font-medium text-green-700">
                    <CheckCircle2 className="h-3 w-3" />
                    installed
                  </span>
                )}
                {cliAgentInstallStatus === 'not-found' && (
                  <span className="inline-flex items-center gap-1 rounded-full bg-red-100 px-2 py-0.5 text-xs font-medium text-red-700">
                    <XCircle className="h-3 w-3" />
                    not found
                  </span>
                )}
              </div>
              <Select
                value={cliAgentPreset}
                onValueChange={(value) => {
                  setCliAgentPreset(value);
                  setCliAgentTestResult(null);
                  setCliAgentInstallStatus('unknown');
                }}
              >
                <SelectTrigger id="cli-agent-preset" className="mt-1">
                  <SelectValue placeholder="Select a CLI" />
                </SelectTrigger>
                <SelectContent>
                  {CLI_AGENT_PRESETS.map((preset) => (
                    <SelectItem key={preset.id} value={preset.id}>
                      {preset.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground mt-1">
                Runs your installed AI CLI as a one-shot subprocess. No API key needed — it uses the
                CLI&apos;s own login. The model is decided by the CLI itself.
              </p>
            </div>

            {/* Custom command fields */}
            {cliAgentPreset === 'custom' && (
              <>
                <div>
                  <Label htmlFor="cli-agent-command">Command *</Label>
                  <Input
                    id="cli-agent-command"
                    value={cliAgentCommand}
                    onChange={(e) => setCliAgentCommand(e.target.value)}
                    placeholder="/usr/local/bin/my-llm"
                    className="mt-1"
                  />
                  <p className="text-xs text-muted-foreground mt-1">
                    Binary that reads the prompt from stdin and writes markdown to stdout.
                  </p>
                </div>
                <div>
                  <Label htmlFor="cli-agent-args">Arguments (optional)</Label>
                  <Input
                    id="cli-agent-args"
                    value={cliAgentArgs}
                    onChange={(e) => setCliAgentArgs(e.target.value)}
                    placeholder="run --stdin"
                    className="mt-1"
                  />
                  <p className="text-xs text-muted-foreground mt-1">
                    Space-separated. The transcript is always delivered via stdin, never as an argument.
                  </p>
                </div>
              </>
            )}

            {/* Speaker attribution discoverability */}
            <Alert>
              <AlertDescription className="text-xs">
                With speaker diarization done and the <strong>&quot;Include speakers in summary&quot;</strong>{' '}
                toggle enabled (Diarization settings), summaries attribute decisions and action items by
                speaker name. If that toggle is off, the transcript is sent without names and the summary
                won&apos;t reference who said what.
              </AlertDescription>
            </Alert>

            {/* Privacy note */}
            <Alert className="border-yellow-500 bg-yellow-50">
              <AlertDescription className="text-xs text-yellow-800">
                Privacy: the full transcript — including any assigned speaker names — is sent to the chosen
                CLI, which forwards it to that subscription provider&apos;s servers. This leaves your machine,
                unlike the fully local Built-in AI / Ollama options.
              </AlertDescription>
            </Alert>

            {/* Test button + inline result */}
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleTestCliAgent}
              disabled={isTestingCliAgent || (cliAgentPreset === 'custom' && !cliAgentCommand.trim())}
              className="w-full"
            >
              {isTestingCliAgent ? (
                <>
                  <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                  Testing…
                </>
              ) : (
                <>
                  <CheckCircle2 className="mr-2 h-4 w-4" />
                  Test CLI
                </>
              )}
            </Button>

            {cliAgentTestResult && (
              <Alert className={cn(cliAgentTestResult.ok ? 'border-green-500 bg-green-50' : 'border-red-500 bg-red-50')}>
                <AlertDescription className={cn('text-xs', cliAgentTestResult.ok ? 'text-green-800' : 'text-red-800')}>
                  {cliAgentTestResult.message}
                </AlertDescription>
              </Alert>
            )}
          </div>
        )}

        {requiresApiKey && (
          <div>
            <Label>API Key</Label>
            <div className="relative mt-1">
              <Input
                type={showApiKey ? 'text' : 'password'}
                value={apiKey || ''}
                onChange={(e) => setApiKey(e.target.value)}
                disabled={isApiKeyLocked}
                placeholder="Enter your API key"
                className="pr-24"
              />
              {isApiKeyLocked && apiKey?.trim() && (
                <div
                  onClick={handleInputClick}
                  className="absolute inset-0 flex items-center justify-center bg-muted/50 rounded-md cursor-not-allowed"
                />
              )}
              <div className="absolute inset-y-0 right-0 pr-1 flex items-center space-x-1">
                {apiKey?.trim() && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    onClick={() => setIsApiKeyLocked(!isApiKeyLocked)}
                    className={isLockButtonVibrating ? 'animate-vibrate text-red-500' : ''}
                    title={isApiKeyLocked ? 'Unlock to edit' : 'Lock to prevent editing'}
                  >
                    {isApiKeyLocked ? <Lock /> : <Unlock />}
                  </Button>
                )}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  onClick={() => setShowApiKey(!showApiKey)}
                >
                  {showApiKey ? <EyeOff /> : <Eye />}
                </Button>
              </div>
            </div>
          </div>
        )}

        {modelConfig.provider === 'ollama' && (
          <div>
            <div
              className="flex items-center justify-between cursor-pointer py-2"
              onClick={() => setIsEndpointSectionCollapsed(!isEndpointSectionCollapsed)}
            >
              <Label className="cursor-pointer">Custom Endpoint (optional)</Label>
              {isEndpointSectionCollapsed ? (
                <ChevronDown className="h-4 w-4 text-muted-foreground" />
              ) : (
                <ChevronUp className="h-4 w-4 text-muted-foreground" />
              )}
            </div>

            {!isEndpointSectionCollapsed && (
              <>
                <p className="text-sm text-muted-foreground mt-1 mb-2">
                  Leave empty or enter a custom endpoint (e.g., http://x.yy.zz:11434)
                </p>
                <div className="flex gap-2 mt-1">
                  <div className="relative flex-1">
                    <Input
                      type="url"
                      value={ollamaEndpoint}
                      onChange={(e) => {
                        setOllamaEndpoint(e.target.value);
                        // Clear models and errors when endpoint changes to avoid showing stale data
                        if (e.target.value.trim() !== lastFetchedEndpoint.trim()) {
                          setModels([]);
                          setError(''); // Clear error state
                        }
                      }}
                      placeholder="http://localhost:11434"
                      className={cn(
                        "pr-10",
                        endpointValidationState === 'invalid' && "border-red-500"
                      )}
                    />
                    {endpointValidationState === 'valid' && (
                      <CheckCircle2 className="absolute right-3 top-1/2 -translate-y-1/2 h-5 w-5 text-green-500" />
                    )}
                    {endpointValidationState === 'invalid' && (
                      <XCircle className="absolute right-3 top-1/2 -translate-y-1/2 h-5 w-5 text-red-500" />
                    )}
                  </div>
                  <Button
                    type="button"
                    size={'sm'}
                    onClick={() => fetchOllamaModels()}
                    disabled={isLoadingOllama}
                    variant="outline"
                    className="whitespace-nowrap"
                  >
                    {isLoadingOllama ? (
                      <>
                        <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                        Fetching...
                      </>
                    ) : (
                      <>
                        <RefreshCw className="mr-2 h-4 w-4" />
                        Fetch Models
                      </>
                    )}
                  </Button>
                </div>
                {ollamaEndpointChanged && !error && (
                  <Alert className="mt-3 border-yellow-500 bg-yellow-50">
                    <AlertDescription className="text-yellow-800">
                      Endpoint changed. Please click "Fetch Models" to load models from the new endpoint before saving.
                    </AlertDescription>
                  </Alert>
                )}
              </>
            )}
          </div>
        )}

        {modelConfig.provider === 'ollama' && (
          <div>
            <div className="flex items-center justify-between mb-4">
              <h4 className="text-sm font-bold">Available Ollama Models</h4>
              {lastFetchedEndpoint && models.length > 0 && (
                <div className="flex items-center gap-2 text-sm">
                  <span className="text-muted-foreground">Using:</span>
                  <code className="px-2 py-1 bg-muted rounded text-xs">
                    {lastFetchedEndpoint || 'http://localhost:11434'}
                  </code>
                </div>
              )}
            </div>
            {models.length > 0 && (
              <div className="mb-4">
                <Input
                  placeholder="Search models..."
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="w-full"
                />
              </div>
            )}
            {isLoadingOllama ? (
              <div className="text-center py-8 text-muted-foreground">
                <RefreshCw className="mx-auto h-8 w-8 animate-spin mb-2" />
                Loading models...
              </div>
            ) : models.length === 0 ? (
              <div className="space-y-3">
                {ollamaNotInstalled ? (
                  /* Show Ollama download link when not installed */
                  <div className="space-y-4">
                    <Alert className="border-orange-500 bg-orange-50">
                      <AlertDescription className="text-orange-800">
                        Ollama is not installed or not running. Please download and install Ollama to use local models.
                      </AlertDescription>
                    </Alert>
                    <Button
                      variant="default"
                      size="sm"
                      onClick={() => invoke('open_external_url', { url: 'https://ollama.com/download' })}
                      className="w-full bg-blue-600 hover:bg-blue-700"
                    >
                      <ExternalLink className="mr-2 h-4 w-4" />
                      Download Ollama
                    </Button>
                    <div className="text-sm text-muted-foreground text-center">
                      After installing Ollama, restart this application and click "Fetch Models" to continue.
                    </div>
                  </div>
                ) : (
                  /* Show model download option when Ollama is installed but no models */
                  <>
                    <Alert className="mb-4">
                      <AlertDescription>
                        {ollamaEndpointChanged
                          ? 'Endpoint changed. Click "Fetch Models" to load models from the new endpoint.'
                          : 'No models found. Download a recommended model or click "Fetch Models" to load available Ollama models.'}
                      </AlertDescription>
                    </Alert>
                    {!ollamaEndpointChanged && (
                      <div className="space-y-3">
                        <Button
                          variant="outline"
                          size="sm"
                          onClick={downloadRecommendedModel}
                          disabled={isDownloading('gemma3:1b')}
                          className="w-full"
                        >
                          {isDownloading('gemma3:1b') ? (
                            <>
                              <RefreshCw className="mr-2 h-4 w-4 animate-spin" />
                              Downloading gemma3:1b...
                            </>
                          ) : (
                            <>
                              <Download className="mr-2 h-4 w-4" />
                              Download gemma3:1b (Recommended, ~800MB)
                            </>
                          )}
                        </Button>

                        {/* Show progress for gemma3:1b download */}
                        {isDownloading('gemma3:1b') && getProgress('gemma3:1b') !== undefined && (
                          <div className="bg-white rounded-md border p-3">
                            <div className="flex items-center justify-between mb-2">
                              <span className="text-sm font-medium text-blue-600">Downloading gemma3:1b</span>
                              <span className="text-sm font-semibold text-blue-600">
                                {Math.round(getProgress('gemma3:1b')!)}%
                              </span>
                            </div>
                            <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
                              <div
                                className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all duration-300"
                                style={{ width: `${getProgress('gemma3:1b')}%` }}
                              />
                            </div>
                          </div>
                        )}
                      </div>
                    )}
                  </>
                )}
              </div>
            ) : !ollamaEndpointChanged && (
              <ScrollArea className="max-h-[calc(100vh-450px)] overflow-y-auto pr-4">
                {filteredModels.length === 0 ? (
                  <Alert>
                    <AlertDescription>
                      No models found matching "{searchQuery}". Try a different search term.
                    </AlertDescription>
                  </Alert>
                ) : (
                  <div className="grid gap-4">
                    {filteredModels.map((model) => {
                      const progress = getProgress(model.name);
                      const modelIsDownloading = isDownloading(model.name);

                      return (
                        <div
                          key={model.id}
                          className={cn(
                            'bg-card p-2 m-0 rounded-md border transition-colors',
                            modelConfig.model === model.name
                              ? 'ring-1 ring-blue-500 border-blue-500 background-blue-100'
                              : 'hover:bg-muted/50',
                            !modelIsDownloading && 'cursor-pointer'
                          )}
                          onClick={() => {
                            if (!modelIsDownloading) {
                              setModelConfig((prev: ModelConfig) => ({ ...prev, model: model.name }))
                            }
                          }}
                        >
                          <div>
                            <b className="font-bold">{model.name}&nbsp;</b>
                            <span className="text-muted-foreground">with a size of </span>
                            <span className="font-mono font-bold text-sm">{model.size}</span>
                          </div>

                          {/* Progress bar for downloading models */}
                          {modelIsDownloading && progress !== undefined && (
                            <div className="mt-3 pt-3 border-t border-gray-200">
                              <div className="flex items-center justify-between mb-2">
                                <span className="text-sm font-medium text-blue-600">Downloading...</span>
                                <span className="text-sm font-semibold text-blue-600">{Math.round(progress)}%</span>
                              </div>
                              <div className="w-full h-2 bg-gray-200 rounded-full overflow-hidden">
                                <div
                                  className="h-full bg-gradient-to-r from-blue-500 to-blue-600 rounded-full transition-all duration-300"
                                  style={{ width: `${progress}%` }}
                                />
                              </div>
                            </div>
                          )}
                        </div>
                      );
                    })}
                  </div>
                )}
              </ScrollArea>
            )}
          </div>
        )}

        {/* Built-in AI Models Section */}
        {modelConfig.provider === 'builtin-ai' && (
          <div className="mt-6">
            <BuiltInModelManager
              selectedModel={modelConfig.model}
              layout={layout}
              onModelSelect={(model) =>
                setModelConfig((prev: ModelConfig) => ({ ...prev, model }))
              }
            />
          </div>
        )}
      </div>

      {/* Auto-generate summaries toggle */}
      {/* <div className="mt-6 pt-6 border-t border-gray-200">
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <Label htmlFor="auto-generate" className="text-base font-medium">
              Auto-generate summaries
            </Label>
            <p className="text-sm text-muted-foreground mt-1">
              Automatically generate summary when opening meetings without one
            </p>
          </div>
          <Switch
            id="auto-generate"
            checked={autoGenerateEnabled}
            onCheckedChange={setAutoGenerateEnabled}
          />
        </div>
      </div> */}

      <div className="mt-6 flex justify-end">
        <Button
          className={cn(
            'px-4 text-sm font-medium text-white rounded-md focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-blue-500',
            isDoneDisabled ? 'bg-gray-400 cursor-not-allowed' : 'bg-blue-600 hover:bg-blue-700'
          )}
          onClick={handleSave}
          disabled={isDoneDisabled}
        >
          Save
        </Button>
      </div>
    </div>
  );
}
