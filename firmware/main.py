"""Pico firmware. Reads a BLE heart rate strap and prints each reading to USB serial.

Bare integer lines are readings. Every other line starts with '#' so the host can ignore it.
"""

import asyncio

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


def set_led(on):
    if led is not None:
        led.value(1 if on else 0)


def read_bpm(data):
    """Returns None while the strap reports that it is not touching skin."""
    flags = data[0]
    if flags & 0x01:
        bpm = data[1] | (data[2] << 8)
    else:
        bpm = data[1]
    contact_supported = flags & 0x04
    contact_detected = flags & 0x02
    if contact_supported and not contact_detected:
        return None
    return bpm or None


async def find_strap():
    wanted = DEVICE_NAME.lower()
    async with aioble.scan(8000, interval_us=30000, window_us=30000, active=True) as scanner:
        async for result in scanner:
            name = result.name() or ""
            if HR_SERVICE not in result.services():
                continue
            if wanted and wanted not in name.lower():
                continue
            print("# found", name or result.device.addr_hex())
            return result.device
    return None


async def stream(device):
    connection = await device.connect(timeout_ms=10000)
    async with connection:
        service = await connection.service(HR_SERVICE)
        characteristic = await service.characteristic(HR_MEASUREMENT)
        await characteristic.subscribe(notify=True)
        print("# subscribed")
        set_led(True)
        quiet = False
        while True:
            bpm = read_bpm(await characteristic.notified(timeout_ms=15000))
            if bpm is None:
                if not quiet:
                    print("# no skin contact")
                    quiet = True
                continue
            quiet = False
            print(bpm)


async def main():
    while True:
        set_led(False)
        try:
            device = await find_strap()
            if device is None:
                print("# no strap found, rescanning")
                await asyncio.sleep(2)
                continue
            await stream(device)
        except (aioble.DeviceDisconnectedError, asyncio.TimeoutError):
            print("# strap disconnected")
        except Exception as error:
            print("# error:", repr(error))
            await asyncio.sleep(2)


asyncio.run(main())
