# Face model files

The processor uses official OpenCV Zoo ONNX models and Google's MediaPipe Face
Landmarker task bundle. The binary model files are ignored by Git and must be
downloaded before processing photos.

| File | SHA-256 | License |
| --- | --- | --- |
| `face_detection_yunet_2023mar.onnx` | `8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4` | MIT |
| `face_landmarker.task` | `64184e229b263107bc2b804c6625db1341ff2bb731874b0bcc2fe6544e0bc9ff` | Apache-2.0 |
| `face_recognition_sface_2021dec.onnx` | `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79` | Apache-2.0 |

Sources:

- <https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet>
- <https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/latest/face_landmarker.task>
- <https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface>

The OpenCV Zoo directory licenses cover its files; the MediaPipe bundle is from
Google's official model bucket. Before commercial use, the SFace model's
training-data provenance should receive a separate legal review; the upstream
model documentation does not identify it precisely.
