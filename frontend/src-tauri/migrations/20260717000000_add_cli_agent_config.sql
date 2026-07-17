-- Migration: Add CLI agent provider configuration
--
-- Stores the CLI agent summary provider config as JSON:
-- {preset, command, args, timeoutSecs}
-- Mirrors the customOpenAIConfig column pattern.
ALTER TABLE settings ADD COLUMN cliAgentConfig TEXT;
