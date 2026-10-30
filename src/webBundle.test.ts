// @vitest-environment node
/**
 * The web app bundled in web/ ships no source maps (owner, 27 Oct 2026): this
 * repository and its installers are public, and a map hands out the whole
 * readable source of lexpad_front. scripts/build-web.sh builds without them
 * and refuses to finish if one appears; this test keeps a stray one out of a
 * commit (it runs in `pnpm check` and in CI).
 */
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const WEB = fileURLToPath(new URL('../web', import.meta.url));

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

describe('web/', () => {
  const all = files(WEB);

  it('holds the web app', () => {
    expect(all.some((path) => path.endsWith('index.html'))).toBe(true);
  });

  it('has no source map', () => {
    expect(all.filter((path) => path.endsWith('.map'))).toEqual([]);
  });

  it('points at no source map', () => {
    const pointing = all
      .filter((path) => /\.(js|css|mjs)$/u.test(path))
      .filter((path) => readFileSync(path, 'utf8').includes('sourceMappingURL='));
    expect(pointing).toEqual([]);
  });
});
