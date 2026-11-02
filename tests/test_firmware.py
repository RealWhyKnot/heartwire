from __future__ import annotations

import asyncio
import io
import unittest
from contextlib import redirect_stdout
from unittest.mock import AsyncMock, patch

from tests.firmware_stubs import DeviceDisconnectedError, GattError, load_firmware


class FirmwareTests(unittest.TestCase):
    def test_boot_is_intercepted_without_starting_the_radio(self) -> None:
        with load_firmware() as fixture:
            self.assertEqual(fixture.runtime.boot_calls, 1)

    def test_measurements_decode_both_widths_and_contact_flags(self) -> None:
        with load_firmware() as fixture:
            for flags in range(256):
                for rate in [0, 1, 72, 255, 256, 511, 65535]:
                    with self.subTest(flags=flags, rate=rate):
                        packet = bytes([flags, rate & 255, rate >> 8])
                        expected = rate if flags & 1 else rate & 255
                        if flags & 4 and not flags & 2:
                            expected = 0
                        self.assertEqual(fixture.logic.read_bpm(packet), expected or None)

    def test_led_values_follow_connection_state(self) -> None:
        with load_firmware() as fixture:
            fixture.logic.set_led(on=True)
            fixture.logic.set_led(on=False)
            led = fixture.logic.led
            self.assertIsNotNone(led)
            if led is not None:
                self.assertEqual(led.values, [1, 0])

    def test_boards_without_an_led_keep_running(self) -> None:
        with load_firmware() as fixture:
            fixture.logic.led = None
            fixture.logic.set_led(on=True)
            self.assertIsNone(fixture.logic.led)

    def test_missing_strap_keeps_the_rescan_message_and_delay(self) -> None:
        with load_firmware() as fixture:
            output = io.StringIO()
            with patch.object(fixture.module, "find_strap", AsyncMock(return_value=None)), redirect_stdout(output):
                asyncio.run(fixture.logic.scan_and_stream())
            self.assertEqual(output.getvalue(), "# no strap found, rescanning\n")
            self.assertEqual(fixture.runtime.delays, [2])

    def test_transport_and_subscription_errors_retry_after_two_seconds(self) -> None:
        for error in [OSError("radio busy"), GattError(5), ValueError("CCCD not found")]:
            with self.subTest(error=type(error).__name__), load_firmware() as fixture:
                output = io.StringIO()
                work = AsyncMock(side_effect=[error, RuntimeError("stop test")])
                with (
                    patch.object(fixture.module, "scan_and_stream", work),
                    redirect_stdout(output),
                    self.assertRaises(RuntimeError),
                ):
                    asyncio.run(fixture.logic.main())
                self.assertEqual(output.getvalue(), "# error: " + repr(error) + "\n")
                self.assertEqual(fixture.runtime.delays, [2])
                self.assertEqual(work.await_count, 2)

    def test_disconnects_and_timeouts_keep_their_existing_output(self) -> None:
        for error in [DeviceDisconnectedError(), TimeoutError()]:
            with self.subTest(error=type(error).__name__), load_firmware() as fixture:
                output = io.StringIO()
                work = AsyncMock(side_effect=[error, RuntimeError("stop test")])
                with (
                    patch.object(fixture.module, "scan_and_stream", work),
                    redirect_stdout(output),
                    self.assertRaises(RuntimeError),
                ):
                    asyncio.run(fixture.logic.main())
                self.assertEqual(output.getvalue(), "# strap disconnected\n")
                self.assertEqual(fixture.runtime.delays, [])
                self.assertEqual(work.await_count, 2)

    def test_unexpected_failures_propagate_without_retrying(self) -> None:
        for error in [RuntimeError("bug"), AttributeError("missing service"), IndexError("short packet")]:
            with self.subTest(error=type(error).__name__), load_firmware() as fixture:
                output = io.StringIO()
                work = AsyncMock(side_effect=error)
                with (
                    patch.object(fixture.module, "scan_and_stream", work),
                    redirect_stdout(output),
                    self.assertRaises(type(error)),
                ):
                    asyncio.run(fixture.logic.main())
                self.assertEqual(output.getvalue(), "")
                self.assertEqual(fixture.runtime.delays, [])
                self.assertEqual(work.await_count, 1)


if __name__ == "__main__":
    unittest.main()
