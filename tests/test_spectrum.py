#!/usr/bin/env python3
from __future__ import annotations

import math
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "backend"))

from protocol import PROTOCOL_VERSION, encode_line, spectrum_event  # noqa: E402
from spectrum import (  # noqa: E402
    NUM_BANDS,
    SAMPLE_RATE,
    WINDOW,
    analyze,
    fft_magnitudes,
)


def sine(freq: float, n: int = WINDOW, sr: int = SAMPLE_RATE, amp: float = 0.6) -> list[float]:
    return [amp * math.sin(2.0 * math.pi * freq * i / sr) for i in range(n)]


class SpectrumTests(unittest.TestCase):
    def test_empty_samples_decay(self):
        previous = [1.0] * NUM_BANDS
        decayed = analyze([], previous)
        self.assertEqual(len(decayed), NUM_BANDS)
        self.assertTrue(all(abs(value - 0.8) < 1e-9 for value in decayed))

    def test_fft_peaks_on_tone(self):
        samples = sine(1000)
        magnitudes = fft_magnitudes([samples[i] * 0.5 * (1.0 - math.cos(2.0 * math.pi * i / (WINDOW - 1)))
                                     for i in range(WINDOW)])
        bin_hz = SAMPLE_RATE / WINDOW
        peak = max(range(1, len(magnitudes)), key=lambda i: magnitudes[i])
        self.assertAlmostEqual(peak * bin_hz, 1000, delta=bin_hz * 2)

    def test_thousand_hertz_dominates_its_band(self):
        levels = [0.0] * NUM_BANDS
        for _ in range(8):
            levels = analyze(sine(1000), levels)
        # Band edges: 800-1600 Hz is index 4.
        self.assertGreater(levels[4], 0.15)
        self.assertGreater(levels[4], levels[0])
        self.assertGreater(levels[4], levels[9])

    def test_bass_tone_stays_in_low_bands(self):
        levels = [0.0] * NUM_BANDS
        for _ in range(8):
            levels = analyze(sine(70), levels)
        self.assertGreater(levels[0], levels[8])
        self.assertGreater(levels[0], 0.05)

    def test_spectrum_event_clamps_and_rounds(self):
        payload = spectrum_event([-0.4, 0.1234, 2.0] + [0.0] * 7)
        self.assertEqual(payload["type"], "event")
        self.assertEqual(payload["event"], "spectrum")
        self.assertEqual(payload["v"], PROTOCOL_VERSION)
        self.assertEqual(payload["bands"][0], 0.0)
        self.assertEqual(payload["bands"][1], 0.123)
        self.assertEqual(payload["bands"][2], 1.0)
        line = encode_line(payload)
        self.assertTrue(line.endswith(b"\n"))
        self.assertLess(len(line), 512)


if __name__ == "__main__":
    unittest.main()
