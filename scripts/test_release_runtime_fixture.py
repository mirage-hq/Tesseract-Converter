"""The release smoke's video source must not depend on FFmpeg input devices."""

from pathlib import Path
import tempfile
import unittest

from test_release_runtime import raw_video


class RawVideoFixtureTests(unittest.TestCase):
    def test_opaque_and_alpha_frames(self):
        with tempfile.TemporaryDirectory() as temporary:
            for alpha, pixel_format, first, middle, last in (
                (False, "rgb24", bytes((0, 0, 128)), bytes((128, 0, 128)), bytes((255, 143, 128))),
                (True, "argb", bytes((0, 0, 0, 128)), bytes((128, 128, 0, 128)), bytes((255, 255, 143, 128))),
            ):
                with self.subTest(alpha=alpha):
                    path = Path(temporary) / ("alpha.raw" if alpha else "opaque.raw")
                    self.assertEqual(raw_video(path, alpha=alpha), pixel_format)
                    data = path.read_bytes()
                    channels = len(first)
                    frame_size = 256 * 144 * channels
                    self.assertEqual(len(data), 12 * frame_size)
                    self.assertEqual(data[:channels], first)
                    self.assertEqual(data[128 * channels:129 * channels], middle)
                    self.assertEqual(data[frame_size - channels:frame_size], last)
                    self.assertEqual(data[:frame_size], data[-frame_size:])


if __name__ == "__main__":
    unittest.main()
