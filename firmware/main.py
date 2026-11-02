import asyncio
import sys

import aioble
import bluetooth
from machine import Pin

HR_SERVICE = bluetooth.UUID(0x180D)
HR_MEASUREMENT = bluetooth.UUID(0x2A37)

try:
    from hr_config import DEVICE_NAME
except ImportError:
    DEVICE_NAME = ""

try:
    led = Pin("LED", Pin.OUT)
except (TypeError, ValueError):
    led = None


def set_led(*, on: bool) -> None:
    if led is not None:
        led.value(1 if on else 0)


def read_bpm(data: bytes) -> int | None:
    flags = data[0]
    bpm = data[1] | (data[2] << 8) if flags & 0x01 else data[1]
    contact_supported = flags & 0x04
    contact_detected = flags & 0x02
    if contact_supported and not contact_detected:
        return None
    return bpm or None


async def find_strap() -> aioble.Device | None:
    wanted = DEVICE_NAME.lower()
    async with aioble.scan(8000, interval_us=30000, window_us=30000, active=True) as scanner:
        async for result in scanner:
            name = result.name() or ""
            if HR_SERVICE not in result.services():
                continue
            if wanted and wanted not in name.lower():
                continue
            sys.stdout.write("# found " + (name or result.device.addr_hex()) + "\n")
            return result.device
    return None


async def stream(device: aioble.Device) -> None:
    connection = await device.connect(timeout_ms=10000)
    async with connection:
        service = await connection.service(HR_SERVICE)
        characteristic = await service.characteristic(HR_MEASUREMENT)
        await characteristic.subscribe(notify=True)
        sys.stdout.write("# subscribed\n")
        set_led(on=True)
        quiet = False
        while True:
            bpm = read_bpm(await characteristic.notified(timeout_ms=15000))
            if bpm is None:
                if not quiet:
                    sys.stdout.write("# no skin contact\n")
                    quiet = True
                continue
            quiet = False
            sys.stdout.write(str(bpm) + "\n")


async def scan_and_stream() -> None:
    device = await find_strap()
    if device is None:
        sys.stdout.write("# no strap found, rescanning\n")
        await asyncio.sleep(2)
        return
    await stream(device)


async def main() -> None:
    while True:
        set_led(on=False)
        try:
            await scan_and_stream()
        except (aioble.DeviceDisconnectedError, asyncio.TimeoutError):
            sys.stdout.write("# strap disconnected\n")
        except (OSError, aioble.GattError, ValueError) as error:
            sys.stdout.write("# error: " + repr(error) + "\n")
            await asyncio.sleep(2)


asyncio.run(main())
