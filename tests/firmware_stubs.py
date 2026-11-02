from __future__ import annotations

import asyncio
import importlib.util
import sys
from contextlib import contextmanager
from dataclasses import dataclass
from pathlib import Path
from types import ModuleType
from typing import TYPE_CHECKING, ClassVar, Protocol, cast
from unittest.mock import patch

if TYPE_CHECKING:
    from collections.abc import Coroutine, Generator


class GattError(Exception):
    pass


class DeviceDisconnectedError(Exception):
    pass


class Device:
    pass


class Pin:
    OUT: ClassVar[int] = 1

    def __init__(self, name: str, mode: int) -> None:
        self.name = name
        self.mode = mode
        self.values: list[int] = []

    def value(self, level: int) -> None:
        self.values.append(level)


class Runtime(ModuleType):
    TimeoutError: ClassVar[type[TimeoutError]] = TimeoutError

    def __init__(self) -> None:
        super().__init__("asyncio")
        self.delays: list[float] = []
        self.boot_calls = 0

    def run(self, task: Coroutine[object, object, None]) -> None:
        self.boot_calls += 1
        task.close()

    async def sleep(self, seconds: float) -> None:
        self.delays.append(seconds)
        await asyncio.sleep(0)


class Radio(ModuleType):
    GattError: ClassVar[type[GattError]] = GattError
    DeviceDisconnectedError: ClassVar[type[DeviceDisconnectedError]] = DeviceDisconnectedError
    Device: ClassVar[type[Device]] = Device


class Bluetooth(ModuleType):
    UUID: ClassVar[type[int]] = int


class Machine(ModuleType):
    Pin: ClassVar[type[Pin]] = Pin


class Firmware(Protocol):
    led: Pin | None

    def read_bpm(self, data: bytes) -> int | None: ...

    def set_led(self, *, on: bool) -> None: ...

    async def main(self) -> None: ...

    async def scan_and_stream(self) -> None: ...


@dataclass
class FirmwareFixture:
    module: ModuleType
    logic: Firmware
    runtime: Runtime


@contextmanager
def load_firmware() -> Generator[FirmwareFixture, None, None]:
    runtime = Runtime()
    replacements = {
        "asyncio": runtime,
        "aioble": Radio("aioble"),
        "bluetooth": Bluetooth("bluetooth"),
        "machine": Machine("machine"),
    }
    source = Path(__file__).resolve().parents[1] / "firmware" / "main.py"
    specification = importlib.util.spec_from_file_location("heartwire_firmware", source)
    if specification is None or specification.loader is None:
        message = f"cannot load firmware test module from {source}"
        raise ImportError(message)
    module = importlib.util.module_from_spec(specification)
    with patch.dict(sys.modules, replacements):
        specification.loader.exec_module(module)
    yield FirmwareFixture(module, cast("Firmware", module), runtime)
