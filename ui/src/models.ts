/**
 * The models the chat window offers. This is a shortcut, not a whitelist: Settings.provider
 * picks the endpoint, and `turn_send` accepts any model id the provider knows. A model that is
 * not in this list still shows up as the current choice, so a config edited elsewhere is never
 * silently replaced.
 */
export interface ModelChoice {
  id: string;
  label: string;
  hint?: string;
}

const CHOICES: Record<string, ModelChoice[]> = {
  omniroute: [
    { id: "gpt-5.6-sol-medium", label: "GPT-5.6 Sol Medium", hint: "default" },
    { id: "openai/gpt-5.4-mini", label: "GPT-5.4 Mini", hint: "cheaper" },
    { id: "anthropic/claude-sonnet-4.5", label: "Claude Sonnet 4.5" },
    { id: "openai/gpt-4.1-mini", label: "GPT-4.1 Mini", hint: "cheaper" },
    { id: "google/gemini-2.5-flash", label: "Gemini 2.5 Flash", hint: "cheaper" },
  ],
  openrouter: [
    { id: "openai/gpt-5.4-mini", label: "GPT-5.4 Mini" },
    { id: "anthropic/claude-sonnet-4.5", label: "Claude Sonnet 4.5" },
    { id: "google/gemini-2.5-flash", label: "Gemini 2.5 Flash", hint: "cheaper" },
    { id: "openai/gpt-4.1-mini", label: "GPT-4.1 Mini", hint: "cheaper" },
    { id: "anthropic/claude-haiku-4.5", label: "Claude Haiku 4.5", hint: "cheaper" },
  ],
  gemini: [
    { id: "gemini-4-argon", label: "Gemini 4 Argon", hint: "default" },
    { id: "gemini-3.8-flash", label: "Gemini 3.8 Flash", hint: "cheaper" },
  ],
};

export function modelChoices(provider: string, current: string): ModelChoice[] {
  const choices = CHOICES[provider] ?? CHOICES.omniroute;
  if (!current || choices.some((choice) => choice.id === current)) return choices;
  return [{ id: current, label: `${current} (current)` }, ...choices];
}
