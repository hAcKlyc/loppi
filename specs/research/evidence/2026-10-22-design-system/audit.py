"""Read-only design inventory. Run from any directory; prints JSON to stdout.

This is a lexical inventory, not a CSS parser or a complete accessibility audit.
Opaque token pairs use WCAG sRGB relative luminance, without screenshot sampling.
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
css_files = sorted((ROOT / 'src').glob('*.css'))
ui_files = [p for p in (ROOT / 'src').glob('*') if p.suffix in {'.css', '.ts', '.tsx'}]
source = '\n'.join(p.read_text() for p in ui_files)
tokens = (ROOT / 'src/tokens.css').read_text()
definitions = set(re.findall(r'["\']?(--[\w-]+)["\']?\s*:', source))
references = set(re.findall(r'var\((--[\w-]+)', source))
colors = dict(re.findall(r'(--[\w-]+):\s*(#[\da-fA-F]+);', tokens))


def luminance(hex_color):
    value = hex_color.lstrip('#')
    if len(value) == 3:
        value = ''.join(c * 2 for c in value)
    assert len(value) == 6, 'Only opaque sRGB colors are supported'
    channels = [int(value[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    linear = [c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in channels]
    return sum(c * weight for c, weight in zip(linear, (0.2126, 0.7152, 0.0722)))


def contrast(foreground, background):
    low, high = sorted((luminance(foreground), luminance(background)))
    return (high + 0.05) / (low + 0.05)


foregrounds = ['--color-text', '--color-text-strong', '--color-text-secondary',
               '--color-text-muted', '--color-text-faint', '--color-focus',
               '--color-warning', '--color-warning-strong', '--color-danger']
backgrounds = ['--color-surface', '--color-canvas', '--color-sidebar',
               '--color-recording-surface', '--color-recording-surface-hover', '--color-danger-surface']
literal_colors = []
literal_font_sizes = []
for path in css_files:
    if path.name == 'tokens.css':
        continue
    for line, value in enumerate(path.read_text().splitlines(), 1):
        record = {'file': str(path.relative_to(ROOT)), 'line': line, 'value': value.strip()}
        if re.search(r'#[\da-fA-F]{3,8}\b|\b(?:rgb|hsl)a?\(', value):
            literal_colors.append(record)
        if re.search(r'font-size:\s*[\d.]', value):
            literal_font_sizes.append(record)

report = {
    'scope': 'Current src/*.css, src/*.ts and src/*.tsx; lexical inventory only',
    'css_lines': {str(p.relative_to(ROOT)): len(p.read_text().splitlines()) for p in css_files},
    'token_count': len(re.findall(r'^\s*--[\w-]+\s*:', tokens, re.M)),
    'undefined_css_variables': sorted(references - definitions),
    'literal_colors_outside_tokens': literal_colors,
    'literal_font_sizes_outside_tokens': literal_font_sizes,
    'control_pairs': [
        {'control': label, 'foreground': f, 'background': b, 'ratio': contrast(colors[f], colors[b])}
        for label, f, b in [
            ('primary', '--color-surface', '--color-brand'),
            ('primary hover', '--color-surface', '--color-brand-hover'),
            ('secondary / create-event', '--color-brand', '--color-surface-selected'),
            ('secondary / create-event hover', '--color-brand', '--color-surface-selected-hover'),
            ('quiet', '--color-brand-accent', '--color-control'),
            ('quiet hover', '--color-brand-accent', '--color-control-hover'),
            ('danger', '--color-surface', '--color-danger'),
            ('danger hover', '--color-surface', '--color-danger-hover'),
            ('focus on selected hover', '--color-focus', '--color-surface-selected-hover'),
        ]
    ],
    'contrast': [
        {'foreground': f, 'background': b, 'ratio': contrast(colors[f], colors[b])}
        for f in foregrounds for b in backgrounds
    ],
}
print(json.dumps(report, ensure_ascii=False, indent=2))
