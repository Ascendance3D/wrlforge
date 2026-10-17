'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

function hexToRgb(hex) {
  hex = hex.replace(/^#/, '');
  if (hex.length === 3) {
    hex = hex.split('').map(char => char + char).join('');
  }
  return {
    r: parseInt(hex.substring(0, 2), 16),
    g: parseInt(hex.substring(2, 4), 16),
    b: parseInt(hex.substring(4, 6), 16)
  };
}

function getLuminance(rgb) {
  const a = [rgb.r, rgb.g, rgb.b].map(v => {
    v /= 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  });
  return a[0] * 0.2126 + a[1] * 0.7152 + a[2] * 0.0722;
}

function getContrastRatio(color1, color2) {
  const l1 = getLuminance(hexToRgb(color1));
  const l2 = getLuminance(hexToRgb(color2));
  const lightest = Math.max(l1, l2);
  const darkest = Math.min(l1, l2);
  return (lightest + 0.05) / (darkest + 0.05);
}

test('Contrast formula self-check', () => {
  assert.equal(getContrastRatio('#000000', '#ffffff').toFixed(2), '21.00');
  assert.equal(getContrastRatio('#777777', '#ffffff').toFixed(2), '4.48');
});

test('Parse themes.css and test contrast', async (t) => {
  const cssFile = path.join(__dirname, '../../apps/desktop-tauri/ui/static/themes.css');
  const css = fs.readFileSync(cssFile, 'utf-8');

  // --wf-text-disabled is exempt (WCAG 1.4.3 exempts inactive components)

  const themeRegex = /:root\[data-theme="([^"]+)"\]\s*\{([^}]+)\}/g;
  let match;
  const parsedThemes = new Set();

  while ((match = themeRegex.exec(css)) !== null) {
    const theme = match[1];
    const properties = match[2];
    parsedThemes.add(theme);

    const vars = {};
    const varRegex = /(--[^:]+):\s*(#[0-9a-fA-F]+);/g;
    let varMatch;
    while ((varMatch = varRegex.exec(properties)) !== null) {
      vars[varMatch[1]] = varMatch[2];
    }

    const bgs = ['--wf-bg-app', '--wf-surface-editor'];
    const texts = ['--wf-text-primary', '--wf-text-secondary', '--wf-text-muted', '--wf-text-placeholder'];

    for (const bg of bgs) {
      assert.ok(vars[bg], `Theme ${theme} is missing background token ${bg}`);
      for (const text of texts) {
        assert.ok(vars[text], `Theme ${theme} is missing text token ${text}`);

        const ratio = getContrastRatio(vars[text], vars[bg]);

        await t.test(`Theme ${theme}: ${text} on ${bg}`, () => {
          assert.ok(ratio >= 4.5, `Theme ${theme}: ${text} (${vars[text]}) on ${bg} (${vars[bg]}) fails contrast ratio: ${ratio.toFixed(2)}`);
        });
      }
    }
  }

  assert.deepEqual(Array.from(parsedThemes).sort(), ['tokyo-night', 'tokyo-night-light', 'tokyo-night-storm'].sort(), 'Should parse exactly the expected themes');
});
