#!/usr/bin/env python3
"""Scripted IE115 engine: drafts then commits, to look at the overlay."""
import asyncio, json, sys
from websockets.asyncio.server import unix_serve
D = "conversation.item.input_audio_transcription.delta"
SCRIPT = [
    ("unstable", "So the first step"),
    ("unstable", "So the first step was to actually"),
    ("committed", "So the first step was to actually create the new flavor."),
    ("unstable", "I found a company online"),
    ("unstable", "I found a company online that said they could custom"),
    ("committed", "I found a company online that said they could custom-make any flavor."),
    ("unstable", "And after a week of waiting, the flavor"),
]
async def session(ws):
    await ws.send(json.dumps({"type": "session.created", "session": {}}))
    async def talk():
        await asyncio.sleep(1)
        for disp, text in SCRIPT:
            await ws.send(json.dumps({"type": D, "item_id": "i1", "delta": text, "disposition": disp}))
            await asyncio.sleep(float(sys.argv[2]))
    t = asyncio.create_task(talk())
    async for _ in ws:
        pass
    t.cancel()
async def main():
    async with unix_serve(session, sys.argv[1]):
        await asyncio.Future()
asyncio.run(main())
