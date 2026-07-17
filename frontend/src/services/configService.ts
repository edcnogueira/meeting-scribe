/**
 * Configuration Service
 *
 * Handles all configuration-related Tauri backend calls.
 * Pure 1-to-1 wrapper - no error handling changes, exact same behavior as direct invoke calls.
 */

import { invoke } from '@tauri-apps/api/core';
import { TranscriptModelProps } from '@/components/TranscriptSettings';

export interface ModelConfig {
  provider: 'ollama' | 'groq' | 'claude' | 'openrouter' | 'openai' | 'builtin-ai' | 'custom-openai' | 'cli-agent';
  model: string;
  whisperModel: string;
  /**
   * @deprecated Use providerApiKeys from ConfigContext instead.
   * This field may contain stale data when provider changes without saving.
   */
  apiKey?: string | null;
  ollamaEndpoint?: string | null;
  // Custom OpenAI fields (only populated when provider is 'custom-openai')
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

export interface CustomOpenAIConfig {
  endpoint: string;
  apiKey: string | null;
  model: string;
  maxTokens: number | null;
  temperature: number | null;
  topP: number | null;
}

/**
 * CLI agent summary provider configuration.
 *
 * The provider spawns an installed AI CLI (codex / claude / gemini) as a
 * one-shot subprocess. Named presets carry their own command + args; the
 * `custom` preset requires an explicit `command` (and optional `args`).
 *
 * NOTE: `timeoutSecs` matches the backend field rename (`#[serde(rename = "timeoutSecs")]`).
 */
export interface CliAgentConfig {
  preset: 'codex' | 'claude' | 'gemini' | 'custom';
  command?: string | null;
  args?: string[] | null;
  timeoutSecs?: number | null;
}

export interface RecordingPreferences {
  preferred_mic_device: string | null;
  preferred_system_device: string | null;
}

/**
 * Configuration Service
 * Singleton service for managing app configuration
 */
export class ConfigService {
  /**
   * Get saved transcript model configuration
   * @returns Promise with { provider, model, apiKey }
   */
  async getTranscriptConfig(): Promise<TranscriptModelProps> {
    return invoke<TranscriptModelProps>('api_get_transcript_config');
  }

  /**
   * Get saved summary model configuration
   * @returns Promise with { provider, model, whisperModel }
   */
  async getModelConfig(): Promise<ModelConfig> {
    return invoke<ModelConfig>('api_get_model_config');
  }

  /**
   * Get saved audio device preferences
   * @returns Promise with { preferred_mic_device, preferred_system_device }
   */
  async getRecordingPreferences(): Promise<RecordingPreferences> {
    return invoke<RecordingPreferences>('get_recording_preferences');
  }

  /**
   * Get custom OpenAI configuration
   * @returns Promise with CustomOpenAIConfig or null if not configured
   */
  async getCustomOpenAIConfig(): Promise<CustomOpenAIConfig | null> {
    return invoke<CustomOpenAIConfig | null>('api_get_custom_openai_config');
  }

  /**
   * Save custom OpenAI configuration
   * @param config - CustomOpenAIConfig to save
   * @returns Promise with result status
   */
  async saveCustomOpenAIConfig(config: CustomOpenAIConfig): Promise<{ status: string; message: string }> {
    return invoke<{ status: string; message: string }>('api_save_custom_openai_config', {
      endpoint: config.endpoint,
      apiKey: config.apiKey,
      model: config.model,
      maxTokens: config.maxTokens,
      temperature: config.temperature,
      topP: config.topP,
    });
  }

  /**
   * Test custom OpenAI connection
   * @param endpoint - API endpoint URL
   * @param apiKey - Optional API key
   * @param model - Model name
   * @returns Promise with test result
   */
  async testCustomOpenAIConnection(
    endpoint: string,
    apiKey: string | null,
    model: string
  ): Promise<{ status: string; message: string; http_status?: number }> {
    return invoke<{ status: string; message: string; http_status?: number }>('api_test_custom_openai_connection', {
      endpoint,
      apiKey,
      model,
    });
  }

  /**
   * Get the saved CLI agent configuration.
   * @returns Promise with CliAgentConfig or null if not configured
   */
  async getCliAgentConfig(): Promise<CliAgentConfig | null> {
    return invoke<CliAgentConfig | null>('api_get_cli_agent_config');
  }

  /**
   * Save the CLI agent configuration.
   * @param config - CliAgentConfig to save (command/args required only for the 'custom' preset)
   * @returns Promise with result status
   */
  async saveCliAgentConfig(config: CliAgentConfig): Promise<{ status: string; message: string }> {
    return invoke<{ status: string; message: string }>('api_save_cli_agent_config', {
      preset: config.preset,
      command: config.command ?? null,
      args: config.args ?? null,
      timeoutSecs: config.timeoutSecs ?? null,
    });
  }

  /**
   * Test that the CLI agent is installed and reachable (runs its `--version`;
   * no prompt is sent, so the user's quota is untouched).
   * @param preset - Preset identifier (codex/claude/gemini/custom)
   * @param command - Binary to run (required for the 'custom' preset)
   * @param args - Optional custom arguments
   * @returns Promise with test result (throws with an actionable message on failure)
   */
  async testCliAgentConnection(
    preset: string,
    command: string | null,
    args: string[] | null
  ): Promise<{ status: string; message: string; version?: string }> {
    return invoke<{ status: string; message: string; version?: string }>('api_test_cli_agent_connection', {
      preset,
      command,
      args,
    });
  }
}

// Export singleton instance
export const configService = new ConfigService();
