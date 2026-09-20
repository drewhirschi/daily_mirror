"""Synthetic checks for registration and actual noise reduction."""
import tempfile
import unittest
from pathlib import Path
import cv2
import numpy as np
from process_burst_experiment import align, process


class BurstChecks(unittest.TestCase):
    def test_translation_restores_known_geometry(self):
        rng = np.random.default_rng(7)
        ref = cv2.GaussianBlur(rng.integers(0, 256, (320, 320, 3), dtype=np.uint8), (0, 0), 1)
        candidate = cv2.warpAffine(ref, np.float32([[1, 0, 4], [0, 1, -4]]), (320, 320))
        result, valid, _, _ = align(ref, candidate)
        self.assertLess(np.abs(result[16:-16,16:-16].astype(float)-ref[16:-16,16:-16]).mean(), 1)
        self.assertFalse(valid.all())

    def test_merging_reduces_error_against_known_clean_image(self):
        rng = np.random.default_rng(8)
        clean = cv2.GaussianBlur(rng.integers(40, 210, (320, 320, 3), dtype=np.uint8), (0, 0), 2)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)
            for i in range(5):
                noisy = np.clip(clean.astype(float)+rng.normal(0, 3, clean.shape), 0, 255).astype(np.uint8)
                cv2.imwrite(str(path/f'frame-{i:02d}.jpg'),noisy,[cv2.IMWRITE_JPEG_QUALITY,100])
            report=process(path)
            best=cv2.imread(str(path/report['best_frame']))
            merged=cv2.imread(str(path/'motion-gated-mean.png'))
            region=np.s_[20:-20,20:-20]
            before=np.mean((best[region].astype(float)-clean[region])**2)
            after=np.mean((merged[region].astype(float)-clean[region])**2)
            self.assertLess(after, before*.8)

    def test_changed_foreground_is_not_averaged_into_reference(self):
        rng = np.random.default_rng(9)
        clean = cv2.GaussianBlur(rng.integers(20, 235, (400, 400, 3), dtype=np.uint8), (0, 0), 1)
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder)
            for i in range(5):
                frame = clean.copy()
                if i == 1:
                    frame[70:95, 30:55] = np.clip(frame[70:95, 30:55].astype(int) + 40, 0, 255)
                cv2.imwrite(str(path/f'frame-{i:02d}.jpg'), frame, [cv2.IMWRITE_JPEG_QUALITY,100])
            report = process(path)
            self.assertEqual(report['best_frame'], 'frame-00.jpg')
            best = cv2.imread(str(path/'best-frame.png')).astype(float)
            mean = cv2.imread(str(path/'aligned-mean.png')).astype(float)
            safe = cv2.imread(str(path/'motion-gated-mean.png')).astype(float)
            region = np.s_[76:89, 36:49]
            self.assertLess(np.abs(safe[region]-best[region]).mean(), 1.0)
            changed = next(item for item in report['alignments'] if item['file'] == 'frame-01.jpg')
            if 'rejected' not in changed:
                self.assertLess(np.abs(safe[region]-best[region]).mean(), np.abs(mean[region]-best[region]).mean() * .5)


if __name__ == '__main__':
    unittest.main()
