#!/usr/bin/env python3
"""Read-only media coverage audit for an explicit test interval; no transcript access."""
import argparse
import json
import os
import selectors
import time
import shutil
import sqlite3
import subprocess
from pathlib import Path
from uuid import UUID


def coverage(intervals, start, end):
    cursor, missing, overlaps, examples = start, 0, 0, []
    for a, b in intervals:
        a, b = max(start, a), min(end, b)
        if b <= a:
            continue
        if a > cursor:
            missing += a - cursor
            if len(examples) < 100:
                examples.append([cursor, a])
        elif a < cursor:
            overlaps += min(b, cursor) - a
        cursor = max(cursor, b)
    if cursor < end:
        missing += end - cursor
        if len(examples) < 100:
            examples.append([cursor, end])
    return {"covered_ms": end-start-missing, "missing_ms": missing,
            "overlap_ms": overlaps, "first_100_gaps": examples}


def decode_frames(path, ffmpeg):
    # One decoder at a time, no complete PCM recording in memory. ffmpeg also
    # validates Ogg/Opus packets; a failed/short decode is not a successful audit.
    process = subprocess.Popen([ffmpeg, "-v", "error", "-xerror", "-i", str(path),
                                "-ac", "1", "-ar", "48000", "-f", "f32le", "pipe:1"],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    count = 0
    deadline=time.monotonic()+30
    selector=selectors.DefaultSelector()
    os.set_blocking(process.stdout.fileno(),False)
    selector.register(process.stdout,selectors.EVENT_READ)
    try:
        while True:
            remaining=deadline-time.monotonic()
            if remaining<=0 or not selector.select(timeout=remaining):
                raise ValueError("decode_timeout")
            data=os.read(process.stdout.fileno(),65536)
            if not data: break
            count+=len(data)
            if count>48_000*11*4: raise ValueError("decoded_block_exceeds_limit")
        if process.wait(timeout=max(.001,deadline-time.monotonic())) != 0 or count % 4:
            raise ValueError("decode_failed")
        return count // 4
    finally:
        selector.close()
        process.stdout.close()
        if process.poll() is None:
            process.kill()
            process.wait()


def audit(directory, start, end, decode=False):
    ffmpeg = shutil.which("ffmpeg") if decode else None
    if decode and not ffmpeg:
        raise ValueError("--decode requires ffmpeg")
    db = sqlite3.connect(f"file:{(directory/'hear.sqlite').as_posix()}?mode=ro", uri=True, timeout=3)
    db.execute("PRAGMA query_only=ON")
    result = {"from_ms": start, "to_ms": end, "decode_requested": decode, "sources": {},
              "limits": "Manifest coverage and optional decode integrity are separate from known-reference sound and speech-retention recall. No reference or quality pass is implied. Run against a stopped isolated capture for a stable final audit."}
    try:
        for source in ("microphone", "system"):
            def intervals(media=False):
                return db.execute("SELECT started_ms,ended_ms FROM audio_blocks WHERE source=? AND started_ms<? AND ended_ms>? "
                                  + ("AND media_path IS NOT NULL " if media else "") + "ORDER BY started_ms,ended_ms", (source,end,start))
            item = {"manifest": coverage(intervals(), start,end), "retained_media": coverage(intervals(True),start,end),
                    "blocks": 0, "retained_files": 0, "decoded_files": 0, "decode_frame_mismatches": 0,
                    "file_errors": 0, "first_100_file_errors": [], "frames": 0, "decisions": {}}
            def media_rows():
                after=(-1,'')
                while True:
                    page=db.execute("SELECT started_ms,id,frames,media_path,retention_decision FROM audio_blocks "
                        "WHERE source=? AND started_ms<? AND ended_ms>? AND (started_ms,id)>(?,?) "
                        "ORDER BY started_ms,id LIMIT 512",(source,end,start,*after)).fetchall()
                    if not page: return
                    after=(page[-1][0],page[-1][1])
                    for row in page: yield row[1:]
            for id,frames,relative,decision in media_rows():
                item['blocks'] += 1; item['frames'] += frames
                item['decisions'][decision] = item['decisions'].get(decision, 0) + 1
                if relative is None:
                    continue
                item['retained_files'] += 1
                try:
                    UUID(id)
                    expected = Path('media')/source/f'{id}.opus'
                    if Path(relative) != expected:
                        raise ValueError('invalid_manifest_path')
                    path = directory/expected
                    if path.is_symlink() or not path.is_file():
                        raise ValueError('missing_or_invalid_file')
                    if decode:
                        actual = decode_frames(path, ffmpeg)
                        item['decoded_files'] += 1
                        if actual != frames:
                            item['decode_frame_mismatches'] += 1
                except (OSError, ValueError, subprocess.SubprocessError) as error:
                    item['file_errors'] += 1
                    if len(item['first_100_file_errors']) < 100:
                        item['first_100_file_errors'].append({'id':id,'error':str(error)})
            item['declared_gap_count'] = db.execute("SELECT count(*) FROM media_gaps WHERE source=? AND started_ms<? AND coalesce(ended_ms,?)>?", (source,end,end,start)).fetchone()[0]
            result['sources'][source] = item
    finally:
        db.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--data-dir',type=Path,required=True)
    parser.add_argument('--from-ms',type=int,required=True)
    parser.add_argument('--to-ms',type=int,required=True)
    parser.add_argument('--decode',action='store_true')
    parser.add_argument('--output',type=Path,required=True)
    args = parser.parse_args()
    if args.from_ms < 0 or args.to_ms <= args.from_ms:
        parser.error('invalid time interval')
    result = audit(args.data_dir.resolve(), args.from_ms, args.to_ms, args.decode)
    args.output.write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
    print(f"Coverage audit saved to {args.output}; quality/reference audio is not asserted.")


if __name__ == '__main__':
    main()
