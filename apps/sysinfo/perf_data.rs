//! Bundled benchmark categories, thresholds and measured references.
use serde_json::Value;
use std::sync::OnceLock;
pub fn catalog() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(
            r##"{
  "categories": [
    {
      "id": "cpu",
      "ids": [
        "cpu.sha256.1",
        "cpu.sha256.n",
        "cpu.matmul.1",
        "cpu.matmul.n",
        "cpu.xz.1",
        "cpu.xz.n",
        "cpu.js.1",
        "cpu.js.n",
        "cpu.ui.1",
        "cpu.ui.n",
        "cpu.shape.1",
        "cpu.shape.n"
      ]
    },
    {
      "id": "memory",
      "ids": [
        "mem.copy",
        "mem.triad",
        "mem.latency"
      ]
    },
    {
      "id": "gpu",
      "ids": [
        "gpu.scene",
        "gpu.fill",
        "gpu.upload",
        "gpu.layers"
      ]
    },
    {
      "id": "storage",
      "ids": [
        "disk.seqwrite",
        "disk.seqread",
        "disk.random4k"
      ]
    },
    {
      "id": "sustained",
      "ids": [
        "sustained.start",
        "sustained.end"
      ]
    },
    {
      "id": "video",
      "ids": [
        "video.h264.1080",
        "video.hevc.1080",
        "video.vp9.1080",
        "video.av1.1080",
        "video.h264.2160",
        "video.hevc.2160",
        "video.enc.h264",
        "video.enc.vp8"
      ]
    }
  ],
  "desktop": [
    {
      "id": "compositor",
      "ids": [
        "comp.fps",
        "comp.p50",
        "comp.p99",
        "comp.missed",
        "comp.in50",
        "comp.in99"
      ]
    },
    {
      "id": "desktop",
      "ids": [
        "desk.fps",
        "desk.p50",
        "desk.p99",
        "desk.missed",
        "desk.in50",
        "desk.in99",
        "desk.work"
      ]
    }
  ],
  "sandbox": [
    "helperSocket",
    "helperInet",
    "helperExec",
    "helperFork",
    "helperNewFile",
    "helperRead"
  ],
  "landlock": [
    "helperNewFile",
    "helperRead"
  ],
  "events": [
    "underVoltage",
    "capped",
    "throttled",
    "softLimit",
    "tempLimit"
  ],
  "engine": [
    "cpu.ui.1",
    "cpu.ui.n",
    "cpu.shape.1",
    "cpu.shape.n"
  ],
  "references": [
    {
      "id": "vm",
      "lordbench": 1,
      "mode": "full",
      "results": {
        "cpu.sha256.1": {
          "value": 165.951
        },
        "cpu.sha256.n": {
          "value": 984.265
        },
        "cpu.matmul.1": {
          "value": 3.31858
        },
        "cpu.matmul.n": {
          "value": 19.9376
        },
        "cpu.xz.1": {
          "value": 60.2491
        },
        "cpu.xz.n": {
          "value": 362.977
        },
        "cpu.js.1": {
          "value": 56.6665
        },
        "cpu.js.n": {
          "value": 335.945
        },
        "mem.copy": {
          "value": 20.0074
        },
        "mem.triad": {
          "value": 22.8006
        },
        "mem.latency": {
          "value": 90.9222
        },
        "gpu.scene": {
          "value": 802.066
        },
        "gpu.fill": {
          "value": 406.961
        },
        "gpu.upload": {
          "value": 9456.65
        },
        "gpu.layers": {
          "value": 22.925
        },
        "disk.seqwrite": {
          "value": 378.126
        },
        "disk.seqread": {
          "value": 516.647
        },
        "disk.random4k": {
          "value": 6256.32
        },
        "sustained.start": {
          "value": 19.5814
        },
        "sustained.end": {
          "value": 19.77
        },
        "video.h264.1080": {
          "value": 399.438
        },
        "video.hevc.1080": {
          "value": 385.477
        },
        "video.vp9.1080": {
          "value": 215.005
        },
        "video.av1.1080": {
          "value": 200.887
        },
        "video.h264.2160": {
          "value": 75.624
        },
        "video.hevc.2160": {
          "value": 102.252
        },
        "video.enc.h264": {
          "value": 106.12
        },
        "video.enc.vp8": {
          "value": 54.175
        },
        "cpu.ui.1": {
          "value": 36.9702,
          "lordos": "0.8.5-beta"
        },
        "cpu.ui.n": {
          "value": 39.9302,
          "lordos": "0.8.5-beta"
        },
        "cpu.shape.1": {
          "value": 618.791,
          "lordos": "0.8.5-beta"
        },
        "cpu.shape.n": {
          "value": 3595.11,
          "lordos": "0.8.5-beta"
        }
      }
    },
    {
      "id": "pi5",
      "lordbench": 1,
      "mode": "full",
      "results": {
        "cpu.sha256.1": {
          "value": 933.43
        },
        "cpu.sha256.n": {
          "value": 3692.78
        },
        "cpu.matmul.1": {
          "value": 2.26049
        },
        "cpu.matmul.n": {
          "value": 5.98807
        },
        "cpu.xz.1": {
          "value": 66.9597
        },
        "cpu.xz.n": {
          "value": 254.003
        },
        "cpu.js.1": {
          "value": 54.342
        },
        "cpu.js.n": {
          "value": 181.428
        },
        "mem.copy": {
          "value": 7.61055
        },
        "mem.triad": {
          "value": 7.29512
        },
        "mem.latency": {
          "value": 114.505
        },
        "gpu.scene": {
          "value": 795.282
        },
        "gpu.fill": {
          "value": 2454.68
        },
        "gpu.upload": {
          "value": 3897.05
        },
        "gpu.layers": {
          "value": 115.042
        },
        "disk.seqwrite": {
          "value": 65.9909
        },
        "disk.seqread": {
          "value": 90.5924
        },
        "disk.random4k": {
          "value": 2692.31
        },
        "sustained.start": {
          "value": 5.85622
        },
        "sustained.end": {
          "value": 5.87865
        },
        "video.h264.1080": {
          "value": 257.551
        },
        "video.hevc.1080": {
          "value": 246.029
        },
        "video.vp9.1080": {
          "value": 204.453
        },
        "video.av1.1080": {
          "value": 159.402
        },
        "video.h264.2160": {
          "value": 55.007
        },
        "video.hevc.2160": {
          "value": 57.213
        },
        "video.enc.h264": {
          "value": 75.332
        },
        "video.enc.vp8": {
          "value": 54.526
        },
        "cpu.ui.1": {
          "value": 39.2238,
          "lordos": "0.8.5-beta"
        },
        "cpu.ui.n": {
          "value": 45.2309,
          "lordos": "0.8.5-beta"
        },
        "cpu.shape.1": {
          "value": 628.321,
          "lordos": "0.8.5-beta"
        },
        "cpu.shape.n": {
          "value": 2514.02,
          "lordos": "0.8.5-beta"
        }
      }
    }
  ],
  "missedWarn": 5,
  "missedFail": 25,
  "inputWarn": 3
}"##,
        )
        .expect("benchmark catalog")
    })
}
