#!/usr/bin/env python3
"""Types text into the VM with the QEMU monitor's `sendkey` (US layout).

Usage (on the Proxmox host):  python3 lord-type.py 115 'ls -la\n'
Special keys in braces: {ret} {tab} {esc} {up} {down} {left} {right} {f12}
{ctrl-c} {meta_l-t} … (any `sendkey` name). Requires LordOS to use the
US layout (ES/US button in the menu bar)."""
import subprocess
import sys
import time

SHIFTED = {'!': '1', '@': '2', '#': '3', '$': '4', '%': '5', '^': '6', '&': '7', '*': '8', '(': '9', ')': '0',
           '_': 'minus', '+': 'equal', '{': 'bracket_left', '}': 'bracket_right', '|': 'backslash', ':': 'semicolon',
           '"': 'apostrophe', '<': 'comma', '>': 'dot', '?': 'slash', '~': 'grave_accent'}
PLAIN = {' ': 'spc', '-': 'minus', '=': 'equal', '[': 'bracket_left', ']': 'bracket_right', '\\': 'backslash',
         ';': 'semicolon', "'": 'apostrophe', ',': 'comma', '.': 'dot', '/': 'slash', '`': 'grave_accent',
         '\n': 'ret', '\t': 'tab'}


def keys(text):
    i = 0
    while i < len(text):
        c = text[i]
        if c == '{' and '}' in text[i:]:
            j = text.index('}', i)
            yield text[i + 1:j]
            i = j + 1
            continue
        if c.isalpha() and c.isascii():
            yield f'shift-{c.lower()}' if c.isupper() else c
        elif c.isdigit():
            yield c
        elif c in SHIFTED:
            yield f'shift-{SHIFTED[c]}'
        elif c in PLAIN:
            yield PLAIN[c]
        i += 1


def main():
    vmid, text = sys.argv[1], sys.argv[2].encode().decode('unicode_escape')
    ks = list(keys(text))
    # In batches: QEMU's keyboard queue drops keys with long bursts.
    for i in range(0, len(ks), 40):
        cmds = ''.join(f'sendkey {k}\n' for k in ks[i:i + 40])
        subprocess.run(['qm', 'monitor', vmid], input=cmds.encode(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
        time.sleep(0.15)


if __name__ == '__main__':
    main()
