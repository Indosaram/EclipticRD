import { test, expect } from 'bun:test';
import { readFileSync, existsSync } from 'node:fs';

for (const configPath of ['../tauri.conf.json', '../src-tauri/tauri.conf.json']) {
  const configUrl = new URL(configPath, import.meta.url);
  const config = JSON.parse(readFileSync(configUrl, 'utf8'));

  test(`${configPath} produces an app bundle with resolvable icons`, () => {
    expect(config.bundle.active).toBe(true);
    expect(config.bundle.icon.some(path => path.endsWith('.icns'))).toBe(true);
    for (const path of config.bundle.icon) {
      expect(existsSync(new URL(path, configUrl))).toBe(true);
    }
  });

  test(`${configPath} supplies RGBA8 PNG data to Tauri`, () => {
    for (const path of config.bundle.icon.filter(path => path.endsWith('.png'))) {
      const png = readFileSync(new URL(path, configUrl));
      expect(png.subarray(0, 8)).toEqual(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
      expect(png.toString('ascii', 12, 16)).toBe('IHDR');
      expect(png[24]).toBe(8);
      expect(png[25]).toBe(6);
    }
  });
}
