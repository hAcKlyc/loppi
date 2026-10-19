#!/usr/bin/env python3
"""Compare compiled component identities. Does not start GUI/Core/model workers."""
import argparse
import json
from pathlib import Path
import subprocess


def inspect(paths):
    identities = {}
    for component, path in paths.items():
        flag = 'build-info' if component == 'cli' else '--build-info'
        info = json.loads(subprocess.check_output([str(path.resolve()), flag], text=True, timeout=15))
        if info['component'] != component or info['git_sha'] == 'unknown':
            raise ValueError(f'{component}: missing or incorrect build identity')
        identities[component] = info
    shared = [{k: v for k,v in info.items() if k != 'component'} for info in identities.values()]
    if any(value != shared[0] for value in shared[1:]):
        raise ValueError('mixed component builds; rebuild the complete bundle with one HEAR_BUILD_EPOCH')
    return {'schema': 1, 'components': identities}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument('--directory', type=Path)
    group.add_argument('--bundle', type=Path)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.bundle:
        paths = {'gui':args.bundle/'Contents/MacOS/hear-desktop', 'cli':args.bundle/'Contents/MacOS/hear',
                 'core':args.bundle/'Contents/Library/LoginItems/HearCore.app/Contents/MacOS/hear-core',
                 'worker':args.bundle/'Contents/Library/LoginItems/HearCore.app/Contents/MacOS/hear-worker'}
    else:
        paths = {role:args.directory/name for role,name in [('gui','hear-desktop'),('core','hear-core'),('worker','hear-worker'),('cli','hear')]}
    data = json.dumps(inspect(paths), indent=2)+'\n'
    if args.output:
        args.output.write_text(data)
    else:
        print(data, end='')


if __name__ == '__main__':
    main()
