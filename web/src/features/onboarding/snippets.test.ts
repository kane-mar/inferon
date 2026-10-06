import { describe, expect, it } from 'vitest';
import { apiBaseUrl, buildSnippet, SNIPPET_LANGS, snippetFile } from './snippets';

describe('apiBaseUrl', () => {
  it('appends /v1 and normalises trailing slashes', () => {
    expect(apiBaseUrl('https://inferon.example.com')).toBe('https://inferon.example.com/v1');
    expect(apiBaseUrl('https://inferon.example.com/')).toBe('https://inferon.example.com/v1');
    expect(apiBaseUrl('http://localhost:3000//')).toBe('http://localhost:3000/v1');
  });
});

describe('buildSnippet', () => {
  it('embeds the base URL and key in every language', () => {
    for (const lang of SNIPPET_LANGS) {
      const snippet = buildSnippet(lang, 'https://inferon.example.com', 'sk-inferon-test');

      expect(snippet).toContain('https://inferon.example.com/v1');
      expect(snippet).toContain('sk-inferon-test');
      expect(snippet).toContain('deepseek-chat');
    }
  });

  it('uses the tenant model when provided (ITEM-048 AC4)', () => {
    expect(buildSnippet('python', 'https://x.test', 'k', 'gpt-4o')).toContain('model="gpt-4o"');
    expect(buildSnippet('node', 'https://x.test', 'k', 'gpt-4o')).toContain('model: \'gpt-4o\'');
    expect(buildSnippet('curl', 'https://x.test', 'k', 'gpt-4o')).toContain('"model":"gpt-4o"');
  });

  it('falls back to the platform model when none is provided', () => {
    expect(buildSnippet('python', 'https://x.test', 'k')).toContain('model="deepseek-chat"');
  });

  it('uses the OpenAI SDK for python and node', () => {
    expect(buildSnippet('python', 'https://x.test', 'k')).toContain('from openai import OpenAI');
    expect(buildSnippet('node', 'https://x.test', 'k')).toContain('import OpenAI');
  });

  it('builds a curl call with a bearer header', () => {
    const curl = buildSnippet('curl', 'https://x.test', 'k');

    expect(curl).toContain('/v1/chat/completions');
    expect(curl).toContain('Authorization: Bearer k');
  });

  it('has a file hint per language', () => {
    expect(snippetFile('python')).toBe('main.py');
    expect(snippetFile('node')).toBe('index.ts');
    expect(snippetFile('curl')).toBe('request.sh');
  });
});
