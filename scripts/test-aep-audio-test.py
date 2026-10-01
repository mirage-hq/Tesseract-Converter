#!/usr/bin/env python3
"""Offline synthetic gate tests. No Adobe, conversion, upload, or render."""

import hashlib
import json
import math
import struct
import subprocess
import sys
import tempfile
import unittest
import wave
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).parent))
from aep_audio_test import AudioTestError, compare, decode, validate_policy

SCRIPT = Path(__file__).with_name("aep-test.py")


def wav(path, frames):
    with wave.open(str(path), "wb") as writer:
        writer.setnchannels(2)
        writer.setsampwidth(2)
        writer.setframerate(48000)
        writer.writeframes(b"".join(struct.pack("<hh", round(l * 32767), round(r * 32767))
                                    for l, r in frames))


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class AudioGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.base = [(0.0, 0.0) if i < 4800 else
                     (0.3 * math.sin(2 * math.pi * 400 * i / 48000),
                      0.2 * math.sin(2 * math.pi * 730 * i / 48000))
                     for i in range(48000)]
        self.ref = self.root / "ref.wav"
        self.actual = self.root / "actual.wav"
        wav(self.ref, self.base)
        self.policy = {"sample_rate": 48000, "channels": 2, "duration_seconds": 1.0,
                       "duration_tolerance_seconds": 0.001, "relative_rms_error_max": 0.05,
                       "window_rms_error_max": 0.003, "silence_reference_rms_max": 0.0001,
                       "silence_leak_rms_max": 0.0005}

    def score(self, frames):
        wav(self.actual, frames)
        return compare(self.actual, self.ref, self.policy)

    def test_identical_passes_with_channel_metrics(self):
        result = self.score(self.base)
        self.assertTrue(result["passed"])
        self.assertEqual(len(result["channel_results"]), 2)
        self.assertTrue(all(ch["error_rms"] == 0 for ch in result["channel_results"]))

    def test_wrong_gain_rejected(self):
        self.assertFalse(self.score([(a * 0.8, b * 0.8) for a, b in self.base])["passed"])

    def test_hold_gain_envelope_accepts_match_rejects_wrong_key_time_and_gain(self):
        def envelope(key_frame, later_gain):
            return [(a * (0.25 if i < key_frame else later_gain),
                     b * (0.25 if i < key_frame else later_gain))
                    for i, (a, b) in enumerate(self.base)]

        expected = envelope(24000, 0.8)
        wav(self.ref, expected)
        self.assertTrue(self.score(expected)["passed"])
        self.assertFalse(self.score(envelope(27000, 0.8))["passed"])
        self.assertFalse(self.score(envelope(24000, 0.6))["passed"])

    def test_linear_gain_envelope_accepts_match_rejects_slope_and_gain(self):
        def envelope(gain_at):
            return [(a * gain_at(i / 48000), b * gain_at(i / 48000))
                    for i, (a, b) in enumerate(self.base)]

        expected = envelope(lambda time: 0.2 + 0.6 * time)
        wav(self.ref, expected)
        self.assertTrue(self.score(expected)["passed"])
        self.assertFalse(self.score(envelope(lambda time: 0.2 + 0.6 * time * time))["passed"])
        self.assertFalse(self.score(envelope(lambda time: 0.3 + 0.6 * time))["passed"])

    def test_shift_rejected_without_alignment(self):
        self.assertFalse(self.score([(0, 0)] * 480 + self.base[:-480])["passed"])

    def test_mute_rejected(self):
        self.assertFalse(self.score([(0, 0)] * len(self.base))["passed"])

    def test_swapped_channels_rejected(self):
        result = self.score([(b, a) for a, b in self.base])
        self.assertFalse(result["passed"])
        self.assertTrue(all(not ch["passed"] for ch in result["channel_results"]))

    def test_channel_only_mute_rejected(self):
        result = self.score([(a, 0) for a, _ in self.base])
        self.assertTrue(result["channel_results"][0]["passed"])
        self.assertFalse(result["channel_results"][1]["passed"])

    def test_silent_reference_allows_bounded_leak_but_not_excess(self):
        wav(self.ref, [(0, 0)] * len(self.base))
        quiet = self.score([(0.0003, 0) for _ in self.base])
        self.assertTrue(quiet["passed"])
        self.assertIsNone(quiet["channel_results"][0]["relative_rms_error"])
        self.assertFalse(self.score([(0.001, 0) for _ in self.base])["passed"])
        self.assertFalse(self.score([(0.004, 0) for _ in self.base])["passed"])

    def test_non_silent_reference_still_requires_relative_threshold(self):
        wav(self.ref, [(0.001, 0) for _ in self.base])
        result = self.score([(0.0012, 0) for _ in self.base])
        self.assertFalse(result["channel_results"][0]["passed"])
        self.assertLess(result["channel_results"][0]["worst_window_rms_error"], 0.003)

    def test_silence_leak_rejected(self):
        changed = [(0.02, b) if i < 4800 else (a, b)
                   for i, (a, b) in enumerate(self.base)]
        result = self.score(changed)
        self.assertGreater(result["channel_results"][0]["max_silence_window_actual_rms"], 0.0005)
        self.assertFalse(result["passed"])

    def test_duration_and_channel_count_fail_closed(self):
        with self.assertRaises(AudioTestError):
            self.score(self.base[:-1000])
        mono = self.root / "mono.wav"
        with wave.open(str(mono), "wb") as writer:
            writer.setnchannels(1)
            writer.setsampwidth(2)
            writer.setframerate(48000)
            writer.writeframes(b"\x00\x00" * 48000)
        with self.assertRaises(AudioTestError):
            compare(mono, self.ref, self.policy)

    def test_aac_packet_padding_is_not_presentation_duration_drift(self):
        encoded = self.root / "encoded.m4a"
        subprocess.run(["ffmpeg", "-v", "error", "-nostdin", "-i", str(self.ref),
                        "-c:a", "aac", str(encoded)], check=True)
        rate, channels, samples = decode(encoded)
        self.assertEqual((rate, channels, len(samples)), (48000, 2, 96000))

    def test_nonfinite_data_rejected(self):
        # Float WAV with NaN encoded as actual sample, not just invalid metadata.
        raw = self.root / "nan.f32"
        raw.write_bytes(struct.pack("<f", float("nan")) * 96000)
        float_wav = self.root / "nan.wav"
        subprocess.run(["ffmpeg", "-v", "error", "-f", "f32le", "-ar", "48000", "-ac", "2",
                        "-i", str(raw), "-c:a", "pcm_f32le", str(float_wav)], check=True)
        with self.assertRaises(AudioTestError):
            decode(float_wav)

    def test_delayed_presentation_timestamp_rejected_before_pcm_decode(self):
        # Identical payload would pass if MP4's delayed audio PTS were discarded.
        probe = {"streams": [{"codec_type": "audio", "sample_rate": "48000",
                              "channels": 2, "duration": "1", "start_time": "0.250000"}],
                 "format": {"format_name": "mov,mp4,m4a,3gp,3g2,mj2", "duration": "1",
                            "start_time": "0.000000"}}
        with patch("aep_audio_test._run", return_value=json.dumps(probe).encode()), \
             patch("aep_audio_test.subprocess.run") as decoder:
            with self.assertRaisesRegex(AudioTestError, "nonzero.*audio stream start_time"):
                decode(self.ref)
            decoder.assert_not_called()
        probe["streams"][0].pop("start_time")
        with patch("aep_audio_test._run", return_value=json.dumps(probe).encode()), \
             patch("aep_audio_test.subprocess.run") as decoder:
            with self.assertRaisesRegex(AudioTestError, "missing.*audio stream start_time"):
                decode(self.ref)
            decoder.assert_not_called()
        probe["streams"][0]["start_time"] = "0"
        probe["format"]["start_time"] = "-0.02"
        with patch("aep_audio_test._run", return_value=json.dumps(probe).encode()), \
             patch("aep_audio_test.subprocess.run") as decoder:
            with self.assertRaisesRegex(AudioTestError, "nonzero.*container start_time"):
                decode(self.ref)
            decoder.assert_not_called()
        # WAV without timestamp fields is allowed; the real PCM WAV is decodable.
        probe["format"]["format_name"] = "wav"
        probe["format"].pop("start_time")
        probe["streams"][0].pop("start_time")
        with patch("aep_audio_test._run", return_value=json.dumps(probe).encode()):
            self.assertEqual(decode(self.ref)[0:2], (48000, 2))

    def test_missing_stream_and_invalid_policy_rejected(self):
        with self.assertRaises(AudioTestError):
            decode(self.root / "missing.wav")
        with self.assertRaises(AudioTestError):
            validate_policy({**self.policy, "window_rms_error_max": float("nan")})

    def test_cli_direction_hash_and_pending_gate(self):
        self.score(self.base)
        source = self.root / "source.aep"
        source.write_bytes(b"pinned native source identity")
        fx = self.root / "edited.tsrct"
        fx.write_bytes(b"explicit edited FX identity")
        cases = [{"id": "import", "direction": "import", "oracle_source_sha256": digest(source),
                  "composition_id": 63, "reference_sha256": digest(self.ref),
                  "edited_fx_sha256": "", "status": "UNRUN"},
                 {"id": "export", "direction": "export", "oracle_source_sha256": digest(source),
                  "composition_id": 12, "reference_sha256": digest(self.ref),
                  "edited_fx_sha256": digest(fx), "status": "UNRUN"}]
        manifest = self.root / "cases.json"
        manifest.write_text(json.dumps({"version": 1, "policy": self.policy, "cases": cases}))
        common = [sys.executable, str(SCRIPT), "compare", "--manifest", str(manifest),
                  "--source", str(source), "--actual", str(self.actual), "--reference", str(self.ref)]
        def run(case, direction, *extra):
            return subprocess.run(common + ["--case", case, "--direction", direction, *extra],
                                  capture_output=True, text=True)
        self.assertEqual(run("import", "import").returncode, 0)
        self.assertEqual(run("export", "export", "--fx-input", str(fx)).returncode, 0)
        self.assertNotEqual(subprocess.run(common[:-1] + [str(self.actual), "--case", "import",
                                             "--direction", "import"], capture_output=True).returncode, 0)
        self.assertNotEqual(run("export", "export").returncode, 0)
        source.write_bytes(b"drift")
        self.assertNotEqual(run("import", "import").returncode, 0)
        source.write_bytes(b"pinned native source identity")
        self.ref.write_bytes(b"drift")
        self.assertNotEqual(run("import", "import").returncode, 0)
        cases[0]["reference_sha256"] = ""
        manifest.write_text(json.dumps({"version": 1, "policy": self.policy, "cases": cases}))
        self.assertNotEqual(run("import", "import").returncode, 0)


if __name__ == "__main__":
    unittest.main()
