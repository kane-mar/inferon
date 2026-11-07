// SPDX-FileCopyrightText: 2026 Kane Mar
// SPDX-License-Identifier: AGPL-3.0-only

export type SnippetLang = 'python' | 'node' | 'curl';

export const SNIPPET_LANGS: SnippetLang[] = ['python', 'node', 'curl'];

/**
 * Used when a tenant has no catalog model of their own (e.g. a provider with
 * free-form ids).
 */
export const DEFAULT_MODEL = 'deepseek-chat';

/** `https://host[/]` + the OpenAI-compatible path → `https://host/v1`. */
export const apiBaseUrl = (appUrl: string): string => `${appUrl.replace(/\/+$/, '')}/v1`;

/**
 * A copy-paste client for the given language, against the public base URL and a
 * tenant key. `model` defaults to the platform model so the snippet works for a
 * shared-key tenant and an own-provider one (ITEM-048 AC4). Pure so it is
 * unit-tested; the UI just renders the string.
 */
export const buildSnippet = (
  lang: SnippetLang,
  appUrl: string,
  apiKey: string,
  model: string = DEFAULT_MODEL,
): string => {
  const base = apiBaseUrl(appUrl);
  switch (lang) {
    case 'python':
      return `from openai import OpenAI

client = OpenAI(base_url="${base}", api_key="${apiKey}")

chat = client.chat.completions.create(
    model="${model}",
    messages=[{"role": "user", "content": "Say hi"}],
)
print(chat.choices[0].message.content)`;
    case 'node':
      return `import OpenAI from 'openai';

const client = new OpenAI({ baseURL: '${base}', apiKey: '${apiKey}' });

const chat = await client.chat.completions.create({
  model: '${model}',
  messages: [{ role: 'user', content: 'Say hi' }],
});
console.log(chat.choices[0].message.content);`;
    case 'curl':
      return `curl ${base}/chat/completions \\
  -H "Authorization: Bearer ${apiKey}" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"${model}","messages":[{"role":"user","content":"Say hi"}]}'`;
  }
};

/** File hint for the snippet's window chrome. */
export const snippetFile = (lang: SnippetLang): string =>
  lang === 'python' ? 'main.py' : lang === 'node' ? 'index.ts' : 'request.sh';
