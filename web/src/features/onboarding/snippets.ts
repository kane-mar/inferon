export type SnippetLang = 'python' | 'node' | 'curl';

export const SNIPPET_LANGS: SnippetLang[] = ['python', 'node', 'curl'];

/** `https://host[/]` + the OpenAI-compatible path → `https://host/v1`. */
export const apiBaseUrl = (appUrl: string): string => `${appUrl.replace(/\/+$/, '')}/v1`;

/**
 * A copy-paste client for the given language, against the public base URL and a
 * tenant key. Pure so it is unit-tested; the UI just renders the string.
 */
export const buildSnippet = (lang: SnippetLang, appUrl: string, apiKey: string): string => {
  const base = apiBaseUrl(appUrl);
  switch (lang) {
    case 'python':
      return `from openai import OpenAI

client = OpenAI(base_url="${base}", api_key="${apiKey}")

chat = client.chat.completions.create(
    model="deepseek-chat",
    messages=[{"role": "user", "content": "Say hi"}],
)
print(chat.choices[0].message.content)`;
    case 'node':
      return `import OpenAI from 'openai';

const client = new OpenAI({ baseURL: '${base}', apiKey: '${apiKey}' });

const chat = await client.chat.completions.create({
  model: 'deepseek-chat',
  messages: [{ role: 'user', content: 'Say hi' }],
});
console.log(chat.choices[0].message.content);`;
    case 'curl':
      return `curl ${base}/chat/completions \\
  -H "Authorization: Bearer ${apiKey}" \\
  -H "Content-Type: application/json" \\
  -d '{"model":"deepseek-chat","messages":[{"role":"user","content":"Say hi"}]}'`;
  }
};

/** File hint for the snippet's window chrome. */
export const snippetFile = (lang: SnippetLang): string =>
  lang === 'python' ? 'main.py' : lang === 'node' ? 'index.ts' : 'request.sh';
