"""Observe the original frontend gate and capture an owned Moli stall."""
from __future__ import annotations

import asyncio
import hashlib
import json
from pathlib import Path
import time

from moli_frontend_smoke import runner
from moli_frontend_smoke.cdp import RawCdpClient


async def main() -> int:
    args = runner.parse_args()
    output = Path(args.output).resolve()
    output.mkdir(parents=True, exist_ok=True)
    original_command = RawCdpClient.command
    original_start = runner.start_moli
    active: dict[int, dict] = {}
    case_by_session: dict[str, str] = {}
    owned_browser = None
    done = asyncio.Event()
    trace = (output / 'cdp-commands.jsonl').open('x')

    async def start_moli(binary):
        nonlocal owned_browser
        owned_browser = await original_start(binary)
        with Path(binary).open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        assert digest == '4f8cd63ea4fc48768c39727282557ece99c4985c09a74959d10c8576bc5a4cb2'
        (output / 'owned-process.json').write_text(json.dumps({
            'pid': owned_browser.process.pid, 'binary': str(binary), 'sha256': digest,
        }, indent=2) + '\n')
        return owned_browser

    async def command(self, method, params=None, **kwargs):
        if owned_browser is None:
            return await original_command(self, method, params, **kwargs)
        session = kwargs.get('session_id')
        if method == 'Page.navigate' and session:
            case_by_session[session] = (params or {}).get('url', '')
        row = dict(id=self.next_id, session=session, case=case_by_session.get(session),
                   method=method, params=params, started=time.monotonic())
        active[id(self)] = row
        trace.write(json.dumps(dict(event='start', **row)) + '\n')
        trace.flush()
        try:
            result = await original_command(self, method, params, **kwargs)
            row['status'] = 'replied'
            return result
        except BaseException as error:
            row.update(status='failed', error_type=type(error).__name__, error=str(error))
            row['unmatched_responses'] = [event for event in self.events if 'id' in event]
            raise
        finally:
            row['elapsed'] = time.monotonic() - row['started']
            active.pop(id(self), None)
            trace.write(json.dumps(dict(event='complete', **row)) + '\n')
            trace.flush()

    async def monitor():
        while not done.is_set():
            for row in list(active.values()):
                if row['method'] != 'Runtime.evaluate':
                    continue
                if 'fragment-anchor-target' not in (row['case'] or ''):
                    continue
                if time.monotonic() - row['started'] < 2:
                    continue
                assert owned_browser is not None
                pid = owned_browser.process.pid
                (output / 'stall-command.json').write_text(json.dumps(row, indent=2) + '\n')
                status = Path('/proc') / str(pid) / 'status'
                (output / 'stall-process-status.txt').write_text(status.read_text())
                (output / 'stall-memory-info.txt').write_text(Path('/proc/meminfo').read_text())
                print('Capturing Moli stall: ' + str(pid) + ' ' + row['case'], flush=True)
                with (output / 'stall-backtrace.txt').open('x') as stream:
                    debug = await asyncio.create_subprocess_exec(
                        'gdb', '-q', '-nx', '-batch', '-p', str(pid),
                        '-ex', 'set pagination off', '-ex', 'thread apply all bt 40', '-ex', 'detach',
                        stdout=stream, stderr=asyncio.subprocess.STDOUT)
                    try:
                        await asyncio.wait_for(debug.wait(), timeout=20)
                    except TimeoutError:
                        debug.kill()
                        await debug.wait()
                (output / 'gdb-result.json').write_text(json.dumps({
                    'returncode': debug.returncode,
                }) + '\n')
                return
            try:
                await asyncio.wait_for(done.wait(), timeout=.1)
            except TimeoutError:
                pass

    runner.start_moli = start_moli
    RawCdpClient.command = command
    monitor_task = asyncio.create_task(monitor())
    try:
        code, summary = await runner.run(args)
        print(json.dumps({'ok': summary['ok'], 'counts': summary['counts']}, indent=2), flush=True)
        return code
    finally:
        done.set()
        await monitor_task
        trace.close()


if __name__ == '__main__':
    raise SystemExit(asyncio.run(main()))
